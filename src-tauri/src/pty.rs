//! Pane PTYs: spawn, input, resize, flow-controlled output (D-018), OSC 7501 status (ARCHITECTURE §6), exit code,
//! process-group shutdown (ARCHITECTURE §3). Pane identity, tokens and the `pane_create`/`pane_close` commands live
//! in `panes.rs`; here a pane is a numeric PTY id.
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use serde::Serialize;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::launch::Launch;
use crate::screen::Screen;
use crate::status::{self, AgentStatus, Kind, QUERY_REPLY, Report, Scanner, Tracker};

/// Max bytes sent to the WebView that xterm.js has not confirmed yet (D-018).
pub const WINDOW: u64 = 128 << 10;
/// Latest output kept for replay when a WebView (re)attaches; the oldest bytes are dropped.
pub const REPLAY: usize = 1 << 20;
/// Time between SIGTERM and SIGKILL on close.
const KILL_GRACE: Duration = Duration::from_secs(3);
/// Upper bound for `close_all`: KILL_GRACE, the 1 s wait after SIGKILL, and slack.
const EXIT_LIMIT: Duration = Duration::from_secs(5);

type Sink = Channel<InvokeResponseBody>;
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// What the core tells the WebView about a pane besides its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PaneEvent {
    /// Official status from OSC 7501 (ARCHITECTURE §6). `unknown`: the agent cleared it, the UI falls back to the
    /// output heuristic. `kind` only matters for `needs_you`; `msg` is decoded agent text, for a tooltip only.
    Status {
        status: &'static str,
        kind: Option<&'static str>,
        msg: Option<String>,
        error: bool,
    },
    /// The process exited; `None` when its status could not be read.
    Exited { code: Option<u32> },
}

pub type OnEvent = Box<dyn Fn(PaneEvent) + Send>;

fn status_event(report: Option<&Report>) -> PaneEvent {
    let mapped = report.and_then(Report::mapped);
    PaneEvent::Status {
        status: match mapped.map(|m| m.status) {
            Some(AgentStatus::Idle) => "idle",
            Some(AgentStatus::Working) => "working",
            Some(AgentStatus::NeedsYou) => "needs_you",
            None => "unknown",
        },
        kind: report.and_then(|r| r.kind).map(|k| match k {
            Kind::Permission => "permission",
            Kind::Question => "question",
            Kind::Auth => "auth",
        }),
        msg: report.and_then(|r| r.msg.clone()),
        error: mapped.is_some_and(|m| m.error),
    }
}

/// OSC 7501 per pane: answers queries (D-016) and reports each change of the root record.
struct StatusScan {
    scanner: Scanner,
    tracker: Tracker,
}

impl StatusScan {
    fn feed(&mut self, chunk: &[u8], writer: &Writer, on_event: &OnEvent) {
        for ev in self.scanner.feed(chunk) {
            match ev {
                status::Event::Query => {
                    // ponytail: blocks while a pane_write holds the writer on a full PTY input buffer; only an agent
                    // that stops reading stdin while querying could stall here. Queue replies if that ever shows up.
                    let mut w = lock(writer);
                    let _ = w.write_all(QUERY_REPLY).and_then(|_| w.flush());
                }
                status::Event::Report(r) => {
                    let before = self.tracker.current().cloned();
                    self.tracker.apply(r);
                    if self.tracker.current() != before.as_ref() {
                        on_event(status_event(self.tracker.current()));
                    }
                }
            }
        }
    }
}

#[derive(Default)]
struct OutState {
    replay: VecDeque<u8>,
    sink: Option<Sink>,
    /// Bytes sent to / confirmed by the current sink. Reset on attach, so totals are per attachment.
    sent: u64,
    acked: u64,
    closed: bool,
}

/// Output side of a pane: replay ring plus credit window. Independent of the PTY so it can be unit tested.
#[derive(Default)]
struct Output {
    st: Mutex<OutState>,
    cv: Condvar,
}

