//! T2.5: `tests/fake-agent` in a real PTY, driven through `status.rs`, `screen.rs` and `delivery.rs` the way the pane
//! reader will be, plus `CALL whoami` against `mcp::server::start` under both protocol versions. T1.7: the same agent
//! in a real `PaneManager` pane, whose reader answers the query and reports status and exit code. T2.3: the agent
//! launched through `panes::start` (catalogue argv, MCP injection, registry) answers `whoami` with its own pane id.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use agentdeck_lib::delivery::{self, Action, Delivery, PaneStatus, Poll, Sender, Target};
use agentdeck_lib::launch::{self, Launch};
use agentdeck_lib::mcp::server;
use agentdeck_lib::panes::{self, Panes, Spec};
use agentdeck_lib::policy::LoopGuard;
use agentdeck_lib::pty::{PaneEvent, PaneManager};
use agentdeck_lib::screen::{self, Screen};
use agentdeck_lib::status::{AgentStatus, Event, Kind, QUERY_REPLY, Scanner, State, Tracker};
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};

const ROWS: u16 = 30;
const COLS: u16 = 200;
const WAIT: Duration = Duration::from_secs(10);

/// Builds the fake agent once per test binary.
fn binary() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fake-agent");
        let target = dir.join("target");
        let status = Command::new(env!("CARGO"))
            .args(["build", "--quiet", "--manifest-path"])
            .arg(dir.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(&target)
            .status()
            .expect("run cargo build for fake-agent");
        assert!(status.success(), "fake-agent build failed");
        target.join("debug/fake-agent")
    })
}

/// One pane: PTY + the per-pane state the core keeps (screen, OSC scanner, status tracker).
struct Agent {
    agent_id: &'static str,
    writer: Box<dyn Write + Send>,
    rx: Receiver<Vec<u8>>,
    child: Box<dyn Child + Send + Sync>,
    screen: Screen,
    scanner: Scanner,
    tracker: Tracker,
    /// Answer `OSC 7501;?` like agentdeck does.
    answer: bool,
    queries: usize,
    states: Vec<State>,
    raw: Vec<u8>,
}

