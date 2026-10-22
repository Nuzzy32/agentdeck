//! The agent-facing tools (MCP-TOOLS): `pane_list`, `pane_read`, `pane_send`, `pane_report`. Arguments are parsed here, not by
//! rmcp, so a bad argument is a readable `BAD_ARGS` tool error rather than a JSON-RPC error. Every error is
//! `CODE: explanation. what to do.` and becomes a tool result with `isError: true`.

use std::sync::Arc;
use std::time::Instant;

use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::report::Reports;
use crate::delivery::Engine;
use crate::panes;

/// `pane_read` line count (MCP-TOOLS): 1–400, default 60.
const READ_LINES_MAX: u64 = 400;
const READ_LINES_DEFAULT: u64 = 60;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoArgs {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    pane: String,
    lines: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SendArgs {
    to: String,
    text: String,
}

fn schema(v: Value) -> Arc<JsonObject> {
    match v {
        Value::Object(o) => Arc::new(o),
        _ => Arc::default(),
    }
}

fn read_only() -> ToolAnnotations {
    ToolAnnotations::new().read_only(true)
}

fn sends() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(false)
        .idempotent(false)
        .open_world(false)
}

pub fn list() -> Vec<Tool> {
    vec![
        Tool::new(
            "pane_list",
            "List the panes in your workspace: id, title, agent, status, your relation to each, and whether you may \
             send to (can_send) or read (can_read) it.",
            schema(json!({"type": "object", "properties": {}, "additionalProperties": false})),
        )
        .with_annotations(read_only()),
        Tool::new(
            "pane_read",
            "Read the plain text of another pane's screen (bottom lines plus scrollback). Only panes with can_read. \
             Full-screen agents have no scrollback, so at most one screen height comes back. Limit: 30 calls per minute.",
            schema(json!({
                "type": "object",
                "properties": {
                    "pane": {"type": "string", "description": "Pane id from pane_list, e.g. \"p2\"."},
                    "lines": {"type": "integer", "minimum": 1, "maximum": READ_LINES_MAX, "default": READ_LINES_DEFAULT}
                },
                "required": ["pane"],
                "additionalProperties": false
            })),
        )
        .with_annotations(read_only()),
        Tool::new(
            "pane_send",
            "Send one prompt to another agent: your child, your parent (to: \"parent\"), or a peer when the person \
             allows it. It is pasted into that agent's terminal when it is idle, after a line naming you as sender. \
             Max 8192 bytes; point to files or commits instead of pasting long logs. Result state: queued, delivered, \
             or paused (the loop guard holds it until the person decides).",
            schema(json!({
                "type": "object",
                "properties": {
                    "to": {"type": "string", "description": "Pane id from pane_list, or \"parent\"."},
                    "text": {"type": "string", "description": "The prompt, 1 to 8192 bytes."}
                },
                "required": ["to", "text"],
                "additionalProperties": false
            })),
        )
        .with_annotations(sends()),
        Tool::new(
            "pane_report",
            "Report your result to the agent that opened your pane (your parent), once, when the task is finished. \
             It arrives there as a line `RESULT status=... files=... tests=...` followed by your summary. Only for \
             panes opened by another agent. Limit: 6 calls per minute.",
            schema(json!({
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["done", "failed", "blocked"]},
                    "summary": {"type": "string", "description": "What you did and what is left, at most 2048 bytes."},
                    "files": {"type": "array", "items": {"type": "string"}, "description": "Paths you changed."},
                    "tests": {"type": "string", "enum": ["pass", "fail", "none"]}
                },
                "required": ["status", "summary"],
                "additionalProperties": false
            })),
        )
        .with_annotations(sends()),
    ]
}

fn args<T: DeserializeOwned>(raw: Option<JsonObject>) -> Result<T, String> {
    serde_json::from_value(Value::Object(raw.unwrap_or_default()))
        .map_err(|e| format!("BAD_ARGS: {e}. Fix the arguments."))
}