impl Output {
    fn lock(&self) -> MutexGuard<'_, OutState> {
        // A poisoned lock only means another thread panicked; the counters and ring are still consistent.
        self.st.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reader thread, per chunk. Blocks while a WebView is attached and more than WINDOW behind; then the kernel
    /// PTY buffer holds the program back. Without a WebView the core confirms by itself (bytes only go to the ring),
    /// so a pane that is not shown never stalls. After close the chunk is discarded.
    fn push(&self, chunk: &[u8]) {
        let mut st = self.lock();
        while st.sink.is_some() && !st.closed && st.sent - st.acked > WINDOW {
            st = self.cv.wait(st).unwrap_or_else(|e| e.into_inner());
        }
        if st.closed {
            return;
        }
        st.replay.extend(chunk);
        let over = st.replay.len().saturating_sub(REPLAY);
        st.replay.drain(..over);
        if let Some(sink) = &st.sink {
            if sink.send(InvokeResponseBody::Raw(chunk.to_vec())).is_ok() {
                st.sent += chunk.len() as u64;
            } else {
                st.sink = None;
            }
        }
    }

    /// Replays the ring into `sink`, then flow control applies to it. Replaces any earlier sink.
    fn attach(&self, sink: Sink) {
        let mut st = self.lock();
        let replay: Vec<u8> = st.replay.iter().copied().collect();
        st.sent = replay.len() as u64;
        st.acked = 0;
        let ok = replay.is_empty() || sink.send(InvokeResponseBody::Raw(replay)).is_ok();
        st.sink = ok.then_some(sink);
        self.cv.notify_all();
    }

    fn detach(&self) {
        self.lock().sink = None;
        self.cv.notify_all();
    }

    /// `total` is cumulative (D-018): a lost ack is covered by the next one, an older (smaller) one is ignored.
    /// Clamped to `sent` so a bogus total cannot open the window beyond what was actually sent.
    fn ack(&self, total: u64) {
        let mut st = self.lock();
        st.acked = st.acked.max(total.min(st.sent));
        self.cv.notify_all();
    }

    fn close(&self) {
        let mut st = self.lock();
        st.closed = true;
        st.sink = None;
        self.cv.notify_all();
    }
}

pub(crate) struct Pane {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Writer,
    /// portable-pty calls setsid() in the child, so its pid is also its process group and session id.
    pgid: libc::pid_t,
    out: Arc<Output>,
    /// Last write that came from the person's keyboard or paste, not an xterm.js reply (ARCHITECTURE §5.2).
    typed_at: Mutex<Option<Instant>>,
    /// Headless screen the reader feeds (agent panes only), kept at the PTY's size.
    screen: Option<Arc<Mutex<Screen>>>,
}

