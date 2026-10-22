//! Manual, costs quota: real Claude Code and Codex launched through `panes::start` (clean env, MCP injected) talk
//! over `pane_send` (ARCHITECTURE §12, T2.7). Run with
//! `cargo test --test real_cli -- --ignored --nocapture`. Prints the screens and message rows at the end.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use agentdeck_lib::catalogue;
use agentdeck_lib::delivery::Engine;
use agentdeck_lib::launch;
use agentdeck_lib::mcp::server;
use agentdeck_lib::panes::{self, Panes, Spec};
use agentdeck_lib::pty::PaneManager;

const ROWS: u16 = 40;
const COLS: u16 = 120;
const TURN: Duration = Duration::from_secs(180);

fn screen(panes: &Panes, pane: &str) -> String {
    let Some(s) = panes::lock(panes).screen(pane) else {
        return String::new();
    };
    s.lock().unwrap().read(ROWS as usize)
}

fn status(panes: &Panes, pane: &str) -> &'static str {
    let reg = panes::lock(panes);
    let list = reg.list(pane).unwrap();
    list.panes.iter().find(|p| p.id == pane).unwrap().status
}

fn wait(panes: &Panes, what: &str, done: impl Fn() -> bool) -> bool {
    let end = Instant::now() + TURN;
    let t0 = Instant::now();
    while !done() {
        let capacity = screen(panes, "p2").contains("at capacity");
        if Instant::now() > end || capacity {
            eprintln!(
                "TIMEOUT {what} (p1 {})\n--- p1 ---\n{}\n--- p2 ---\n{}",
                status(panes, "p1"),
                screen(panes, "p1"),
                screen(panes, "p2")
            );
            return false;
        }
        thread::sleep(Duration::from_millis(250));
    }
    eprintln!("ok {what} after {:.1}s", t0.elapsed().as_secs_f32());
    true
}

/// `filler` lines up to exactly 8192 bytes, then `last`.
fn max_message(last: &str) -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() + last.len() + 40 < 8192 {
        i += 1;
        s.push_str(&format!("filler line {i:04}: nothing to do here.\n"));
    }
    s.push_str(&"-".repeat(8192 - s.len() - last.len() - 1));
    s.push('\n');
    s.push_str(last);
    assert_eq!(s.len(), 8192);
    s
}

#[test]
#[ignore = "launches the real claude and codex; costs quota"]
fn claude_and_codex_round_trip() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let panes = Panes::default();
    let pm = PaneManager::default();
    let engine = Engine::new(panes.clone(), pm.clone(), None, Arc::new(|_| {}));
    let port = rt.block_on(server::start(engine.clone())).unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");
    // The repo root: trusted already, and its CLAUDE.md import (AGENTS.md) is inside cwd, so Claude shows no dialog.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let cwd = dir.canonicalize().unwrap().to_str().unwrap().to_owned();
    let file = format!("answer-{}.txt", std::process::id());
    // Codex's default model is often "at capacity" (D-013); a wrapper first on PATH adds `-m` (env MODEL, spikes only).
    let wrap = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("target/real-cli-bin-{}", std::process::id()));
    std::fs::create_dir_all(&wrap).unwrap();
    let real = catalogue::login_path()
        .split(':')
        .map(|d| PathBuf::from(d).join("codex"))
        .find(|p| p.is_file())
        .unwrap();
    let model = std::env::var("MODEL").unwrap_or_else(|_| "gpt-5.6-luna".into());
    let script = wrap.join("codex");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nexec '{}' -m '{model}' \"$@\"\n", real.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", wrap.display(), catalogue::login_path());
    let env = || launch::env(|k| std::env::var_os(k), &path);
    let ws = panes::lock(&panes).create_workspace();
    let spec = |agent: &str, parent: Option<&str>, task: Option<&str>| Spec {
        parent: parent.map(str::to_owned),
        workspace: ws,
        pane: None,
        agent: Some(agent.into()),
        title: format!("real: {agent}"),
        cwd: cwd.clone(),
        task: task.map(str::to_owned),
        rows: ROWS,
        cols: COLS,
    };
    let p1 = panes::start(
        &panes,
        &pm,
        Some(&url),
        env(),
        &spec("claude-code", None, None),
        Box::new(|_| {}),
    )
    .unwrap();
    assert!(wait(&panes, "claude idle", || status(&panes, "p1") == "idle"));

    // 1. Codex (child) asks Claude (parent) to do work; the request reaches Claude entirely as a paste.
    let task = format!(
        "Call the agentdeck MCP tool pane_send with to set to \"parent\" and this text: \
        Please create a file named {file} in your current folder containing only the number 391. \
        When the file exists, use the agentdeck tool pane_send to send the text DONE to \"p2\". \
        After pane_send returns, stop and wait; do nothing else."
    );
    let p2 = panes::start(
        &panes,
        &pm,
        Some(&url),
        env(),
        &spec("codex", Some("p1"), Some(&task)),
        Box::new(|_| {}),
    )
    .unwrap();
    let answer = dir.join(&file);
    let made = wait(&panes, "claude created the file", || answer.exists());
    let done = wait(&panes, "DONE from claude in codex", || {
        let s = screen(&panes, "p2");
        s.contains("message from p1") && s.contains("DONE")
    });
    eprintln!("{file} = {:?}", std::fs::read_to_string(&answer).ok());
    let _ = std::fs::remove_file(&answer);

    // 2. Maximum message (8192 bytes + marker) each way; the reply quotes the last line, so the tail arrived.
    let idle = |p: &str| status(&panes, p) == "idle";
    wait(&panes, "both idle", || idle("p1") && idle("p2"));
    let to_claude = max_message(
        "Last line: the code word is ZEBRA-4417. Reply with pane_send to \"p2\" with only the code word.",
    );
    eprintln!(
        "send to claude: {:?}",
        engine.send("p2", "parent", &to_claude)
    );
    let zebra = wait(&panes, "ZEBRA-4417 back in codex", || {
        screen(&panes, "p2").contains("ZEBRA-4417")
    });
    wait(&panes, "both idle", || idle("p1") && idle("p2"));
    let to_codex = max_message(
        "Last line: the code word is OKAPI-9020. Reply with pane_send to \"parent\" with only the code word.",
    );
    eprintln!("send to codex: {:?}", engine.send("p1", "p2", &to_codex));
    let okapi = wait(&panes, "OKAPI-9020 back in claude", || {
        screen(&panes, "p1").contains("OKAPI-9020")
    });

    for r in panes::lock(&panes).messages(ws) {
        eprintln!(
            "{} {}→{} {} {:?} {}B",
            r.id,
            r.from,
            r.to,
            r.state,
            r.reason,
            r.text.len()
        );
    }
    eprintln!(
        "--- p1 ---\n{}\n--- p2 ---\n{}",
        screen(&panes, "p1"),
        screen(&panes, "p2")
    );
    let _ = (p1, p2);
    pm.close_all();
    assert!(
        made && done && zebra && okapi,
        "made {made} done {done} zebra {zebra} okapi {okapi}"
    );
}