/// Runs tool `name` for pane `caller` (from its token). `Ok` and `Err` are both the text the agent reads.
/// Blocking: `pane_send` and `pane_report` may write a paste and wait the Enter delay.
pub fn call(
    engine: &Engine,
    reports: &Reports,
    caller: &str,
    name: &str,
    raw: Option<JsonObject>,
) -> Result<String, String> {
    match name {
        "pane_list" => {
            args::<NoArgs>(raw)?;
            let list = panes::lock(&engine.panes)
                .list(caller)
                .ok_or("UNKNOWN_PANE: your pane is gone. Tell the person.")?;
            serde_json::to_string(&list).map_err(|e| e.to_string())
        }
        "pane_read" => {
            let a: ReadArgs = args(raw)?;
            let lines = a.lines.unwrap_or(READ_LINES_DEFAULT);
            if !(1..=READ_LINES_MAX).contains(&lines) {
                return Err(format!(
                    "BAD_ARGS: lines must be 1 to {READ_LINES_MAX}. Fix the arguments."
                ));
            }
            let (status, screen) =
                panes::lock(&engine.panes).read(caller, &a.pane, Instant::now())?;
            let text = screen.map_or_else(String::new, |s| {
                s.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .read(usize::try_from(lines).unwrap_or(usize::MAX))
            });
            Ok(json!({"pane": a.pane, "status": status, "text": text}).to_string())
        }
        "pane_send" => {
            let a: SendArgs = args(raw)?;
            let (id, state) = engine.send(caller, &a.to, &a.text)?;
            Ok(json!({"message": id, "state": state}).to_string())
        }
        "pane_report" => {
            let (id, state) = reports.send(engine, caller, args(raw)?)?;
            Ok(json!({"message": id, "state": state}).to_string())
        }
        _ => Err(format!(
            "BAD_ARGS: no tool named {name:?}. Use pane_list, pane_read, pane_send or pane_report."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panes::{Panes, Spec};
    use crate::pty::{PaneEvent, PaneManager};

    fn spec(ws: u32, agent: Option<&str>, parent: Option<&str>) -> Spec {
        Spec {
            parent: parent.map(str::to_owned),
            workspace: ws,
            pane: None,
            agent: agent.map(str::to_owned),
            title: "t".into(),
            cwd: "/".into(),
            task: None,
            rows: 24,
            cols: 80,
        }
    }

    /// p1 claude, p2 its child, p3 a peer, p4 a shell, p5 in another workspace, p6 an exited child of p1.
    /// No processes: nothing is ever delivered, messages stay queued.
    fn engine() -> (Engine, u32) {
        let panes = Panes::default();
        let ws = {
            let mut r = panes::lock(&panes);
            let ws = r.create_workspace();
            let other = r.create_workspace();
            for s in [
                spec(ws, Some("claude-code"), None),
                spec(ws, Some("codex"), Some("p1")),
                spec(ws, Some("codex"), None),
                spec(ws, None, None),
                spec(other, Some("codex"), None),
                spec(ws, Some("codex"), Some("p1")),
            ] {
                r.open(&s).unwrap();
            }
            r.event("p6", 0, &PaneEvent::Exited { code: Some(0) });
            ws
        };
        let e = Engine::new(panes, PaneManager::default(), None, Arc::new(|_| {}));
        (e, ws)
    }

    fn call_json(e: &Engine, caller: &str, name: &str, args: Value) -> Result<Value, String> {
        let Value::Object(a) = args else {
            panic!("args")
        };
        let r = Reports::new(Arc::new(|_| {}));
        call(e, &r, caller, name, Some(a)).map(|t| serde_json::from_str(&t).unwrap())
    }

    fn code(r: Result<Value, String>) -> String {
        let e = r.expect_err("expected an error");
        let (code, rest) = e.split_once(": ").unwrap();
        assert!(rest.ends_with('.'), "{e}");
        code.to_owned()
    }

    fn send(e: &Engine, from: &str, to: &str, text: &str) -> Result<Value, String> {
        call_json(e, from, "pane_send", json!({"to": to, "text": text}))
    }

    #[test]
    fn annotations_follow_mcp_tools() {
        let tools = list();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
        assert_eq!(
            names,
            ["pane_list", "pane_read", "pane_send", "pane_report"]
        );
        for t in &tools[..2] {
            assert_eq!(t.annotations.as_ref().unwrap().read_only_hint, Some(true));
        }
        for t in &tools[2..] {
            let a = t.annotations.as_ref().unwrap();
            assert_eq!(
                (
                    a.read_only_hint,
                    a.destructive_hint,
                    a.idempotent_hint,
                    a.open_world_hint
                ),
                (Some(false), Some(false), Some(false), Some(false))
            );
        }
        for t in &tools {
            assert_eq!(t.input_schema["additionalProperties"], json!(false));
        }
    }

    #[test]
    fn pane_list_shows_workspace_and_rejects_arguments() {
        let (e, _) = engine();
        let l = call_json(&e, "p1", "pane_list", json!({})).unwrap();
        assert_eq!(l["you"], "p1");
        let ids: Vec<&str> = l["panes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["p1", "p2", "p3", "p4", "p6"]);
        assert_eq!(l["panes"][1]["relation"], "child");
        assert_eq!(l["peer_messaging"], false);
        let r = Reports::new(Arc::new(|_| {}));
        assert_eq!(call(&e, &r, "p1", "pane_list", None).map(|_| ()), Ok(()));
        assert_eq!(
            code(call_json(&e, "p1", "pane_list", json!({"x": 1}))),
            "BAD_ARGS"
        );
        assert!(
            call(&e, &r, "p1", "whoami", None)
                .unwrap_err()
                .starts_with("BAD_ARGS: ")
        );
    }

    #[test]
    fn pane_send_relations_and_every_error_code() {
        let (e, ws) = engine();
        // §5.4: parent → own child, child → "parent".
        let r = send(&e, "p1", "p2", "review this").unwrap();
        assert_eq!(
            (r["message"].as_str(), r["state"].as_str()),
            (Some("m1"), Some("queued"))
        );
        assert_eq!(
            send(&e, "p2", "parent", "RESULT ok").unwrap()["state"],
            "queued"
        );
        assert_eq!(code(send(&e, "p1", "p9", "x")), "UNKNOWN_PANE");
        assert_eq!(code(send(&e, "p1", "p5", "x")), "NOT_ALLOWED");
        assert_eq!(code(send(&e, "p1", "p3", "x")), "PEER_SEND_OFF");
        assert_eq!(code(send(&e, "p1", "p4", "x")), "TARGET_SHELL");
        assert_eq!(code(send(&e, "p1", "p6", "x")), "TARGET_EXITED");
        assert_eq!(code(send(&e, "p1", "parent", "x")), "NO_PARENT");
        assert_eq!(code(send(&e, "p1", "p1", "x")), "SELF");
        assert_eq!(code(send(&e, "p1", "p2", "\x1b[201~\r")), "BAD_ARGS");
        assert_eq!(
            code(call_json(&e, "p1", "pane_send", json!({"to": "p2"}))),
            "BAD_ARGS"
        );
        assert_eq!(
            code(call_json(
                &e,
                "p1",
                "pane_send",
                json!({"to": "p2", "text": "x", "from": "p3"})
            )),
            "BAD_ARGS"
        );
        assert_eq!(code(send(&e, "p1", "p2", &"a".repeat(8193))), "TOO_LARGE");
        // Peer switch on: the same send goes through.
        assert!(panes::lock(&e.panes).set_peer(ws, true));
        assert_eq!(send(&e, "p1", "p3", "x").unwrap()["state"], "queued");
        // 8192 bytes fit; the queue of p2 then holds 5 (m1 + four) and the sixth is refused.
        assert_eq!(
            send(&e, "p1", "p2", &"a".repeat(8192)).unwrap()["state"],
            "queued"
        );
        send(&e, "p1", "p2", "3").unwrap();
        send(&e, "p1", "p2", "4").unwrap();
        send(&e, "p3", "p2", "5").unwrap();
        assert_eq!(code(send(&e, "p1", "p2", "6")), "QUEUE_FULL");
        let rows = panes::lock(&e.panes).messages(ws);
        assert_eq!(rows.len(), 7);
        assert!(rows.iter().all(|r| r.state == "queued"));
        assert_eq!(rows[0].from, "p1");
        assert_eq!(rows[1].to, "p1", "\"parent\" resolves to the pane id");
    }

    #[test]
    fn pane_read_permissions_lines_and_rate() {
        let (e, ws) = engine();
        let screen = panes::lock(&e.panes).screen("p2").unwrap();
        screen.lock().unwrap().process(b"one\r\ntwo\r\nthree");
        let r = call_json(&e, "p1", "pane_read", json!({"pane": "p2", "lines": 2})).unwrap();
        assert_eq!(r["pane"], "p2");
        assert_eq!(r["text"], "two\nthree");
        assert_eq!(r["status"], "starting");
        assert_eq!(
            call_json(&e, "p2", "pane_read", json!({"pane": "p1"})).unwrap()["text"],
            ""
        );
        // An exited child stays readable.
        assert!(call_json(&e, "p1", "pane_read", json!({"pane": "p6"})).is_ok());
        assert_eq!(
            code(call_json(&e, "p1", "pane_read", json!({"pane": "p9"}))),
            "UNKNOWN_PANE"
        );
        for (pane, why) in [
            ("p3", "peer messaging is off"),
            ("p4", "shell"),
            ("p5", "another workspace"),
            ("p1", "your own pane"),
        ] {
            let err = call_json(&e, "p1", "pane_read", json!({"pane": pane})).unwrap_err();
            assert!(
                err.starts_with("NOT_ALLOWED: ") && err.contains(why),
                "{pane}: {err}"
            );
        }
        for lines in [0, 401] {
            let r = call_json(&e, "p1", "pane_read", json!({"pane": "p2", "lines": lines}));
            assert_eq!(code(r), "BAD_ARGS");
        }
        assert_eq!(
            code(call_json(
                &e,
                "p1",
                "pane_read",
                json!({"pane": "p2", "x": 1})
            )),
            "BAD_ARGS"
        );
        assert!(panes::lock(&e.panes).set_peer(ws, true));
        assert!(call_json(&e, "p1", "pane_read", json!({"pane": "p3"})).is_ok());
        // p3 has made no call yet: 30 pass, the 31st is READ_RATE; other callers are unaffected.
        for _ in 0..30 {
            assert!(call_json(&e, "p3", "pane_read", json!({"pane": "p1"})).is_ok());
        }
        let err = call_json(&e, "p3", "pane_read", json!({"pane": "p1"})).unwrap_err();
        assert!(
            err.starts_with("READ_RATE: ") && err.contains("Wait 60 seconds"),
            "{err}"
        );
        assert!(call_json(&e, "p2", "pane_read", json!({"pane": "p1"})).is_ok());
    }

    #[test]
    fn pane_report_goes_to_parent_and_every_error_code() {
        use std::sync::Mutex;
        let (e, ws) = engine();
        let heard = Arc::new(Mutex::new(Vec::new()));
        let sink = heard.clone();
        let r = Reports::new(Arc::new(move |rep| sink.lock().unwrap().push(rep.clone())));
        let report = |caller: &str, args: Value| -> Result<Value, String> {
            let Value::Object(a) = args else {
                panic!("args")
            };
            call(&e, &r, caller, "pane_report", Some(a)).map(|t| serde_json::from_str(&t).unwrap())
        };
        let ok = json!({"status": "done", "summary": "all good", "files": ["a.rs", "b.rs"], "tests": "pass"});
        // Person-opened pane: NO_PARENT, and nothing stored or heard.
        assert_eq!(code(report("p1", ok.clone())), "NO_PARENT");
        assert_eq!(code(report("p1", json!({"status": "done"}))), "BAD_ARGS");
        assert_eq!(
            code(report(
                "p2",
                json!({"status": "done", "summary": "s", "to": "p3"})
            )),
            "BAD_ARGS"
        );
        assert_eq!(
            code(report(
                "p2",
                json!({"status": "done", "summary": "a".repeat(2049)})
            )),
            "BAD_ARGS"
        );
        assert!(heard.lock().unwrap().is_empty());
        assert_eq!(panes::lock(&e.panes).report("p2"), None);

        let v = report("p2", ok.clone()).unwrap();
        assert_eq!(
            (v["message"].as_str(), v["state"].as_str()),
            (Some("m1"), Some("queued"))
        );
        let rows = panes::lock(&e.panes).messages(ws);
        assert_eq!((rows[0].from.as_str(), rows[0].to.as_str()), ("p2", "p1"));
        assert_eq!(
            rows[0].text,
            "RESULT status=done files=a.rs,b.rs tests=pass\nall good"
        );
        let last = panes::lock(&e.panes).report("p2").unwrap();
        assert_eq!(
            (last.status, last.tests, last.files.len()),
            ("done", "pass", 2)
        );
        assert_eq!(heard.lock().unwrap().as_slice(), [last]);

        // p1's queue holds 5; a refused send (QUEUE_FULL) does not use a REPORT_RATE slot.
        for _ in 0..4 {
            report("p2", ok.clone()).unwrap();
        }
        assert_eq!(code(report("p2", ok.clone())), "QUEUE_FULL");
        assert!(panes::lock(&e.panes).discard("m1").is_some());
        report("p2", ok.clone()).unwrap();
        let err = report("p2", ok.clone()).unwrap_err();
        assert!(
            err.starts_with("REPORT_RATE: ") && err.contains("Wait 6"),
            "{err}"
        );
        assert_eq!(heard.lock().unwrap().len(), 6);
    }
}