/// Cheap to clone: clones share the same panes (the delivery engine keeps one).
#[derive(Default, Clone)]
pub struct PaneManager {
    panes: Arc<Mutex<HashMap<u32, Arc<Pane>>>>,
    next: Arc<AtomicU32>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// `killpg`; returns false when no process in the group could be signalled (e.g. ESRCH).
fn signal_group(pgid: libc::pid_t, sig: libc::c_int) -> bool {
    // SAFETY: killpg takes plain integers and has no memory-safety preconditions.
    unsafe { libc::killpg(pgid, sig) == 0 }
}

fn signal_pid(pid: libc::pid_t, sig: libc::c_int) -> bool {
    // SAFETY: kill takes plain integers and has no memory-safety preconditions.
    unsafe { libc::kill(pid, sig) == 0 }
}

/// Every process below `pid` by parent link, whatever its group or session.
#[cfg(target_os = "macos")]
fn descendants(pid: libc::pid_t) -> Vec<libc::pid_t> {
    let mut found = Vec::new();
    let mut todo = vec![pid];
    while let Some(p) = todo.pop() {
        // ponytail: 1024 children per process; a parent with more loses the rest from the scan.
        let mut buf = [0 as libc::pid_t; 1024];
        let size = std::mem::size_of_val(&buf) as libc::c_int;
        // SAFETY: `buf` is valid for `size` bytes. The call returns a count of pids, not bytes.
        let n = unsafe { libc::proc_listchildpids(p, buf.as_mut_ptr().cast(), size) };
        for &c in buf.iter().take(usize::try_from(n).unwrap_or(0)) {
            if c > 1 && !found.contains(&c) {
                found.push(c);
                todo.push(c);
            }
        }
    }
    found
}

#[cfg(not(target_os = "macos"))]
fn descendants(_pid: libc::pid_t) -> Vec<libc::pid_t> {
    Vec::new()
}

/// Waits until `pred` holds or `deadline` passes.
fn wait_until(deadline: Instant, pred: impl Fn() -> bool) -> bool {
    while !pred() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

impl PaneManager {
    /// Runs `launch.argv` directly, without a shell (D-009), with exactly `launch.env` (D-014). `on_event` gets the
    /// pane's status changes and, last, its exit. `screen`, if any, is fed every chunk and resized with the PTY.
    pub fn spawn(
        &self,
        launch: &Launch,
        cwd: &Path,
        (rows, cols): (u16, u16),
        screen: Option<Arc<Mutex<Screen>>>,
        on_event: OnEvent,
    ) -> Result<u32, String> {
        let (prog, args) = launch.argv.split_first().ok_or("empty argv")?;
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(err)?;
        let mut cmd = CommandBuilder::new(prog);
        cmd.args(args);
        cmd.cwd(cwd);
        cmd.env_clear();
        for (k, v) in &launch.env {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(err)?;
        drop(pair.slave);
        let pgid = child
            .process_id()
            .and_then(|p| libc::pid_t::try_from(p).ok())
            // killpg(0 or 1) would hit agentdeck's own group or every process; never allow it.
            .filter(|&p| p > 1);
        let Some(pgid) = pgid else {
            let _ = child.kill();
            return Err("child has no usable pid".into());
        };
        let mut reader = pair.master.try_clone_reader().map_err(err)?;
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().map_err(err)?));
        let out = Arc::new(Output::default());
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let pane = Pane {
            master: Mutex::new(pair.master),
            writer: writer.clone(),
            pgid,
            out: out.clone(),
            typed_at: Mutex::new(None),
            screen: screen.clone(),
        };
        lock(&self.panes).insert(id, Arc::new(pane));
        let mut scan = launch.osc_status.then(|| StatusScan {
            scanner: Scanner::default(),
            tracker: Tracker::default(),
        });

        std::thread::Builder::new()
            .name(format!("pane-{id}-reader"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 << 10];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        // Keep draining after close: on macOS an exiting process blocks in the tty close until
                        // its pending output is read, so stopping here would leave it stuck exiting.
                        Ok(n) => {
                            // Before push: a query must be answered even while flow control holds the output.
                            if let Some(scan) = &mut scan {
                                scan.feed(&buf[..n], &writer, &on_event);
                            }
                            if let Some(screen) = &screen {
                                lock(screen).process(&buf[..n]);
                            }
                            out.push(&buf[..n]);
                        }
                    }
                }
                // Reap the child so it does not linger as a zombie (and so close sees the group empty).
                let code = child.wait().ok().map(|s| s.exit_code());
                on_event(PaneEvent::Exited { code });
            })
            .map_err(err)?;
        Ok(id)
    }

    fn get(&self, id: u32) -> Result<Arc<Pane>, String> {
        lock(&self.panes)
            .get(&id)
            .cloned()
            .ok_or_else(|| "no such pane".into())
    }

    pub fn write(&self, id: u32, data: &[u8], typed: bool) -> Result<(), String> {
        let pane = self.get(id)?;
        if typed {
            *lock(&pane.typed_at) = Some(Instant::now());
        }
        let mut w = lock(&pane.writer);
        w.write_all(data).and_then(|_| w.flush()).map_err(err)
    }

    /// When the person last typed in the pane; Delivery's readiness check (`typing_ms`) reads it.
    pub fn typed_at(&self, id: u32) -> Result<Option<Instant>, String> {
        Ok(*lock(&self.get(id)?.typed_at))
    }

    fn resize(&self, id: u32, rows: u16, cols: u16) -> Result<(), String> {
        let pane = self.get(id)?;
        if let Some(screen) = &pane.screen {
            lock(screen).resize(rows, cols);
        }
        let master = lock(&pane.master);
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(err)
    }

    fn attach(&self, id: u32, sink: Sink) -> Result<(), String> {
        self.get(id)?.out.attach(sink);
        Ok(())
    }

    fn detach(&self, id: u32) -> Result<(), String> {
        self.get(id)?.out.detach();
        Ok(())
    }

    fn ack(&self, id: u32, total: u64) -> Result<(), String> {
        self.get(id)?.out.ack(total);
        Ok(())
    }

    /// Removes the pane; the caller then runs `terminate` off the async runtime (it blocks up to KILL_GRACE).
    pub(crate) fn remove(&self, id: u32) -> Result<Arc<Pane>, String> {
        lock(&self.panes)
            .remove(&id)
            .ok_or_else(|| "no such pane".into())
    }

    /// App exit (TERM-05): terminates every pane in parallel. Returns once all are gone, or at the latest after
    /// EXIT_LIMIT, so quitting never hangs; whatever is left then still gets the kernel's SIGHUP as the masters close.
    pub fn close_all(&self) {
        let panes: Vec<_> = lock(&self.panes).drain().map(|(_, p)| p).collect();
        let (tx, rx) = std::sync::mpsc::channel();
        for pane in panes {
            let tx = tx.clone();
            // If the thread cannot start, its closure (and `tx`) is dropped and the pane is left to the SIGHUP.
            let _ = std::thread::Builder::new()
                .name("pane-close".into())
                .spawn(move || {
                    terminate(&pane);
                    let _ = tx.send(());
                });
        }
        drop(tx);
        let deadline = Instant::now() + EXIT_LIMIT;
        // Ends when every thread has reported and dropped its sender (Disconnected), or at the deadline.
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if rx.recv_timeout(left).is_err() {
                break;
            }
        }
    }
}

