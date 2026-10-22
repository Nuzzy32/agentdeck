//! Fake agent TUI for agentdeck tests (ARCHITECTURE §11, T2.5). Mimics what Phase 0 proved about the real CLIs:
//! alternate screen + bracketed paste, a prompt row the `screen.rs` input check understands, OSC 7501 status
//! (`claude`: query first, report only when answered; `codex`: report without query, `blocked` without `kind`),
//! and MCP calls with the pane token.
//!
//! Usage: `fake-agent --mode claude|codex [-- <first task>]`. A prompt is pasted or typed text ended by `\r`. Its last
//! non-empty line may be a command: `ASK` (blocked until `y`), `CALL <tool> [<json args>]`, `EXIT <code>`.
//! Every prompt prints `RECEIVED <n> bytes: <first line>`. The token is never printed.

use std::env;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
const QUERY: &[u8] = b"\x1b]7501;?\x1b\\";
/// How long `claude` mode waits for the query reply before staying silent for good.
const QUERY_WAIT: Duration = Duration::from_secs(2);
/// base64 of `approve Write: fake.txt`, the shape Claude Code sends (D-016).
const ASK_MSG: &str = "YXBwcm92ZSBXcml0ZTogZmFrZS50eHQ=";
/// Longest escape sequence kept; longer ones are dropped.
const MAX_ESC: usize = 4096;
const V_STATELESS: &str = "2026-07-28";
const V_SESSION: &str = "2025-06-18";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Claude,
    Codex,
}

impl Mode {
    fn app(self) -> &'static str {
        match self {
            Mode::Claude => "claude-code",
            Mode::Codex => "codex",
        }
    }
}

fn parse_args(args: &[String]) -> Result<(Mode, Option<String>), String> {
    let (opts, task) = match args.iter().position(|a| a == "--") {
        Some(i) => (&args[..i], Some(args[i + 1..].join(" "))),
        None => (args, None),
    };
    let mode = match opts {
        [flag, m] if flag == "--mode" && m == "claude" => Mode::Claude,
        [flag, m] if flag == "--mode" && m == "codex" => Mode::Codex,
        _ => return Err("usage: fake-agent --mode claude|codex [-- <first task>]".into()),
    };
    Ok((mode, task.filter(|t| !t.is_empty())))
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    /// A typed (not pasted) byte.
    Key(u8),
    /// `\r` outside a paste: the input line so far.
    Submit(Vec<u8>),
    /// OSC body between `ESC ]` and its terminator.
    Osc(Vec<u8>),
}

/// Keyboard input decoder: bracketed paste, typing, `\r`, backspace; CSI and other escapes are swallowed.
#[derive(Debug, Default)]
struct Input {
    line: Vec<u8>,
    esc: Option<Vec<u8>>,
    paste: bool,
}

impl Input {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut out = Vec::new();
        for &b in bytes {
            if self.paste {
                self.line.push(b);
                if self.line.ends_with(PASTE_END) {
                    self.line.truncate(self.line.len() - PASTE_END.len());
                    self.paste = false;
                }
            } else if let Some(seq) = self.esc.as_mut() {
                seq.push(b);
                let osc_end = seq[1] == b']' && (b == BEL || seq.ends_with(b"\x1b\\"));
                let done = match seq[1] {
                    b'[' => seq.len() > 2 && (0x40..=0x7e).contains(&b),
                    b']' => osc_end,
                    _ => true,
                };
                if done || seq.len() > MAX_ESC {
                    let seq = std::mem::take(seq);
                    self.esc = None;
                    if seq == PASTE_START {
                        self.paste = true;
                    } else if osc_end {
                        let end = seq.len() - if b == BEL { 1 } else { 2 };
                        out.push(Event::Osc(seq[2..end].to_vec()));
                    }
                }
            } else {
                match b {
                    ESC => self.esc = Some(vec![ESC]),
                    b'\r' => out.push(Event::Submit(std::mem::take(&mut self.line))),
                    0x7f => {
                        self.line.pop();
                    }
                    _ => {
                        self.line.push(b);
                        out.push(Event::Key(b));
                    }
                }
            }
        }
        out
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Cmd<'a> {
    Plain,
    Ask,
    Call(&'a str, &'a str),
    Exit(u8),
}

