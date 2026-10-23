//! T2.2: the MCP server over real HTTP. Auth/Origin/Host rejections, then `pane_list` (whose `you` is the caller)
//! under both protocol versions.
//! T2.3: tokens come from the pane registry; a closed pane's token is refused.

use std::sync::Arc;

use agentdeck_lib::delivery::Engine;
use agentdeck_lib::mcp::server::{self, INSTRUCTIONS};
use agentdeck_lib::panes::{self, Panes, Spec};
use agentdeck_lib::pty::PaneManager;
use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

struct Fixture {
    url: String,
    p1: String,
    p2: String,
}

/// A registered agent pane with no process: enough for the server, which only maps tokens.
pub fn agent(workspace: u32) -> Spec {
    Spec {
        parent: None,
        workspace,
        pane: None,
        agent: Some("codex".into()),
        title: "t".into(),
        cwd: "/".into(),
        task: None,
        rows: 24,
        cols: 80,
    }
}

async fn fixture() -> Fixture {
    let panes = Panes::default();
    let (p1, p2) = {
        let mut reg = panes::lock(&panes);
        let ws = reg.create_workspace();
        let mut open = |want: &str| {
            let o = reg.open(&agent(ws)).unwrap();
            assert_eq!(o.id, want);
            o.token.unwrap().expose().to_owned()
        };
        (open("p1"), open("p2"))
    };
    let port = server::start(engine(panes)).await.unwrap();
    Fixture {
        url: format!("http://127.0.0.1:{port}/mcp"),
        p1,
        p2,
    }
}

fn post(
    url: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: &Value,
) -> reqwest::RequestBuilder {
    let mut req = Client::new()
        .post(url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .json(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    req
}

/// JSON-RPC message from a JSON or SSE response body.
async fn rpc(resp: Response) -> Value {
    let status = resp.status();
    let body = resp.text().await.unwrap();
    assert_eq!(status, StatusCode::OK, "{body}");
    let data = body
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim)
        .find(|d| !d.is_empty())
        .unwrap_or(&body);
    serde_json::from_str(data).unwrap()
}

/// No PTYs, no log, no UI: the server only needs the registry here.
pub fn engine(panes: Panes) -> Engine {
    Engine::new(panes, PaneManager::default(), None, Arc::new(|_| {}))
}

/// `you` of a `pane_list` result: the caller's pane id.
fn you(msg: &Value) -> String {
    let text = msg["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let list: Value = serde_json::from_str(text).unwrap_or_default();
    list["you"].as_str().unwrap_or_default().to_owned()
}

fn init(version: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": version, "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}})
}

#[tokio::test]
async fn rejects_missing_or_wrong_token() {
    let f = fixture().await;
    let body = init("2025-06-18");
    let status = |t: Option<String>| {
        let req = post(&f.url, t.as_deref(), &[], &body);
        async move { req.send().await.unwrap().status() }
    };
    assert_eq!(status(None).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(Some("wrong".into())).await, StatusCode::UNAUTHORIZED);
    assert_eq!(
        status(Some(f.p1.to_uppercase())).await,
        StatusCode::UNAUTHORIZED
    );
    let resp = post(
        &f.url,
        None,
        &[("Authorization", &format!("Basic {}", f.p1))],
        &body,
    )
    .send()
    .await
    .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    // The 401 body never echoes the presented token.
    let resp = post(&f.url, Some(&format!("{}x", f.p1)), &[], &body)
        .send()
        .await
        .unwrap();
    assert!(!resp.text().await.unwrap().contains(&f.p1));
}

#[tokio::test]
async fn rejects_any_origin_and_foreign_host_even_with_valid_token() {
    let f = fixture().await;
    let body = init("2025-06-18");
    for origin in ["http://127.0.0.1", "https://evil.example", "null"] {
        let resp = post(&f.url, Some(&f.p1), &[("Origin", origin)], &body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Origin {origin}");
    }
    for host in ["evil.example", "attacker.test:80"] {
        let resp = post(&f.url, Some(&f.p1), &[("Host", host)], &body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "Host {host}");
    }
}

/// Codex 0.163.0: `initialize` handshake, then `Mcp-Session-Id` on every request.
#[tokio::test]
async fn legacy_2025_06_18_with_session() {
    let f = fixture().await;
    for (token, pane) in [(&f.p1, "p1"), (&f.p2, "p2")] {
        let resp = post(&f.url, Some(token), &[], &init("2025-06-18"))
            .send()
            .await
            .unwrap();
        let session = resp
            .headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let msg = rpc(resp).await;
        assert_eq!(msg["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(msg["result"]["serverInfo"]["name"], "agentdeck");
        assert_eq!(msg["result"]["instructions"], INSTRUCTIONS);

        let h = [
            ("Mcp-Session-Id", session.as_str()),
            ("MCP-Protocol-Version", "2025-06-18"),
        ];
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let resp = post(&f.url, Some(token), &h, &note).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "pane_list", "arguments": {}}});
        let msg = rpc(post(&f.url, Some(token), &h, &call).send().await.unwrap()).await;
        assert_eq!(you(&msg), pane);

        // Identity comes from the token, not the session: p1's session with p2's token answers p2.
        let other = if pane == "p1" { &f.p2 } else { &f.p1 };
        let msg = rpc(post(&f.url, Some(other), &h, &call).send().await.unwrap()).await;
        assert_ne!(you(&msg), pane);
        // A request without a token never reaches the session.
        let resp = post(&f.url, None, &h, &call).send().await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}

/// Claude Code: `2026-07-28` is stateless (SEP-2567); no handshake, protocol metadata on every request.
#[tokio::test]
async fn stateless_2026_07_28() {
    let f = fixture().await;
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    for (token, pane) in [(&f.p1, "p1"), (&f.p2, "p2")] {
        let discover = json!({"jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": {"_meta": meta}});
        let h = [
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "server/discover"),
        ];
        let resp = post(&f.url, Some(token), &h, &discover)
            .send()
            .await
            .unwrap();
        assert!(resp.headers().get("mcp-session-id").is_none());
        let msg = rpc(resp).await;
        assert!(
            msg["result"]["supportedVersions"]
                .as_array()
                .unwrap()
                .contains(&json!("2026-07-28")),
            "{msg}"
        );
        assert_eq!(msg["result"]["instructions"], INSTRUCTIONS);

        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "pane_list", "arguments": {}, "_meta": meta}});
        let h = [
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tools/call"),
            ("Mcp-Name", "pane_list"),
        ];
        let resp = post(&f.url, Some(token), &h, &call).send().await.unwrap();
        assert!(resp.headers().get("mcp-session-id").is_none());
        let msg = rpc(resp).await;
        assert_eq!(you(&msg), pane, "{msg}");
    }
    // A client that still opens with `initialize` naming 2026-07-28 gets a handshake version back, not an error.
    let msg = rpc(post(&f.url, Some(&f.p1), &[], &init("2026-07-28"))
        .send()
        .await
        .unwrap())
    .await;
    assert_eq!(msg["result"]["instructions"], INSTRUCTIONS, "{msg}");
}