/// SIGHUP + SIGTERM to the process group and every descendant, SIGKILL after KILL_GRACE. SIGHUP is what closing a
/// terminal sends: an interactive shell ignores SIGTERM, but on SIGHUP it hangs up its background jobs. The
/// descendant scan reaches what killpg cannot: jobs in their own group and processes in a session of their own
/// (Claude Code's Bash tool runs each command that way, `ps` shows it as `Ss`).
pub(crate) fn terminate(pane: &Pane) {
    pane.out.close();
    // Snapshot before signalling: once a parent dies its children move to launchd and drop out of the tree.
    // ponytail: a pid that exits during the grace and is reused within it would get the SIGKILL; check start
    // times if that ever matters. Processes reparented to launchd before close (double fork) are not found.
    let tree = descendants(pane.pgid);
    let signal = |sig| {
        signal_group(pane.pgid, sig);
        for &p in &tree {
            signal_pid(p, sig);
        }
    };
    let gone = || !signal_group(pane.pgid, 0) && !tree.iter().any(|&p| signal_pid(p, 0));
    signal(libc::SIGHUP);
    signal(libc::SIGTERM);
    if !wait_until(Instant::now() + KILL_GRACE, gone) {
        signal(libc::SIGKILL);
        wait_until(Instant::now() + Duration::from_secs(1), gone);
    }
}

#[tauri::command]
pub async fn pane_attach(
    panes: State<'_, PaneManager>,
    id: u32,
    on_data: Sink,
) -> Result<(), String> {
    panes.attach(id, on_data)
}

#[tauri::command]
pub async fn pane_detach(panes: State<'_, PaneManager>, id: u32) -> Result<(), String> {
    panes.detach(id)
}

#[tauri::command]
pub async fn pane_write(
    panes: State<'_, PaneManager>,
    engine: State<'_, crate::delivery::Engine>,
    id: u32,
    data: String,
    typed: bool,
) -> Result<(), String> {
    panes.write(id, data.as_bytes(), typed)?;
    // Enter from the person (not an xterm.js reply) starts the loop guard over (ARCHITECTURE §5.5).
    if typed && data.contains('\r') {
        engine.human_enter(id);
    }
    Ok(())
}

#[tauri::command]
pub async fn pane_resize(
    panes: State<'_, PaneManager>,
    id: u32,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    panes.resize(id, rows, cols)
}