impl Agent {
    fn spawn(mode: &str, env: &[(&str, &str)], task: Option<&str>, answer: bool) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut cmd = CommandBuilder::new(binary());
        cmd.args(["--mode", mode]);
        if let Some(t) = task {
            cmd.args(["--", t]);
        }
        cmd.env("TERM", "xterm-256color");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let _master = pair.master; // keep the PTY open while reading
            let mut buf = [0u8; 8192];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        Self {
            agent_id: if mode == "claude" {
                "claude-code"
            } else {
                "codex"
            },
            writer,
            rx,
            child,
            screen: Screen::new(ROWS, COLS),
            scanner: Scanner::default(),
            tracker: Tracker::default(),
            answer,
            queries: 0,
            states: Vec::new(),
            raw: Vec::new(),
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    /// Reads output for up to `timeout`, feeding it where the pane reader will.
    fn pump(&mut self, timeout: Duration) {
        let Ok(chunk) = self.rx.recv_timeout(timeout) else {
            return;
        };
        self.raw.extend_from_slice(&chunk);
        self.screen.process(&chunk);
        for event in self.scanner.feed(&chunk) {
            match event {
                Event::Query => {
                    self.queries += 1;
                    if self.answer {
                        self.send(QUERY_REPLY);
                    }
                }
                Event::Report(r) => {
                    self.states.push(r.state);
                    self.tracker.apply(r);
                }
            }
        }
    }

    fn wait(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let end = Instant::now() + WAIT;
        while !done(self) {
            assert!(
                Instant::now() < end,
                "timed out waiting for {what}; screen:\n{}",
                self.screen.read(ROWS as usize)
            );
            self.pump(Duration::from_millis(50));
        }
    }

    fn wait_text(&mut self, text: &str) {
        self.wait(text, |a| a.screen.read(ROWS as usize).contains(text));
    }

    fn wait_state(&mut self, state: State) {
        self.wait(&format!("{state:?}"), |a| {
            a.tracker.current().map(|r| r.state) == Some(state)
        });
    }

    /// Let output settle so the latest redraw is on the screen.
    fn settle(&mut self) {
        let end = Instant::now() + Duration::from_millis(300);
        while Instant::now() < end {
            self.pump(Duration::from_millis(20));
        }
    }

    /// The snapshot T2.7 will hand to `Delivery::poll`.
    fn target(&self) -> Target {
        let status = match self.tracker.mapped().map(|m| m.status) {
            Some(AgentStatus::Idle) => PaneStatus::Idle,
            None => PaneStatus::Unknown,
            Some(_) => PaneStatus::Busy,
        };
        Target {
            alive: true,
            status,
            last_output: self.screen.last_output().unwrap_or_else(Instant::now),
            last_typed: None,
            input: match self.screen.input_state(self.agent_id) {
                screen::InputState::Empty => delivery::InputState::Empty,
                screen::InputState::HasText => delivery::InputState::HasText,
                screen::InputState::Unknown => delivery::InputState::Unknown,
            },
            bracketed_paste: self.screen.bracketed_paste(),
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
fn claude_reports_after_query_and_takes_delivered_paste_as_one_prompt() {
    let mut a = Agent::spawn("claude", &[], None, true);
    a.wait_state(State::Idle);
    assert_eq!(a.queries, 1);
    let r = a.tracker.current().unwrap();
    assert_eq!(r.app.as_deref(), Some("claude-code"));
    a.settle();
    assert!(a.screen.bracketed_paste() && a.screen.alternate_screen());
    assert_eq!(
        a.screen.input_state("claude-code"),
        screen::InputState::Empty
    );

    // Typed text shows in the input box, so delivery would wait.
    a.send(b"draft");
    a.wait("typed text", |a| {
        a.screen.input_state("claude-code") == screen::InputState::HasText
    });
    a.send(b"\x7f\x7f\x7f\x7f\x7f");
    a.wait("empty input", |a| {
        a.screen.input_state("claude-code") == screen::InputState::Empty
    });

    let from = Sender {
        pane: "p2",
        agent: "codex",
        title: Some("review: auth"),
    };
    let body = "line one\nline two\nCALL nothing-here\nlast line";
    let mut d = Delivery::default();
    let mut guard = LoopGuard::default();
    d.enqueue(&from, "p1", body, &guard).unwrap();
    let actions = match d.poll("p1", &a.target(), &mut guard, 12, Instant::now()) {
        Poll::Deliver { actions, .. } => actions,
        other => panic!("not delivered: {other:?}"),
    };
    a.states.clear();
    for action in actions {
        match action {
            Action::Write(bytes) => a.send(&bytes),
            Action::Sleep(t) => thread::sleep(t),
        }
    }
    let marker = delivery::marker(&from);
    let n = marker.len() + 1 + body.len();
    a.wait_text(&format!("RECEIVED {n} bytes: {marker}"));
    a.wait_state(State::Done);
    assert_eq!(a.states, vec![State::Working, State::Done]);
    a.settle();
    assert_eq!(count(&a.screen.read(ROWS as usize), "RECEIVED"), 1);
    assert_eq!(
        a.screen.input_state("claude-code"),
        screen::InputState::Empty
    );

    a.send(b"EXIT 3\r");
    let end = Instant::now() + WAIT;
    let status = loop {
        if let Some(s) = a.child.try_wait().unwrap() {
            break s;
        }
        assert!(Instant::now() < end, "fake-agent did not exit");
        a.pump(Duration::from_millis(50));
    };
    assert_eq!(status.exit_code(), 3);
}

#[test]
fn claude_stays_silent_without_query_reply() {
    let mut a = Agent::spawn("claude", &[], None, false);
    a.wait_text("no OSC 7501 reply; status reports off");
    a.send(b"hello\r");
    a.wait_text("RECEIVED 5 bytes: hello");
    a.settle();
    assert_eq!(a.queries, 1);
    assert!(a.states.is_empty());
    assert_eq!(a.tracker.mapped(), None);
}

#[test]
fn codex_reports_without_query_and_prints_first_task() {
    let mut a = Agent::spawn("codex", &[], Some("-first task"), true);
    a.wait_text("RECEIVED 11 bytes: -first task");
    a.wait_state(State::Idle);
    a.settle();
    assert_eq!(a.queries, 0);
    assert_eq!(a.states, vec![State::Idle, State::Working, State::Idle]);
    assert_eq!(a.tracker.current().unwrap().app.as_deref(), Some("codex"));
    assert_eq!(a.screen.input_state("codex"), screen::InputState::Empty);
    assert!(a.screen.bracketed_paste());
}

#[test]
fn ask_blocks_until_y_in_both_modes() {
    for (mode, kind, msg, label, finished) in [
        (
            "claude",
            Some(Kind::Permission),
            Some("approve Write: fake.txt"),
            "butuh izin",
            State::Done,
        ),
        ("codex", None, None, "butuh kamu", State::Idle),
    ] {
        let mut a = Agent::spawn(mode, &[], None, true);
        a.wait_state(State::Idle);
        a.send(b"ASK\r");
        a.wait_state(State::Blocked);
        let r = a.tracker.current().unwrap().clone();
        assert_eq!((r.kind, r.msg.as_deref()), (kind, msg), "{mode}");
        let m = r.mapped().unwrap();
        assert_eq!((m.status, m.label), (AgentStatus::NeedsYou, Some(label)));
        // Blocked: a new prompt is refused, only `y` answers.
        a.send(b"more\r");
        a.wait_text("BLOCKED: press y");
        a.send(b"y");
        a.wait_text("APPROVED");
        a.wait_state(finished);
    }
}

#[test]
fn call_whoami_under_both_protocols() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let panes = Panes::default();
    let (t1, t2) = {
        let mut reg = panes::lock(&panes);
        let ws = reg.create_workspace();
        let mut token = |agent: &str| {
            let spec = spec(ws, agent, None);
            reg.open(&spec).unwrap().token.unwrap().expose().to_owned()
        };
        (token("claude-code"), token("codex"))
    };
    let port = rt.block_on(server::start(panes)).unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");

    for (mode, token, pane) in [("claude", &t1, "p1"), ("codex", &t2, "p2")] {
        let env = [
            ("AGENTDECK_MCP_URL", url.as_str()),
            ("AGENTDECK_TOKEN", token.as_str()),
        ];
        let mut a = Agent::spawn(mode, &env, None, true);
        a.wait_state(State::Idle);
        a.send(b"CALL whoami {}\r");
        // Each prompt ends in a redraw; wait on the text itself rather than catch one mid-redraw.
        let want = format!(r#""text":"{pane}""#);
        a.wait(&format!("{mode} whoami -> {pane}"), |a| {
            let text = a.screen.read(ROWS as usize);
            text.contains(&want) || text.contains("MCP ERROR")
        });
        let text = a.screen.read(ROWS as usize);
        assert!(text.contains(&want), "{mode}:\n{text}");
        assert!(!String::from_utf8_lossy(&a.raw).contains(token.as_str()));
    }

    // A wrong token is refused with 401 and the token is not echoed.
    let env = [
        ("AGENTDECK_MCP_URL", url.as_str()),
        ("AGENTDECK_TOKEN", "not-a-token"),
    ];
    let mut a = Agent::spawn("codex", &env, None, true);
    a.wait_state(State::Idle);
    a.send(b"CALL whoami\r");
    a.wait_text("MCP ERROR HTTP 401");
    assert!(!a.screen.read(ROWS as usize).contains("not-a-token"));
}

/// Status events from a real `PaneManager` pane (T1.7), until one with `status` arrives.
fn wait_status(rx: &Receiver<PaneEvent>, status: &str) -> PaneEvent {
    let end = Instant::now() + WAIT;
    loop {
        let left = end.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(ev @ PaneEvent::Status { status: s, .. }) if s == status => return ev,
            Ok(PaneEvent::Exited { code }) => {
                panic!("exited ({code:?}) while waiting for {status}")
            }
            Ok(_) => {}
            Err(_) => panic!("timed out waiting for status {status}"),
        }
    }
}

/// T1.7: the pane reader itself answers `OSC 7501;?`, status changes reach the callback the WebView channel is built
/// on, and the exit code is reported last.
#[test]
fn pane_manager_reports_status_and_exit_code_in_both_modes() {
    for (mode, kind, msg) in [
        (
            "claude",
            Some("permission"),
            Some("approve Write: fake.txt"),
        ),
        ("codex", None, None),
    ] {
        let pm = PaneManager::default();
        let (tx, rx) = mpsc::channel();
        let launch = Launch {
            argv: vec![
                binary().to_str().unwrap().into(),
                "--mode".into(),
                mode.into(),
            ],
            env: launch::env(|k| std::env::var_os(k), "/usr/bin:/bin"),
            osc_status: true,
        };
        let on_event = Box::new(move |ev| {
            let _ = tx.send(ev);
        });
        let id = pm
            .spawn(&launch, Path::new("/"), (ROWS, COLS), None, on_event)
            .unwrap();
        // Claude mode only reports once the query is answered, so this also proves the reply.
        wait_status(&rx, "idle");
        pm.write(id, b"ASK\r", true).unwrap();
        let blocked = wait_status(&rx, "needs_you");
        assert_eq!(
            blocked,
            PaneEvent::Status {
                status: "needs_you",
                kind,
                msg: msg.map(str::to_owned),
                error: false
            },
            "{mode}"
        );
        pm.write(id, b"y", true).unwrap();
        wait_status(&rx, "idle");
        pm.write(id, b"EXIT 3\r", true).unwrap();
        let end = Instant::now() + WAIT;
        let exited = loop {
            match rx.recv_timeout(end.saturating_duration_since(Instant::now())) {
                Ok(PaneEvent::Exited { code }) => break code,
                Ok(_) => {}
                Err(_) => panic!("{mode}: no exit event"),
            }
        };
        assert_eq!(exited, Some(3), "{mode}");
        pm.close_all();
    }
}

fn spec(workspace: u32, agent: &str, task: Option<&str>) -> Spec {
    Spec {
        workspace,
        pane: None,
        agent: Some(agent.into()),
        title: format!("t: {agent}"),
        cwd: "/".into(),
        task: task.map(str::to_owned),
        rows: ROWS,
        cols: COLS,
    }
}

/// `claude` and `codex` on a PATH of their own: each records its argv and `AGENTDECK_*` env, drops the catalogue
/// flags before `--` (fake-agent only takes `--mode`), and execs fake-agent in that mode.
fn fake_clis() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("agentdeck-t23-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, mode) in [("claude", "claude"), ("codex", "codex")] {
        let script = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > \"$0.argv\"\n\
             printf '%s' \"$AGENTDECK_TOKEN\" > \"$0.token\"\n\
             printf '%s %s' \"$AGENTDECK_PANE_ID\" \"$AGENTDECK_MCP_URL\" > \"$0.env\"\n\
             while [ $# -gt 0 ] && [ \"$1\" != -- ]; do shift; done\n\
             exec '{}' --mode {mode} \"$@\"\n",
            binary().display()
        );
        let p = dir.join(name);
        std::fs::write(&p, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dir
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn wait_screen(panes: &Panes, pane: &str, what: &str, done: impl Fn(&str) -> bool) -> String {
    let screen = panes::lock(panes).screen(pane).unwrap();
    let end = Instant::now() + WAIT;
    loop {
        let text = screen.lock().unwrap().read(ROWS as usize);
        if done(&text) {
            return text;
        }
        assert!(
            Instant::now() < end,
            "timed out waiting for {what}:\n{text}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

/// T2.3 end to end: a 16 KB first task starting with `-` arrives as one prompt, the catalogue's MCP argv and env
/// reach the process with the token in env only, `CALL whoami {}` answers the registry's pane id, and the token is
/// refused (401) once the pane exits or is closed.
#[test]
fn launch_path_injects_mcp_and_whoami_answers_pane_id() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let panes = Panes::default();
    let port = rt.block_on(server::start(panes.clone())).unwrap();
    let url = format!("http://127.0.0.1:{port}/mcp");
    let bin = fake_clis();
    let path = format!("{}:/usr/bin:/bin", bin.display());
    let pm = PaneManager::default();
    let ws = panes::lock(&panes).create_workspace();
    // A shell first, so agent ids are not simply p1/p2 by accident.
    let shell = Spec {
        agent: None,
        ..spec(ws, "", None)
    };
    let env = || launch::env(|k| std::env::var_os(k), &path);
    panes::start(&panes, &pm, Some(&url), env(), &shell, Box::new(|_| {})).unwrap();

    let task = format!("-{}\nCALL whoami {{}}", "x".repeat(16 * 1024 - 16));
    assert_eq!(task.len(), 16 * 1024);
    let mut started = Vec::new();
    for (agent, bin_name, flag) in [
        ("claude-code", "claude", "--mcp-config"),
        ("codex", "codex", "-c"),
    ] {
        let (tx, rx) = mpsc::channel();
        let on_event = Box::new(move |ev| {
            let _ = tx.send(ev);
        });
        let s = panes::start(
            &panes,
            &pm,
            Some(&url),
            env(),
            &spec(ws, agent, Some(&task)),
            on_event,
        )
        .unwrap();
        let want = format!(r#""text":"{}""#, s.pane);
        let text = wait_screen(&panes, &s.pane, &want, |t| {
            t.contains(&want) || t.contains("MCP ERROR")
        });
        assert!(text.contains(&want), "{agent}:\n{text}");
        // The task's last line ran as a command: the 16 KB task reached the agent as one prompt.

        let argv = read(bin.join(format!("{bin_name}.argv")));
        let token = read(bin.join(format!("{bin_name}.token")));
        assert_eq!(token.len(), 64, "{agent}");
        assert_eq!(
            read(bin.join(format!("{bin_name}.env"))),
            format!("{} {url}", s.pane)
        );
        assert!(argv.starts_with(flag), "{agent}: {argv}");
        assert!(
            argv.ends_with(&format!("\n--\n{task}\n")),
            "{agent}: task not one arg after --"
        );
        assert!(!argv.contains(&token), "{agent}: token in argv");
        started.push((s, token, rx));
    }

    let status = |token: &str| {
        let req = reqwest::Client::new()
            .post(&url)
            .bearer_auth(token)
            .header("Accept", "application/json, text/event-stream")
            .json(&serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "0"}}}));
        rt.block_on(async { req.send().await.unwrap().status() })
    };
    let (claude, codex) = (&started[0], &started[1]);
    assert_eq!(status(&claude.1), reqwest::StatusCode::OK);
    assert_eq!(status(&codex.1), reqwest::StatusCode::OK);

    // Codex pane: the process exits on its own.
    pm.write(codex.0.pty, b"EXIT 0\r", true).unwrap();
    let end = Instant::now() + WAIT;
    loop {
        match codex
            .2
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            Ok(PaneEvent::Exited { .. }) => break,
            Ok(_) => {}
            Err(_) => panic!("codex fake did not exit"),
        }
    }
    assert_eq!(status(&codex.1), reqwest::StatusCode::UNAUTHORIZED);
    let list = panes::lock(&panes).list(&claude.0.pane).unwrap();
    let ids: Vec<(&str, &str, &str)> = list
        .panes
        .iter()
        .map(|p| (p.id.as_str(), p.relation, p.status))
        .collect();
    assert_eq!(ids[0].1, "shell");
    assert_eq!(ids[1].1, "you");
    assert_eq!((ids[2].1, ids[2].2), ("peer", "exited"));

    // Claude pane: the person closes it.
    assert_eq!(
        panes::lock(&panes).close(&claude.0.pane),
        Some(claude.0.pty)
    );
    assert_eq!(status(&claude.1), reqwest::StatusCode::UNAUTHORIZED);
    pm.close_all();
    let _ = std::fs::remove_dir_all(&bin);
}
