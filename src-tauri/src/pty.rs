//! Pane PTYs: spawn, input, resize, flow-controlled output (D-018), process-group shutdown (ARCHITECTURE §3).
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

/// Max bytes sent to the WebView that xterm.js has not confirmed yet (D-018).
pub const WINDOW: u64 = 256 << 10;
/// Latest output kept for replay when a WebView (re)attaches; the oldest bytes are dropped.
pub const REPLAY: usize = 1 << 20;
/// Time between SIGTERM and SIGKILL on close.
const KILL_GRACE: Duration = Duration::from_secs(3);
/// The only variables a pane inherits from agentdeck (D-014); TERM/COLORTERM are set explicitly.
const PASS_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "SHELL", "LANG", "PATH"];

type Sink = Channel<InvokeResponseBody>;

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

struct Pane {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    /// portable-pty calls setsid() in the child, so its pid is also its process group and session id.
    pgid: libc::pid_t,
    out: Arc<Output>,
}

#[derive(Default)]
pub struct PaneManager {
    panes: Mutex<HashMap<u32, Arc<Pane>>>,
    next: AtomicU32,
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

/// Waits until the group is gone or `deadline` passes.
fn wait_group(pgid: libc::pid_t, deadline: Instant) -> bool {
    while signal_group(pgid, 0) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    true
}

impl PaneManager {
    /// Runs `argv` directly, without a shell (D-009), with an environment built from scratch (D-014).
    fn spawn(&self, argv: &[String], cwd: &Path, rows: u16, cols: u16) -> Result<u32, String> {
        let (prog, args) = argv.split_first().ok_or("empty argv")?;
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
        for key in PASS_ENV {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
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
        let writer = pair.master.take_writer().map_err(err)?;
        let out = Arc::new(Output::default());
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let pane = Pane {
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            pgid,
            out: out.clone(),
        };
        lock(&self.panes).insert(id, Arc::new(pane));

        std::thread::Builder::new()
            .name(format!("pane-{id}-reader"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 << 10];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        // Keep draining after close: on macOS an exiting process blocks in the tty close until
                        // its pending output is read, so stopping here would leave it stuck exiting.
                        Ok(n) => out.push(&buf[..n]),
                    }
                }
                // Reap the child so it does not linger as a zombie (and so close sees the group empty).
                let _ = child.wait();
            })
            .map_err(err)?;
        Ok(id)
    }

    /// `$SHELL -l` in `cwd`.
    fn spawn_shell(&self, cwd: &str, rows: u16, cols: u16) -> Result<u32, String> {
        let cwd = Path::new(cwd);
        if !cwd.is_dir() {
            return Err("cwd is not a directory".into());
        }
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        self.spawn(&[shell, "-l".into()], cwd, rows, cols)
    }

    fn get(&self, id: u32) -> Result<Arc<Pane>, String> {
        lock(&self.panes)
            .get(&id)
            .cloned()
            .ok_or_else(|| "no such pane".into())
    }

    fn write(&self, id: u32, data: &[u8]) -> Result<(), String> {
        let pane = self.get(id)?;
        let mut w = lock(&pane.writer);
        w.write_all(data).and_then(|_| w.flush()).map_err(err)
    }

    fn resize(&self, id: u32, rows: u16, cols: u16) -> Result<(), String> {
        let pane = self.get(id)?;
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
    fn remove(&self, id: u32) -> Result<Arc<Pane>, String> {
        lock(&self.panes)
            .remove(&id)
            .ok_or_else(|| "no such pane".into())
    }
}

/// SIGHUP + SIGTERM to the process group, SIGKILL after KILL_GRACE. SIGHUP is what closing a terminal sends: an
/// interactive shell ignores SIGTERM, but on SIGHUP it hangs up its background jobs, which live in their own groups.
fn terminate(pane: &Pane) {
    pane.out.close();
    signal_group(pane.pgid, libc::SIGHUP);
    signal_group(pane.pgid, libc::SIGTERM);
    if !wait_group(pane.pgid, Instant::now() + KILL_GRACE) {
        signal_group(pane.pgid, libc::SIGKILL);
        // ponytail: a job that left the group and ignores SIGHUP (nohup ... &) survives and keeps the PTY open;
        // killing the whole session needs a process scan (T1.8 if it shows up with real agents).
        wait_group(pane.pgid, Instant::now() + Duration::from_secs(1));
    }
}