#[tauri::command]
pub async fn pane_ack(panes: State<'_, PaneManager>, id: u32, total: u64) -> Result<(), String> {
    panes.ack(id, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch;
    use std::process::Command;
    use std::sync::mpsc::{Receiver, channel};

    const CHUNK: usize = 64 << 10;

    fn sink() -> (Sink, Receiver<Vec<u8>>) {
        let (tx, rx) = channel();
        let ch = Channel::new(move |body| {
            if let InvokeResponseBody::Raw(b) = body {
                let _ = tx.send(b);
            }
            Ok(())
        });
        (ch, rx)
    }

    fn pusher(out: &Arc<Output>, chunks: usize) -> Receiver<()> {
        let (done, rx) = channel();
        let out = out.clone();
        std::thread::spawn(move || {
            for _ in 0..chunks {
                out.push(&[b'x'; CHUNK]);
            }
            let _ = done.send(());
        });
        rx
    }

    /// Chunks the reader sends before it waits: while sent - acked <= WINDOW, i.e. at 0, CHUNK, ..., WINDOW.
    const PER_WINDOW: usize = WINDOW as usize / CHUNK + 1;

    const SHORT: Duration = Duration::from_millis(200);
    const LONG: Duration = Duration::from_secs(5);

    #[test]
    fn credit_stops_at_window_and_resumes_after_ack() {
        let out = Arc::new(Output::default());
        let (ch, rx) = sink();
        out.attach(ch);
        let n = PER_WINDOW;
        let done = pusher(&out, 2 * n);
        for _ in 0..n {
            assert_eq!(rx.recv_timeout(LONG).map(|b| b.len()), Ok(CHUNK));
        }
        assert!(
            rx.recv_timeout(SHORT).is_err(),
            "reader must stop at the window"
        );
        assert!(done.try_recv().is_err());
        out.ack((n * CHUNK) as u64);
        for _ in 0..n {
            assert!(rx.recv_timeout(LONG).is_ok());
        }
        assert!(done.recv_timeout(LONG).is_ok());
    }

    #[test]
    fn stale_ack_is_ignored() {
        let out = Arc::new(Output::default());
        let (ch, rx) = sink();
        out.attach(ch);
        let n = PER_WINDOW;
        let _done = pusher(&out, 2 * n);
        for _ in 0..n {
            assert!(rx.recv_timeout(LONG).is_ok());
        }
        out.ack((n * CHUNK) as u64); // frees n more chunks
        out.ack(CHUNK as u64); // older total arriving late: must not shrink the window
        for _ in 0..n {
            assert!(
                rx.recv_timeout(LONG).is_ok(),
                "stale ack must not stall the reader"
            );
        }
        assert_eq!(out.lock().acked, (n * CHUNK) as u64);
        out.ack(u64::MAX); // more than was sent: clamped
        assert_eq!(out.lock().acked, (2 * n * CHUNK) as u64);
    }

    #[test]
    fn without_channel_never_blocks_and_replays_on_attach() {
        let out = Arc::new(Output::default());
        let done = pusher(&out, 4 * REPLAY / CHUNK); // 4 MB, far beyond the window
        assert!(
            done.recv_timeout(LONG).is_ok(),
            "no channel attached: reader must not wait for acks"
        );
        out.push(b"tail");
        let (ch, rx) = sink();
        out.attach(ch);
        let replay = rx.recv_timeout(LONG).unwrap_or_default();
        assert_eq!(replay.len(), REPLAY);
        assert!(replay.ends_with(b"tail"));
        // Replay counts against the window: 1 MB unconfirmed, so the reader now waits.
        let done = pusher(&out, 1);
        assert!(done.recv_timeout(SHORT).is_err());
        out.detach();
        assert!(
            done.recv_timeout(LONG).is_ok(),
            "detach releases the reader"
        );
    }

    #[test]
    fn close_releases_a_blocked_reader() {
        let out = Arc::new(Output::default());
        let (ch, _rx) = sink();
        out.attach(ch);
        let done = pusher(&out, 10);
        assert!(done.recv_timeout(SHORT).is_err());
        out.close();
        assert!(done.recv_timeout(LONG).is_ok());
    }

    fn recv_until(rx: &Receiver<Vec<u8>>, needle: &str) -> bool {
        let deadline = Instant::now() + LONG;
        let mut seen = Vec::new();
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(b) => seen.extend(b),
                Err(_) => break,
            }
            if String::from_utf8_lossy(&seen).contains(needle) {
                return true;
            }
        }
        false
    }

    fn alive(pid: libc::pid_t) -> bool {
        signal_pid(pid, 0)
    }

    /// Polls until `pred` holds, up to LONG.
    fn eventually(mut pred: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + LONG;
        while Instant::now() < deadline {
            if pred() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// `a` run directly with the from-scratch env (test process PATH) and no status scan.
    fn argv(a: &[&str]) -> Launch {
        let path = std::env::var("PATH").unwrap_or_default();
        Launch {
            argv: a.iter().map(|s| s.to_string()).collect(),
            env: launch::env(|k| std::env::var_os(k), &path),
            osc_status: false,
        }
    }

    fn ignore() -> OnEvent {
        Box::new(|_| {})
    }

    fn events() -> (OnEvent, Receiver<PaneEvent>) {
        let (tx, rx) = channel();
        (
            Box::new(move |ev| {
                let _ = tx.send(ev);
            }),
            rx,
        )
    }

    #[test]
    fn child_env_is_only_the_launch_env() {
        let allowed: Vec<&str> = launch::INHERIT
            .into_iter()
            .chain(["PATH", "TERM", "COLORTERM"])
            .collect();
        // cargo runs tests with CARGO_* (and Claude Code sessions add CLAUDE_CODE_*): there is something to leak.
        assert!(std::env::vars_os().any(|(k, _)| !allowed.iter().any(|a| k == *a)));
        let pm = PaneManager::default();
        let (on_event, done) = events();
        let id = pm
            .spawn(
                &argv(&["/usr/bin/env"]),
                Path::new("/"),
                (24, 200),
                None,
                on_event,
            )
            .unwrap();
        assert!(matches!(
            done.recv_timeout(LONG),
            Ok(PaneEvent::Exited { .. })
        ));
        let (ch, rx) = sink();
        pm.attach(id, ch).unwrap();
        let out = String::from_utf8(rx.recv_timeout(LONG).unwrap()).unwrap();
        let keys: Vec<&str> = out
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, _)| k)
            .collect();
        assert!(keys.contains(&"COLORTERM"), "{out}");
        for k in keys {
            assert!(allowed.contains(&k), "{k} leaked into the child");
        }
    }

    #[test]
    fn exit_code_is_reported() {
        let pm = PaneManager::default();
        let (on_event, rx) = events();
        pm.spawn(
            &argv(&["/bin/sh", "-c", "exit 7"]),
            Path::new("/"),
            (24, 80),
            None,
            on_event,
        )
        .unwrap();
        assert_eq!(
            rx.recv_timeout(LONG),
            Ok(PaneEvent::Exited { code: Some(7) })
        );
    }

    #[test]
    fn echo_appears_in_stream() {
        let pm = PaneManager::default();
        let id = pm
            .spawn(
                &argv(&["/bin/sh"]),
                Path::new("/"),
                (24, 80),
                None,
                ignore(),
            )
            .unwrap();
        let (ch, rx) = sink();
        pm.attach(id, ch).unwrap();
        // The expansion makes the output differ from the echoed input.
        pm.write(id, b"echo hi$((40+2))\n", false).unwrap();
        assert!(recv_until(&rx, "hi42"));
        assert_eq!(
            pm.typed_at(id),
            Ok(None),
            "a reply must not count as typing"
        );
        let before = Instant::now();
        pm.write(id, b"\n", true).unwrap();
        assert!(pm.typed_at(id).unwrap().is_some_and(|t| t >= before));
        pm.resize(id, 40, 120).unwrap();
        terminate(&pm.remove(id).unwrap());
        assert!(pm.write(id, b"x", true).is_err());
    }

    /// Types `jobs` into an interactive shell and returns once the shell has registered them.
    fn type_jobs(pm: &PaneManager, id: u32, jobs: &[u8]) {
        let (ch, rx) = sink();
        pm.attach(id, ch).unwrap();
        pm.write(id, jobs, false).unwrap();
        // Wait until the shell has run the next command, i.e. it is done registering the background jobs.
        pm.write(id, b"echo ready-$((1+1))\n", false).unwrap();
        assert!(recv_until(&rx, "ready-2"));
        pm.detach(id).unwrap();
    }

    /// The pane's leader plus its descendants, once there are at least `expect` of them.
    fn tree(pm: &PaneManager, id: u32, expect: usize) -> Vec<libc::pid_t> {
        let leader = pm.get(id).unwrap().pgid;
        let mut tree = Vec::new();
        assert!(eventually(|| {
            tree = descendants(leader);
            tree.len() >= expect
        }));
        tree.push(leader);
        tree
    }

    fn assert_gone(tree: &[libc::pid_t]) {
        for &pid in tree {
            if !eventually(|| !alive(pid)) {
                let ps = Command::new("ps")
                    .args(["-o", "pid,ppid,pgid,stat,command", "-p", &pid.to_string()])
                    .output()
                    .unwrap();
                panic!(
                    "pid {pid} survived: {}",
                    String::from_utf8_lossy(&ps.stdout)
                );
            }
        }
    }

    fn assert_close_kills_tree(argv: &Launch, setup: Option<&[u8]>, expect: usize) {
        let pm = PaneManager::default();
        let id = pm
            .spawn(argv, Path::new("/"), (24, 80), None, ignore())
            .unwrap();
        if let Some(input) = setup {
            type_jobs(&pm, id, input);
        }
        let tree = tree(&pm, id, expect);
        terminate(&pm.remove(id).unwrap());
        assert_gone(&tree);
    }

    #[test]
    fn close_stops_child_and_grandchild() {
        // sh -> (sh -> sleep, sleep); `; true` keeps sh from exec'ing straight into sleep.
        let cmd = "sh -c 'sleep 300; true' & sleep 300; true";
        assert_close_kills_tree(&argv(&["/bin/sh", "-c", cmd]), None, 3);
    }

    #[test]
    fn close_stops_background_job_of_interactive_shell() {
        // Job control puts the job in its own process group, out of reach of killpg on the shell's group.
        assert_close_kills_tree(
            &argv(&["/bin/zsh", "-f"]),
            Some(b"sh -c 'sleep 301; true' &\n"),
            2,
        );
    }

    /// Seven descendants: sh -> sleep; sh -> sleep that ignore SIGHUP and SIGTERM; perl -> sleep in a session of its
    /// own (like Claude Code's Bash tool); and a foreground sleep.
    const JOBS: &[u8] = b"sh -c 'sleep 305; true' &\n\
        sh -c 'trap \"\" HUP TERM; sleep 306; true' &\n\
        perl -MPOSIX -e 'if (my $p = fork) { waitpid $p, 0 } else { setsid; exec \"sleep\", 307 }' &\n";

    #[test]
    fn close_all_stops_every_pane_in_parallel() {
        let pm = PaneManager::default();
        let mut all = Vec::new();
        for _ in 0..3 {
            let id = pm
                .spawn(
                    &argv(&["/bin/zsh", "-f"]),
                    Path::new("/"),
                    (24, 80),
                    None,
                    ignore(),
                )
                .unwrap();
            type_jobs(&pm, id, JOBS);
            pm.write(id, b"sleep 308\n", false).unwrap();
            all.extend(tree(&pm, id, 7));
        }
        // SAFETY: getsid takes a plain integer.
        let sessions = all.iter().filter(|&&p| unsafe { libc::getsid(p) } == p);
        assert_eq!(sessions.count(), 6, "3 shells + 3 setsid'd sleeps");
        let start = Instant::now();
        pm.close_all();
        let took = start.elapsed();
        assert_gone(&all);
        assert!(lock(&pm.panes).is_empty());
        // Each pane needs the full grace (a job ignores SIGTERM); in parallel that is one grace, not three.
        assert!(took < KILL_GRACE + Duration::from_secs(2), "{took:?}");
    }

    const CRASH_HELPER: &str = "AGENTDECK_CRASH_HELPER";

    /// Holds two shell panes until `crash_leaves_no_process` SIGKILLs it.
    #[test]
    #[ignore = "child process of crash_leaves_no_process"]
    fn crash_helper() {
        if std::env::var_os(CRASH_HELPER).is_none() {
            return;
        }
        let pm = PaneManager::default();
        let mut all = Vec::new();
        for _ in 0..2 {
            let id = pm
                .spawn(
                    &argv(&["/bin/zsh", "-f"]),
                    Path::new("/"),
                    (24, 80),
                    None,
                    ignore(),
                )
                .unwrap();
            type_jobs(&pm, id, b"sh -c 'sleep 310; true' &\n");
            pm.write(id, b"sleep 311\n", false).unwrap();
            all.extend(tree(&pm, id, 3));
        }
        let pids: Vec<String> = all.iter().map(|p| p.to_string()).collect();
        println!("pids {}", pids.join(" "));
        loop {
            std::thread::sleep(LONG);
        }
    }

    #[test]
    fn crash_leaves_no_process() {
        use std::io::{BufRead, BufReader};
        let mut helper = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "pty::tests::crash_helper",
                "--ignored",
                "--nocapture",
            ])
            .env(CRASH_HELPER, "1")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let line = BufReader::new(helper.stdout.take().unwrap())
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.strip_prefix("pids ").map(str::to_owned))
            .expect("helper printed no pids");
        let tree: Vec<libc::pid_t> = line.split(' ').map(|p| p.parse().unwrap()).collect();
        assert_eq!(tree.len(), 8);
        helper.kill().unwrap(); // SIGKILL: no destructor, no close_all; only the kernel closes the masters.
        helper.wait().unwrap();
        assert_gone(&tree);
    }
}