/// The command on the last non-empty line, so a delivered message (marker line first) can carry one too.
fn command(prompt: &str) -> Cmd<'_> {
    let line = prompt
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line == "ASK" {
        Cmd::Ask
    } else if let Some(rest) = line.strip_prefix("CALL ") {
        let rest = rest.trim();
        let (tool, args) = rest.split_once(' ').unwrap_or((rest, "{}"));
        Cmd::Call(tool, args.trim())
    } else if let Some(code) = line
        .strip_prefix("EXIT ")
        .and_then(|c| c.trim().parse().ok())
    {
        Cmd::Exit(code)
    } else {
        Cmd::Plain
    }
}

struct App {
    mode: Mode,
    rows: usize,
    cols: usize,
    log: Vec<String>,
    input: Input,
    /// `claude`: only after the query was answered.
    reporting: bool,
    blocked: bool,
}

impl App {
    fn write(&self, bytes: &[u8]) {
        let mut out = io::stdout().lock();
        // A closed PTY ends the session through stdin EOF; nothing useful to do with a write error here.
        let _ = out.write_all(bytes).and_then(|()| out.flush());
    }

    /// Long lines are wrapped here so a redraw never scrolls.
    fn log(&mut self, line: &str) {
        let chars: Vec<char> = line.chars().collect();
        for chunk in chars.chunks(self.cols.max(1)) {
            self.log.push(chunk.iter().collect());
        }
        if chars.is_empty() {
            self.log.push(String::new());
        }
    }

    /// Log, then the prompt row (`❯` / `›`) with the cursor parked on it and the row below closing the box: Claude's
    /// `─` border, Codex's blank row. That is what `Screen::input_state` checks.
    fn render(&self) {
        let keep = self.rows.saturating_sub(3);
        let start = self.log.len().saturating_sub(keep);
        let mut s = String::from("\x1b[H\x1b[2J");
        for line in &self.log[start..] {
            s.push_str(line);
            s.push_str("\r\n");
        }
        let typed: String = String::from_utf8_lossy(&self.input.line)
            .chars()
            .filter(|c| !c.is_control())
            .take(self.cols.saturating_sub(3))
            .collect();
        let (glyph, below) = match self.mode {
            Mode::Claude => ("❯", "─".repeat(self.cols)),
            Mode::Codex => ("›", String::new()),
        };
        s.push_str(&format!("{glyph} {typed}\r\n{below}\x1b[A\r"));
        let col = 2 + typed.chars().count();
        s.push_str(&format!("\x1b[{col}C"));
        self.write(s.as_bytes());
    }

    fn report(&self, state: &str, extra: &str) {
        if !self.reporting {
            return;
        }
        self.render();
        let end = match self.mode {
            Mode::Claude => "\x1b\\",
            Mode::Codex => "\x07",
        };
        let app = self.mode.app();
        self.write(format!("\x1b]7501;state={state}:app={app}{extra}{end}").as_bytes());
    }

    fn finish(&self) {
        self.report(
            match self.mode {
                Mode::Claude => "done",
                Mode::Codex => "idle",
            },
            "",
        );
    }

    fn approve(&mut self) {
        self.blocked = false;
        self.input.line.clear();
        self.log("APPROVED");
        self.finish();
    }