#[tauri::command]
pub async fn pane_create_shell(
    panes: State<'_, PaneManager>,
    cwd: String,
    rows: u16,
    cols: u16,
) -> Result<u32, String> {
    panes.spawn_shell(&cwd, rows, cols)
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
    id: u32,
    data: String,
) -> Result<(), String> {
    panes.write(id, data.as_bytes())
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

#[tauri::command]
pub async fn pane_close(panes: State<'_, PaneManager>, id: u32) -> Result<(), String> {
    let pane = panes.remove(id)?;
    tauri::async_runtime::spawn_blocking(move || terminate(&pane))
        .await
        .map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
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

    const SHORT: Duration = Duration::from_millis(200);
    const LONG: Duration = Duration::from_secs(5);

    #[test]
    fn credit_stops_at_window_and_resumes_after_ack() {
        let out = Arc::new(Output::default());
        let (ch, rx) = sink();
        out.attach(ch);
        let done = pusher(&out, 10);
        // Sends while sent - acked <= WINDOW: 0, 64K, ..., 256K -> 5 chunks, then the reader waits.
        for _ in 0..5 {
            assert_eq!(rx.recv_timeout(LONG).map(|b| b.len()), Ok(CHUNK));
        }
        assert!(
            rx.recv_timeout(SHORT).is_err(),
            "reader must stop at the window"
        );
        assert!(done.try_recv().is_err());
        out.ack(5 * CHUNK as u64);
        for _ in 0..5 {
            assert!(rx.recv_timeout(LONG).is_ok());
        }
        assert!(done.recv_timeout(LONG).is_ok());
    }

    #[test]
    fn stale_ack_is_ignored() {
        let out = Arc::new(Output::default());
        let (ch, rx) = sink();
        out.attach(ch);
        let _done = pusher(&out, 10);
        for _ in 0..5 {
            assert!(rx.recv_timeout(LONG).is_ok());
        }
        out.ack(5 * CHUNK as u64); // frees 5 more chunks
        out.ack(CHUNK as u64); // older total arriving late: must not shrink the window
        for _ in 0..5 {
            assert!(
                rx.recv_timeout(LONG).is_ok(),
                "stale ack must not stall the reader"
            );
        }
        assert_eq!(out.lock().acked, 5 * CHUNK as u64);
        out.ack(u64::MAX); // more than was sent: clamped
        assert_eq!(out.lock().acked, 10 * CHUNK as u64);
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

    fn children(pid: u32) -> Vec<u32> {
        let out = Command::new("pgrep")
            .arg("-P")
            .arg(pid.to_string())
            .output();
        let stdout = out.map(|o| o.stdout).unwrap_or_default();
        String::from_utf8_lossy(&stdout)
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect()
    }

    fn descendants(pid: u32) -> Vec<u32> {
        children(pid)
            .into_iter()
            .flat_map(|c| std::iter::once(c).chain(descendants(c)))
            .collect()
    }

    fn alive(pid: u32) -> bool {
        // SAFETY: kill with signal 0 only checks for existence.
        unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
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

    fn argv(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn echo_appears_in_stream() {
        let pm = PaneManager::default();
        let id = pm
            .spawn(&argv(&["/bin/sh"]), Path::new("/"), 24, 80)
            .unwrap();
        let (ch, rx) = sink();
        pm.attach(id, ch).unwrap();
        // The expansion makes the output differ from the echoed input.
        pm.write(id, b"echo hi$((40+2))\n").unwrap();
        assert!(recv_until(&rx, "hi42"));
        pm.resize(id, 40, 120).unwrap();
        terminate(&pm.remove(id).unwrap());
        assert!(pm.write(id, b"x").is_err());
    }

    fn assert_close_kills_tree(argv: &[String], setup: Option<&[u8]>, expect: usize) {
        let pm = PaneManager::default();
        let id = pm.spawn(argv, Path::new("/"), 24, 80).unwrap();
        let leader = pm.get(id).unwrap().pgid as u32;
        if let Some(input) = setup {
            let (ch, rx) = sink();
            pm.attach(id, ch).unwrap();
            pm.write(id, input).unwrap();
            // Wait until the shell has run the next command, i.e. it is done registering the background job.
            pm.write(id, b"echo ready-$((1+1))\n").unwrap();
            assert!(recv_until(&rx, "ready-2"));
        }
        let mut tree = Vec::new();
        assert!(eventually(|| {
            tree = descendants(leader);
            tree.len() >= expect
        }));
        tree.push(leader);
        terminate(&pm.remove(id).unwrap());
        for pid in tree {
            if !eventually(|| !alive(pid)) {
                let ps = Command::new("ps")
                    .args(["-o", "pid,ppid,pgid,stat,command", "-p", &pid.to_string()])
                    .output()
                    .unwrap();
                panic!(
                    "pid {pid} survived (leader {leader}): {}",
                    String::from_utf8_lossy(&ps.stdout)
                );
            }
        }
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
}