/// T2.3: closing a pane revokes its token at once; the other pane keeps working.
#[tokio::test]
async fn closed_pane_token_is_refused() {
    let panes = Panes::default();
    let (t1, t2) = {
        let mut reg = panes::lock(&panes);
        let ws = reg.create_workspace();
        let t1 = reg.open(&agent(ws)).unwrap().token.unwrap();
        let t2 = reg.open(&agent(ws)).unwrap().token.unwrap();
        (t1.expose().to_owned(), t2.expose().to_owned())
    };
    let url = format!(
        "http://127.0.0.1:{}/mcp",
        server::start(engine(panes.clone())).await.unwrap()
    );
    let body = init("2025-06-18");
    let status = |t: &str| {
        let req = post(&url, Some(t), &[], &body);
        async move { req.send().await.unwrap().status() }
    };
    assert_eq!(status(&t1).await, StatusCode::OK);
    panes::lock(&panes).close("p1");
    assert_eq!(status(&t1).await, StatusCode::UNAUTHORIZED);
    assert_eq!(status(&t2).await, StatusCode::OK);
}

/// T2.4/T2.7: tools are listed with their annotations, and a refused call is a tool result with `isError` and a
/// `CODE: ...` text the agent can read, not a JSON-RPC error.
#[tokio::test]
async fn tools_listed_and_errors_are_tool_results() {
    let f = fixture().await;
    let meta = json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": {"name": "test", "version": "0"},
        "io.modelcontextprotocol/clientCapabilities": {}
    });
    let h = |method: &'static str| {
        [
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", method),
        ]
    };
    let list =
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {"_meta": meta}});
    let msg = rpc(post(&f.url, Some(&f.p1), &h("tools/list"), &list)
        .send()
        .await
        .unwrap())
    .await;
    let tools = msg["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "pane_list",
            "pane_read",
            "pane_spawn",
            "pane_send",
            "pane_report",
            "pane_claim"
        ],
        "{msg}"
    );
    assert_eq!(tools[1]["annotations"]["readOnlyHint"], true);
    assert_eq!(tools[2]["annotations"]["destructiveHint"], false);

    for (args, code) in [
        (json!({"to": "p1", "text": "hi"}), "SELF: "),
        (json!({"to": "p2", "text": "hi"}), "PEER_SEND_OFF: "),
        (json!({"to": "p2"}), "BAD_ARGS: "),
    ] {
        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "pane_send", "arguments": args, "_meta": meta}});
        let [v, m] = h("tools/call");
        let hc = [v, m, ("Mcp-Name", "pane_send")];
        let msg = rpc(post(&f.url, Some(&f.p1), &hc, &call).send().await.unwrap()).await;
        assert_eq!(msg["result"]["isError"], true, "{msg}");
        let text = msg["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with(code), "{text}");
        assert!(!text.contains(&f.p1), "no token in errors");
    }

    // D-033: p1 claims a folder; p2's claim inside it is refused naming p1, and pane_list shows p1's claim.
    let call = |token: &str, name: &'static str, args: Value| {
        let body = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": name, "arguments": args, "_meta": meta}});
        let [v, m] = h("tools/call");
        let req = post(&f.url, Some(token), &[v, m, ("Mcp-Name", name)], &body);
        async move { rpc(req.send().await.unwrap()).await }
    };
    let ok = call(&f.p1, "pane_claim", json!({"paths": ["agentdeck-it/src"]})).await;
    assert_eq!(ok["result"]["isError"], false, "{ok}");
    let refused = call(
        &f.p2,
        "pane_claim",
        json!({"paths": ["agentdeck-it/src/a.rs"]}),
    )
    .await;
    assert_eq!(refused["result"]["isError"], true, "{refused}");
    let text = refused["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.starts_with("CLAIMED: ") && text.contains("claimed by p1"),
        "{text}"
    );
    let list = call(&f.p2, "pane_list", json!({})).await;
    let list: Value =
        serde_json::from_str(list["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(list["panes"][0]["claims"], json!(["/agentdeck-it/src"]));
}
