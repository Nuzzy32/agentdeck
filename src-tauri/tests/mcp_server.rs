//! T2.2: the MCP server over real HTTP. Auth/Origin/Host rejections, then `whoami` under both protocol versions.
//! T2.3: tokens come from the pane registry; a closed pane's token is refused.

use agentdeck_lib::mcp::server::{self, INSTRUCTIONS};
use agentdeck_lib::panes::{self, Panes, Spec};
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
    let port = server::start(panes).await.unwrap();
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
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.text().await.unwrap();
    let data = body
        .lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim)
        .find(|d| !d.is_empty())
        .unwrap_or(&body);
    serde_json::from_str(data).unwrap()
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

        let call = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "whoami", "arguments": {}}});
        let msg = rpc(post(&f.url, Some(token), &h, &call).send().await.unwrap()).await;
        assert_eq!(msg["result"]["content"][0]["text"], pane);

        // Identity comes from the token, not the session: p1's session with p2's token answers p2.
        let other = if pane == "p1" { &f.p2 } else { &f.p1 };
        let msg = rpc(post(&f.url, Some(other), &h, &call).send().await.unwrap()).await;
        assert_ne!(msg["result"]["content"][0]["text"], pane);
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
            "params": {"name": "whoami", "arguments": {}, "_meta": meta}});
        let h = [
            ("MCP-Protocol-Version", "2026-07-28"),
            ("Mcp-Method", "tools/call"),
            ("Mcp-Name", "whoami"),
        ];
        let resp = post(&f.url, Some(token), &h, &call).send().await.unwrap();
        assert!(resp.headers().get("mcp-session-id").is_none());
        let msg = rpc(resp).await;
        assert_eq!(msg["result"]["content"][0]["text"], pane, "{msg}");
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
        server::start(panes.clone()).await.unwrap()
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