    /// `Some(code)` to exit.
    fn prompt(&mut self, bytes: &[u8]) -> Option<u8> {
        if bytes.is_empty() {
            return None;
        }
        if self.blocked {
            self.log("BLOCKED: press y");
            return None;
        }
        let text = String::from_utf8_lossy(bytes).into_owned();
        let first = text.lines().next().unwrap_or("");
        self.log(&format!("RECEIVED {} bytes: {first}", bytes.len()));
        self.report("working", "");
        match command(&text) {
            Cmd::Plain => {}
            Cmd::Ask => {
                self.blocked = true;
                self.log("ASK: allow? press y");
                let extra = match self.mode {
                    Mode::Claude => format!(":kind=permission:msg={ASK_MSG}"),
                    Mode::Codex => String::new(),
                };
                self.report("blocked", &extra);
                return None;
            }
            Cmd::Call(tool, args) => {
                let line = match mcp_call(self.mode, tool, args) {
                    Ok(reply) => format!("MCP {tool} -> {reply}"),
                    Err(e) => format!("MCP ERROR {e}"),
                };
                self.log(&line);
            }
            Cmd::Exit(code) => return Some(code),
        }
        self.finish();
        None
    }
}

fn run(mode: Mode, task: Option<String>, rows: usize, cols: usize) -> u8 {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = stdin.read(&mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    let mut app = App {
        mode,
        rows,
        cols,
        log: vec![format!("fake-agent ({})", mode.app())],
        input: Input::default(),
        reporting: mode == Mode::Codex,
        blocked: false,
    };
    app.write(b"\x1b[?1049h\x1b[?2004h");
    app.render();
    let mut deadline = None;
    match mode {
        Mode::Claude => {
            app.write(QUERY);
            deadline = Some(Instant::now() + QUERY_WAIT);
        }
        Mode::Codex => app.report("idle", ""),
    }
    if let Some(task) = task
        && let Some(code) = app.prompt(task.as_bytes())
    {
        return code;
    }
    app.render();

    loop {
        let chunk = match deadline {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let chunk = match chunk {
            Ok(c) => c,
            Err(RecvTimeoutError::Timeout) => {
                deadline = None;
                app.log("no OSC 7501 reply; status reports off");
                app.render();
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return 0,
        };
        for event in app.input.feed(&chunk) {
            match event {
                Event::Osc(body) if deadline.is_some() && body.starts_with(b"7501;?:") => {
                    deadline = None;
                    app.reporting = true;
                    app.report("idle", "");
                }
                Event::Osc(_) | Event::Key(_) if !app.blocked => {}
                Event::Key(b'y') => app.approve(),
                Event::Osc(_) | Event::Key(_) => {}
                Event::Submit(p) => {
                    if let Some(code) = app.prompt(&p) {
                        return code;
                    }
                }
            }
        }
        app.render();
    }
}

fn stty(args: &[&str]) -> Option<String> {
    let out = Command::new("stty")
        .args(args)
        .stdin(Stdio::inherit())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let (mode, task) = match parse_args(&args) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let (rows, cols) = stty(&["size"])
        .and_then(|s| {
            let (r, c) = s.split_once(' ')?;
            Some((r.parse().ok()?, c.parse().ok()?))
        })
        .filter(|&(r, c)| r > 0 && c > 0)
        .unwrap_or((24, 80));
    let saved = stty(&["-g"]);
    stty(&["raw", "-echo"]);
    let code = run(mode, task, rows, cols);
    let mut out = io::stdout().lock();
    let _ = out
        .write_all(b"\x1b[?2004l\x1b[?1049l")
        .and_then(|()| out.flush());
    if let Some(saved) = saved {
        stty(&[&saved]);
    }
    ExitCode::from(code)
}

// ---- MCP over plain HTTP/1.0 (one request per connection; no chunked bodies to decode) ----

struct Reply {
    status: u16,
    session: Option<String>,
    body: String,
}

/// `http://host:port/path` → (`host:port`, `/path`). Only plain HTTP: the server is on loopback.
fn endpoint(url: &str) -> Result<(&str, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or("AGENTDECK_MCP_URL must start with http://")?;
    Ok(match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_owned()),
    })
}

fn parse_reply(raw: &str) -> Result<Reply, String> {
    let (head, body) = raw.split_once("\r\n\r\n").ok_or("malformed HTTP reply")?;
    let status = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .ok_or("malformed HTTP status")?;
    let session = head.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case("mcp-session-id")
            .then(|| v.trim().to_owned())
    });
    Ok(Reply {
        status,
        session,
        body: body.to_owned(),
    })
}

/// The JSON-RPC message from a JSON or SSE body.
fn rpc(body: &str) -> &str {
    body.lines()
        .filter_map(|l| l.strip_prefix("data:"))
        .map(str::trim)
        .find(|d| !d.is_empty())
        .unwrap_or(body.trim())
}

/// Errors never contain the token: it only goes into the request bytes.
fn post(
    addr: &str,
    path: &str,
    token: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Result<Reply, String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| format!("connect {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let mut req = format!(
        "POST {path} HTTP/1.0\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\n\
         Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
         Content-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    stream
        .write_all(req.as_bytes())
        .map_err(|e| format!("send: {e}"))?;
    let mut raw = String::new();
    stream
        .read_to_string(&mut raw)
        .map_err(|e| format!("read: {e}"))?;
    let reply = parse_reply(&raw)?;
    if !(200..300).contains(&reply.status) {
        return Err(format!("HTTP {}", reply.status));
    }
    Ok(reply)
}

/// `claude`: stateless `2026-07-28` (`server/discover`, then `tools/call` with `_meta`), as Claude Code negotiates.
/// `codex`: `2025-06-18` handshake and `Mcp-Session-Id`, as Codex 0.163.0 does (D-017). Returns the raw JSON-RPC reply.
fn mcp_call(mode: Mode, tool: &str, args: &str) -> Result<String, String> {
    if tool.is_empty()
        || !tool
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("tool name must be [A-Za-z0-9_-]+".into());
    }
    let url = env::var("AGENTDECK_MCP_URL").map_err(|_| "AGENTDECK_MCP_URL not set")?;
    let token = env::var("AGENTDECK_TOKEN").map_err(|_| "AGENTDECK_TOKEN not set")?;
    let (addr, path) = endpoint(&url)?;
    let send = |headers: &[(&str, &str)], body: String| post(addr, &path, &token, headers, &body);
    match mode {
        Mode::Claude => {
            let meta = format!(
                r#"{{"io.modelcontextprotocol/protocolVersion":"{V_STATELESS}","io.modelcontextprotocol/clientInfo":{{"name":"fake-agent","version":"0"}},"io.modelcontextprotocol/clientCapabilities":{{}}}}"#
            );
            send(
                &[
                    ("MCP-Protocol-Version", V_STATELESS),
                    ("Mcp-Method", "server/discover"),
                ],
                format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{{"_meta":{meta}}}}}"#
                ),
            )?;
            let reply = send(
                &[
                    ("MCP-Protocol-Version", V_STATELESS),
                    ("Mcp-Method", "tools/call"),
                    ("Mcp-Name", tool),
                ],
                format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{args},"_meta":{meta}}}}}"#
                ),
            )?;
            Ok(rpc(&reply.body).to_owned())
        }
        Mode::Codex => {
            let reply = send(
                &[],
                format!(
                    r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{V_SESSION}","capabilities":{{}},"clientInfo":{{"name":"fake-agent","version":"0"}}}}}}"#
                ),
            )?;
            let session = reply
                .session
                .ok_or("no Mcp-Session-Id in initialize reply")?;
            let h = [
                ("Mcp-Session-Id", session.as_str()),
                ("MCP-Protocol-Version", V_SESSION),
            ];
            send(
                &h,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.into(),
            )?;
            let reply = send(
                &h,
                format!(
                    r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{args}}}}}"#
                ),
            )?;
            Ok(rpc(&reply.body).to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn args_parse() {
        assert_eq!(
            parse_args(&args(&["--mode", "claude"])),
            Ok((Mode::Claude, None))
        );
        assert_eq!(
            parse_args(&args(&["--mode", "codex", "--", "-x", "task"])),
            Ok((Mode::Codex, Some("-x task".into())))
        );
        assert!(parse_args(&args(&["--mode", "gpt"])).is_err());
        assert!(parse_args(&args(&[])).is_err());
    }

    #[test]
    fn paste_split_across_chunks_is_one_prompt() {
        let mut i = Input::default();
        let whole = b"\x1b[200~[agentdeck] marker\nline 2\r\nline 3\x1b[201~\r";
        let mut events = Vec::new();
        for b in whole {
            events.extend(i.feed(&[*b]));
        }
        assert_eq!(
            events,
            vec![Event::Submit(
                b"[agentdeck] marker\nline 2\r\nline 3".to_vec()
            )]
        );
    }

    #[test]
    fn typing_backspace_and_csi() {
        let mut i = Input::default();
        let ev = i.feed(b"ab\x7fc\x1b[A\x1bOx\r");
        assert_eq!(
            ev,
            vec![
                Event::Key(b'a'),
                Event::Key(b'b'),
                Event::Key(b'c'),
                Event::Key(b'x'),
                Event::Submit(b"acx".to_vec())
            ]
        );
    }

    #[test]
    fn osc_with_st_and_bel() {
        let mut i = Input::default();
        let mut ev = i.feed(b"\x1b]7501;?:states=idle\x1b");
        ev.extend(i.feed(b"\\\x1b]11;rgb:0/0/0\x07"));
        assert_eq!(
            ev,
            vec![
                Event::Osc(b"7501;?:states=idle".to_vec()),
                Event::Osc(b"11;rgb:0/0/0".to_vec())
            ]
        );
        // An oversized escape is dropped (bytes after the cap are plain input) and input recovers.
        i.feed(&[b"\x1b]".as_slice(), &[b'a'; MAX_ESC - 2]].concat());
        assert!(i.esc.as_ref().is_some_and(|e| e.len() == MAX_ESC));
        assert_eq!(i.feed(b"a"), vec![]);
        assert_eq!(i.esc, None);
        assert_eq!(
            i.feed(b"z\r"),
            vec![Event::Key(b'z'), Event::Submit(b"z".to_vec())]
        );
    }

    #[test]
    fn commands() {
        assert_eq!(command("ASK"), Cmd::Ask);
        assert_eq!(
            command("[agentdeck] message from p2\n  ASK  \n\n"),
            Cmd::Ask
        );
        assert_eq!(command("CALL whoami {}"), Cmd::Call("whoami", "{}"));
        assert_eq!(command("CALL whoami"), Cmd::Call("whoami", "{}"));
        assert_eq!(
            command("CALL pane_send {\"to\": \"p1\"}"),
            Cmd::Call("pane_send", "{\"to\": \"p1\"}")
        );
        assert_eq!(command("EXIT 3"), Cmd::Exit(3));
        assert_eq!(command("EXIT nope"), Cmd::Plain);
        assert_eq!(command("ASK\nhello"), Cmd::Plain);
    }

    #[test]
    fn http_bits() {
        assert_eq!(
            endpoint("http://127.0.0.1:4000/mcp"),
            Ok(("127.0.0.1:4000", "/mcp".to_owned()))
        );
        assert!(endpoint("https://x/mcp").is_err());
        let r = parse_reply(
            "HTTP/1.1 200 OK\r\nmcp-session-id: abc\r\ncontent-type: text/event-stream\r\n\r\ndata:\n\ndata: {\"id\":1}\n\n",
        )
        .unwrap();
        assert_eq!((r.status, r.session.as_deref()), (200, Some("abc")));
        assert_eq!(rpc(&r.body), "{\"id\":1}");
        assert_eq!(rpc("{\"id\":2}\n"), "{\"id\":2}");
        assert!(parse_reply("garbage").is_err());
    }

    #[test]
    fn bad_tool_name_rejected_before_any_io() {
        assert!(mcp_call(Mode::Codex, "a\r\nX-Evil: 1", "{}").is_err());
        assert!(mcp_call(Mode::Codex, "", "{}").is_err());
    }
}
