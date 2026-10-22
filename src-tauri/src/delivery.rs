//! Message delivery (ARCHITECTURE §5.1–5.3, MCP-TOOLS `pane_send`): sanitising, the sender marker, the paste shape,
//! a FIFO queue per target pane, and the readiness rule. Pure logic: time is passed in, PTY writes are returned as
//! actions, so tests use a fake clock. Wiring to PTY, screen, and the `pane_send` tool is T2.7.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::time::{Duration, Instant};

use crate::policy::{LoopGuard, QUEUE_PER_TARGET, SEND_MAX_BYTES};

/// Sender marker limit (ARCHITECTURE §5.3).
pub const MARKER_MAX_BYTES: usize = 256;
/// Agent names come from the user's catalogue; capped so the marker always fits.
const AGENT_MAX_BYTES: usize = 32;
/// Pause between the paste and Enter (D-013).
pub const ENTER_DELAY: Duration = Duration::from_millis(150);
pub const PASTE_START: &[u8] = b"\x1b[200~";
pub const PASTE_END: &[u8] = b"\x1b[201~";

/// `quiet_ms` and `typing_ms` (§5.2). Defaults are unmeasured; tuned in T2.7 against the real CLIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    pub quiet: Duration,
    pub typing: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(1500),
            typing: Duration::from_millis(2000),
        }
    }
}

/// `pane_send` errors decided here; `code()` is the MCP-TOOLS code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendError {
    Empty,
    TooLarge,
    QueueFull,
}

impl SendError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Empty => "BAD_ARGS",
            Self::TooLarge => "TOO_LARGE",
            Self::QueueFull => "QUEUE_FULL",
        }
    }
}

impl fmt::Display for SendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        f.write_str(match self {
            Self::Empty => "text is empty after removing control characters. Send some text.",
            Self::TooLarge => {
                "text is over 8192 bytes. Summarise; point to a file or commit instead of pasting long logs."
            }
            Self::QueueFull => {
                "5 messages are already waiting for this pane. Wait for it to process them."
            }
        })
    }
}

impl std::error::Error for SendError {}

/// Control characters except `\n` removed, `ESC[201~` removed (so the body cannot close the paste early).
/// `Err(Empty)` for nothing left, `Err(TooLarge)` for more than 8192 UTF-8 bytes.
pub fn sanitize(text: &str) -> Result<String, SendError> {
    let out: String = text
        .replace("\x1b[201~", "")
        .chars()
        .filter(|&c| c == '\n' || !c.is_control())
        .collect();
    if out.is_empty() {
        Err(SendError::Empty)
    } else if out.len() > SEND_MAX_BYTES {
        Err(SendError::TooLarge)
    } else {
        Ok(out)
    }
}

/// Who sent a message. Filled by agentdeck from the sender's token, never from tool arguments.
#[derive(Clone, Copy, Debug)]
pub struct Sender<'a> {
    pub pane: &'a str,
    pub agent: &'a str,
    pub title: Option<&'a str>,
}

fn one_line(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

fn cut(s: &str, max: usize) -> &str {
    &s[..s.floor_char_boundary(max)]
}

/// `[agentdeck] message from p2 (codex, "review: auth"). Reply with pane_send to "p2".`, at most 256 bytes; the title
/// is cut at a UTF-8 boundary to fit. Assumes agentdeck pane ids are short (`p<N>`).
pub fn marker(from: &Sender) -> String {
    let agent = one_line(from.agent);
    let agent = cut(&agent, AGENT_MAX_BYTES);
    let pane = from.pane;
    let tail = format!("). Reply with pane_send to \"{pane}\".");
    let head = format!("[agentdeck] message from {pane} ({agent}");
    match from.title.map(one_line).filter(|t| !t.is_empty()) {
        None => format!("{head}{tail}"),
        Some(title) => {
            let budget = MARKER_MAX_BYTES.saturating_sub(head.len() + tail.len() + 4); // `, "` + `"`
            format!("{head}, \"{}\"{tail}", cut(&title, budget))
        }
    }
}

/// One step of a delivery, executed in order by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Write(Vec<u8>),
    Sleep(Duration),
}

/// `ESC[200~<marker>\n<body>ESC[201~`, wait 150 ms, `\r` (§5.3). `body` must come from `sanitize`.
pub fn paste_actions(marker: &str, body: &str) -> Vec<Action> {
    let mut paste =
        Vec::with_capacity(PASTE_START.len() + marker.len() + 1 + body.len() + PASTE_END.len());
    paste.extend_from_slice(PASTE_START);
    paste.extend_from_slice(marker.as_bytes());
    paste.push(b'\n');
    paste.extend_from_slice(body.as_bytes());
    paste.extend_from_slice(PASTE_END);
    vec![
        Action::Write(paste),
        Action::Sleep(ENTER_DELAY),
        Action::Write(b"\r".to_vec()),
    ]
}

/// Pane status as far as delivery cares (ARCHITECTURE §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneStatus {
    Idle,
    /// No official status signal; fall back to output quietness.
    Unknown,
    /// `starting`, `working`, or `needs_you`.
    Busy,
}

/// What `vt100` says about the agent's input box (§5.2 item 4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputState {
    Empty,
    HasText,
    Unknown,
}

/// Snapshot of the target pane, gathered by the caller from PTY, screen, and status tracker.
#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub alive: bool,
    pub status: PaneStatus,
    pub last_output: Instant,
    /// Last real keystroke or paste by the person (not xterm.js `reply` traffic).
    pub last_typed: Option<Instant>,
    pub input: InputState,
    pub bracketed_paste: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitReason {
    Paused,
    Busy,
    NotQuiet,
    Typing,
    /// UI chip "pesan menunggu: kosongkan input".
    InputNotEmpty,
    InputUnknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropReason {
    TargetExited,
    NoBracketedPaste,
    /// The person chose "Hentikan" on the loop-guard card.
    Stopped,
}

impl DropReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TargetExited => "target agent exited",
            Self::NoBracketedPaste => "target terminal has bracketed paste off",
            Self::Stopped => "stopped by the person at the loop guard",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Wait(WaitReason),
    Drop(DropReason),
}

/// §5.2. Bracketed paste is checked last, only when the paste would otherwise go out, so a TUI that briefly turns it
/// off while busy does not lose messages.
pub fn ready(t: &Target, paused: bool, timing: &Timing, now: Instant) -> Readiness {
    let since = |at: Instant| now.saturating_duration_since(at);
    if !t.alive {
        return Readiness::Drop(DropReason::TargetExited);
    }
    if paused {
        return Readiness::Wait(WaitReason::Paused);
    }
    match t.status {
        PaneStatus::Idle => {}
        PaneStatus::Unknown if since(t.last_output) >= timing.quiet => {}
        PaneStatus::Unknown => return Readiness::Wait(WaitReason::NotQuiet),
        PaneStatus::Busy => return Readiness::Wait(WaitReason::Busy),
    }
    if t.last_typed.is_some_and(|at| since(at) < timing.typing) {
        return Readiness::Wait(WaitReason::Typing);
    }
    match t.input {
        InputState::Empty => {}
        InputState::HasText => return Readiness::Wait(WaitReason::InputNotEmpty),
        InputState::Unknown => return Readiness::Wait(WaitReason::InputUnknown),
    }
    if !t.bracketed_paste {
        return Readiness::Drop(DropReason::NoBracketedPaste);
    }
    Readiness::Ready
}

/// Message state for the `pane_send` result and the message log (MSG-05).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Delivered,
    Paused,
    Dropped(DropReason),
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Delivered => "delivered",
            Self::Paused => "paused",
            Self::Dropped(_) => "dropped",
        }
    }
}

#[derive(Debug)]
struct Message {
    id: String,
    marker: String,
    body: String,
}

#[derive(Debug, Default)]
struct Queue {
    items: VecDeque<Message>,
    last_delivered: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    Empty,
    Wait(WaitReason),
    /// Message `id` left the queue as delivered; the caller runs `actions` and sets the target `working` (§5.1).
    Deliver {
        id: String,
        actions: Vec<Action>,
    },
    Dropped(Vec<(String, DropReason)>),
}

/// All queues, keyed by target pane id.
#[derive(Debug, Default)]
pub struct Delivery {
    pub timing: Timing,
    next_id: u64,
    queues: HashMap<String, Queue>,
}

impl Delivery {
    /// `guard` is the loop guard of the target's workspace. Returns the new id and `queued` or `paused`.
    /// Relation and exited-target checks are `policy::can_send`, done before this.
    pub fn enqueue(
        &mut self,
        from: &Sender,
        to: &str,
        text: &str,
        guard: &LoopGuard,
    ) -> Result<(String, State), SendError> {
        let body = sanitize(text)?;
        let q = self.queues.entry(to.to_owned()).or_default();
        if q.items.len() >= QUEUE_PER_TARGET {
            return Err(SendError::QueueFull);
        }
        self.next_id += 1;
        let id = format!("m{}", self.next_id);
        q.items.push_back(Message {
            id: id.clone(),
            marker: marker(from),
            body,
        });
        let state = if guard.is_paused() {
            State::Paused
        } else {
            State::Queued
        };
        Ok((id, state))
    }

    pub fn pending(&self, to: &str) -> usize {
        self.queues.get(to).map_or(0, |q| q.items.len())
    }

    /// Looks at the head of `to`'s queue. On delivery, counts it on `guard` (§5.5).
    pub fn poll(
        &mut self,
        to: &str,
        target: &Target,
        guard: &mut LoopGuard,
        loop_limit: u32,
        now: Instant,
    ) -> Poll {
        let Some(q) = self.queues.get_mut(to).filter(|q| !q.items.is_empty()) else {
            return Poll::Empty;
        };
        // The paste's own echo is output too, but do not rely on it: quietness counts from the last delivery.
        let t = Target {
            last_output: q
                .last_delivered
                .map_or(target.last_output, |d| d.max(target.last_output)),
            ..*target
        };
        match ready(&t, guard.is_paused(), &self.timing, now) {
            Readiness::Wait(why) => Poll::Wait(why),
            Readiness::Drop(DropReason::NoBracketedPaste) => match q.items.pop_front() {
                Some(m) => Poll::Dropped(vec![(m.id, DropReason::NoBracketedPaste)]),
                None => Poll::Empty,
            },
            Readiness::Drop(why) => self.drop_all(to, why),
            Readiness::Ready => match q.items.pop_front() {
                Some(m) => {
                    q.last_delivered = Some(now);
                    guard.record_delivered(loop_limit);
                    Poll::Deliver {
                        id: m.id,
                        actions: paste_actions(&m.marker, &m.body),
                    }
                }
                None => Poll::Empty,
            },
        }
    }

    /// Target exited or the person chose "Hentikan": every waiting message becomes `dropped`.
    pub fn drop_all(&mut self, to: &str, why: DropReason) -> Poll {
        let ids: Vec<_> = self
            .queues
            .remove(to)
            .map(|q| q.items.into_iter().map(|m| (m.id, why)).collect())
            .unwrap_or_default();
        if ids.is_empty() {
            Poll::Empty
        } else {
            Poll::Dropped(ids)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: Sender = Sender {
        pane: "p1",
        agent: "claude",
        title: Some("main"),
    };

    // ---- sanitize ----

    #[test]
    fn sanitize_strips_control_keeps_newline() {
        let s = sanitize("a\tb\r\nc\x07d\x7f\u{9b}e\u{85}f\x1b[31mg").unwrap();
        assert_eq!(s, "ab\ncdef[31mg");
    }

    #[test]
    fn sanitize_removes_paste_end() {
        assert_eq!(sanitize("x\x1b[201~y").unwrap(), "xy");
        // Nested attempt: no ESC survives, so no sequence can be rebuilt.
        let s = sanitize("\x1b\x1b[201~[201~z").unwrap();
        assert!(!s.contains('\x1b'));
        assert_eq!(sanitize("\u{9b}201~").unwrap(), "201~");
    }

    #[test]
    fn sanitize_empty() {
        assert_eq!(sanitize(""), Err(SendError::Empty));
        assert_eq!(sanitize("\x1b[201~\r\t\x00"), Err(SendError::Empty));
        assert_eq!(SendError::Empty.code(), "BAD_ARGS");
    }

    #[test]
    fn sanitize_limit_exact() {
        assert_eq!(sanitize(&"a".repeat(8192)).map(|s| s.len()), Ok(8192));
        assert_eq!(sanitize(&"a".repeat(8193)), Err(SendError::TooLarge));
        // Limit counts after sanitising: control characters do not count.
        let padded = format!("{}{}", "a".repeat(8192), "\r".repeat(100));
        assert_eq!(sanitize(&padded).map(|s| s.len()), Ok(8192));
    }

    #[test]
    fn sanitize_limit_multibyte() {
        // "é" is 2 bytes, "€" 3: 8190 + 2 = 8192 fits, 8190 + 3 = 8193 does not.
        let base = "a".repeat(8190);
        assert_eq!(sanitize(&format!("{base}é")).map(|s| s.len()), Ok(8192));
        assert_eq!(sanitize(&format!("{base}€")), Err(SendError::TooLarge));
        assert_eq!(
            sanitize(&"€".repeat(2730)).map(|s| s.len()),
            Ok(8190) // 2731 × 3 = 8193
        );
        assert_eq!(sanitize(&"€".repeat(2731)), Err(SendError::TooLarge));
    }

    // ---- marker ----

    #[test]
    fn marker_shape() {
        let s = Sender {
            pane: "p2",
            agent: "codex",
            title: Some("review: auth"),
        };
        assert_eq!(
            marker(&s),
            r#"[agentdeck] message from p2 (codex, "review: auth"). Reply with pane_send to "p2"."#
        );
        let s = Sender { title: None, ..s };
        assert_eq!(
            marker(&s),
            r#"[agentdeck] message from p2 (codex). Reply with pane_send to "p2"."#
        );
    }

    #[test]
    fn marker_cuts_long_title_on_utf8_boundary() {
        for unit in ["x", "é", "€", "😀"] {
            let title = unit.repeat(300);
            let s = Sender {
                title: Some(&title),
                ..ME
            };
            let m = marker(&s);
            assert!(m.len() <= MARKER_MAX_BYTES, "{unit}: {}", m.len());
            assert!(
                m.len() > MARKER_MAX_BYTES - unit.len(),
                "{unit}: budget used"
            );
            assert!(m.ends_with(r#""). Reply with pane_send to "p1"."#));
        }
    }

    #[test]
    fn marker_strips_control_and_caps_agent() {
        let agent = "a".repeat(100);
        let s = Sender {
            pane: "p1",
            agent: &agent,
            title: Some("t\x1b[201~\nx"),
        };
        let m = marker(&s);
        assert!(!m.chars().any(char::is_control));
        assert!(m.contains(&format!("({}, ", "a".repeat(32))));
        assert!(m.contains(r#""t[201~x""#));
    }

    // ---- paste ----

    #[test]
    fn paste_action_order() {
        let a = paste_actions("M", "b1\nb2");
        assert_eq!(
            a,
            vec![
                Action::Write(b"\x1b[200~M\nb1\nb2\x1b[201~".to_vec()),
                Action::Sleep(Duration::from_millis(150)),
                Action::Write(b"\r".to_vec()),
            ]
        );
    }

    // ---- ready, fake clock ----

    fn idle(t0: Instant) -> Target {
        Target {
            alive: true,
            status: PaneStatus::Idle,
            last_output: t0,
            last_typed: None,
            input: InputState::Empty,
            bracketed_paste: true,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn ready_when_idle() {
        let t0 = Instant::now();
        let tm = Timing::default();
        assert_eq!(ready(&idle(t0), false, &tm, t0), Readiness::Ready);
    }

    #[test]
    fn ready_exited_drops() {
        let t0 = Instant::now();
        let t = Target {
            alive: false,
            ..idle(t0)
        };
        assert_eq!(
            ready(&t, true, &Timing::default(), t0),
            Readiness::Drop(DropReason::TargetExited)
        );
    }

    #[test]
    fn ready_paused_waits() {
        let t0 = Instant::now();
        assert_eq!(
            ready(&idle(t0), true, &Timing::default(), t0),
            Readiness::Wait(WaitReason::Paused)
        );
    }

    #[test]
    fn ready_busy_waits() {
        let t0 = Instant::now();
        let t = Target {
            status: PaneStatus::Busy,
            ..idle(t0)
        };
        let late = t0 + Duration::from_secs(60);
        assert_eq!(
            ready(&t, false, &Timing::default(), late),
            Readiness::Wait(WaitReason::Busy)
        );
    }

    #[test]
    fn ready_unknown_needs_quiet() {
        let t0 = Instant::now();
        let t = Target {
            status: PaneStatus::Unknown,
            ..idle(t0)
        };
        let tm = Timing::default();
        assert_eq!(
            ready(&t, false, &tm, t0 + ms(1499)),
            Readiness::Wait(WaitReason::NotQuiet)
        );
        assert_eq!(ready(&t, false, &tm, t0 + ms(1500)), Readiness::Ready);
        let tm = Timing {
            quiet: ms(300),
            ..tm
        };
        assert_eq!(ready(&t, false, &tm, t0 + ms(300)), Readiness::Ready);
    }

    #[test]
    fn ready_waits_for_typing() {
        let t0 = Instant::now();
        let t = Target {
            last_typed: Some(t0),
            ..idle(t0)
        };
        let tm = Timing::default();
        assert_eq!(
            ready(&t, false, &tm, t0 + ms(1999)),
            Readiness::Wait(WaitReason::Typing)
        );
        assert_eq!(ready(&t, false, &tm, t0 + ms(2000)), Readiness::Ready);
        let tm = Timing {
            typing: ms(500),
            ..tm
        };
        assert_eq!(ready(&t, false, &tm, t0 + ms(500)), Readiness::Ready);
    }

    #[test]
    fn ready_needs_empty_input() {
        let t0 = Instant::now();
        for (input, why) in [
            (InputState::HasText, WaitReason::InputNotEmpty),
            (InputState::Unknown, WaitReason::InputUnknown),
        ] {
            let t = Target { input, ..idle(t0) };
            assert_eq!(
                ready(&t, false, &Timing::default(), t0 + Duration::from_secs(60)),
                Readiness::Wait(why)
            );
        }
    }

    #[test]
    fn ready_no_bracketed_paste_drops_only_when_otherwise_ready() {
        let t0 = Instant::now();
        let t = Target {
            bracketed_paste: false,
            ..idle(t0)
        };
        let tm = Timing::default();
        assert_eq!(
            ready(&t, false, &tm, t0),
            Readiness::Drop(DropReason::NoBracketedPaste)
        );
        let busy = Target {
            status: PaneStatus::Busy,
            ..t
        };
        assert_eq!(
            ready(&busy, false, &tm, t0),
            Readiness::Wait(WaitReason::Busy)
        );
    }

    // ---- queue ----

    #[test]
    fn fifo_ids_and_queue_full() {
        let mut d = Delivery::default();
        let g = LoopGuard::default();
        for i in 1..=5 {
            assert_eq!(
                d.enqueue(&ME, "p2", &format!("msg {i}"), &g),
                Ok((format!("m{i}"), State::Queued))
            );
        }
        assert_eq!(d.enqueue(&ME, "p2", "six", &g), Err(SendError::QueueFull));
        assert_eq!(SendError::QueueFull.code(), "QUEUE_FULL");
        // Other targets have their own queue; ids stay global.
        assert_eq!(
            d.enqueue(&ME, "p3", "x", &g),
            Ok(("m6".into(), State::Queued))
        );
        assert_eq!(d.enqueue(&ME, "p3", "", &g), Err(SendError::Empty));
        assert_eq!(
            d.enqueue(&ME, "p3", &"a".repeat(8193), &g),
            Err(SendError::TooLarge)
        );

        // Delivered in order.
        let mut g = LoopGuard::default();
        let t0 = Instant::now();
        let mut now = t0;
        for i in 1..=5 {
            now += ms(1500);
            let target = idle(t0);
            match d.poll("p2", &target, &mut g, 12, now) {
                Poll::Deliver { id, actions } => {
                    assert_eq!(id, format!("m{i}"));
                    let Action::Write(paste) = &actions[0] else {
                        panic!("first action writes the paste")
                    };
                    let paste = String::from_utf8_lossy(paste);
                    assert!(paste.ends_with(&format!("\nmsg {i}\x1b[201~")));
                    assert!(paste.contains("[agentdeck] message from p1 (claude, \"main\")"));
                }
                other => panic!("{i}: {other:?}"),
            }
            // A slot freed: one more fits again.
            if i == 1 {
                assert!(d.enqueue(&ME, "p2", "late", &g).is_ok());
                assert_eq!(d.pending("p2"), 5);
            }
        }
        assert_eq!(g.count(), 5);
    }

    #[test]
    fn next_message_waits_quiet_after_delivery() {
        let mut d = Delivery::default();
        let mut g = LoopGuard::default();
        d.enqueue(&ME, "p2", "a", &g).unwrap();
        d.enqueue(&ME, "p2", "b", &g).unwrap();
        let t0 = Instant::now();
        let t = Target {
            status: PaneStatus::Unknown,
            ..idle(t0)
        };
        let at = t0 + ms(1500);
        assert!(matches!(
            d.poll("p2", &t, &mut g, 12, at),
            Poll::Deliver { .. }
        ));
        assert_eq!(
            d.poll("p2", &t, &mut g, 12, at + ms(1499)),
            Poll::Wait(WaitReason::NotQuiet)
        );
        assert!(matches!(
            d.poll("p2", &t, &mut g, 12, at + ms(1500)),
            Poll::Deliver { .. }
        ));
        assert_eq!(d.poll("p2", &t, &mut g, 12, at + ms(9999)), Poll::Empty);
    }

    #[test]
    fn loop_guard_pauses_queue() {
        let mut d = Delivery::default();
        let mut g = LoopGuard::default();
        let t0 = Instant::now();
        let t = idle(t0);
        d.enqueue(&ME, "p2", "a", &g).unwrap();
        d.enqueue(&ME, "p2", "b", &g).unwrap();
        // Limit 1: the first delivery pauses the workspace.
        assert!(matches!(
            d.poll("p2", &t, &mut g, 1, t0),
            Poll::Deliver { .. }
        ));
        assert!(g.is_paused());
        let later = t0 + Duration::from_secs(10);
        assert_eq!(
            d.poll("p2", &t, &mut g, 1, later),
            Poll::Wait(WaitReason::Paused)
        );
        assert_eq!(
            d.enqueue(&ME, "p2", "c", &g),
            Ok(("m3".into(), State::Paused))
        );
        assert_eq!(State::Paused.as_str(), "paused");
        // "Lanjutkan": delivery resumes.
        g.reset();
        assert!(matches!(
            d.poll("p2", &t, &mut g, 12, later),
            Poll::Deliver { id, .. } if id == "m2"
        ));
        // "Hentikan": the rest is dropped.
        assert_eq!(
            d.drop_all("p2", DropReason::Stopped),
            Poll::Dropped(vec![("m3".into(), DropReason::Stopped)])
        );
        assert_eq!(d.pending("p2"), 0);
    }

    #[test]
    fn exit_drops_all_waiting() {
        let mut d = Delivery::default();
        let mut g = LoopGuard::default();
        for t in ["a", "b", "c"] {
            d.enqueue(&ME, "p2", t, &g).unwrap();
        }
        let t0 = Instant::now();
        let t = Target {
            alive: false,
            ..idle(t0)
        };
        assert_eq!(
            d.poll("p2", &t, &mut g, 12, t0),
            Poll::Dropped(
                ["m1", "m2", "m3"]
                    .map(|id| (id.to_string(), DropReason::TargetExited))
                    .to_vec()
            )
        );
        assert_eq!(d.pending("p2"), 0);
        assert_eq!(d.poll("p2", &t, &mut g, 12, t0), Poll::Empty);
        assert_eq!(g.count(), 0);
    }

    #[test]
    fn no_bracketed_paste_drops_head_with_reason() {
        let mut d = Delivery::default();
        let mut g = LoopGuard::default();
        d.enqueue(&ME, "p2", "a", &g).unwrap();
        d.enqueue(&ME, "p2", "b", &g).unwrap();
        let t0 = Instant::now();
        let t = Target {
            bracketed_paste: false,
            ..idle(t0)
        };
        assert_eq!(
            d.poll("p2", &t, &mut g, 12, t0),
            Poll::Dropped(vec![("m1".into(), DropReason::NoBracketedPaste)])
        );
        assert_eq!(d.pending("p2"), 1);
        assert_eq!(g.count(), 0, "a drop is not a delivery");
        assert_eq!(
            State::Dropped(DropReason::NoBracketedPaste).as_str(),
            "dropped"
        );
        assert!(!DropReason::NoBracketedPaste.as_str().is_empty());
    }

    #[test]
    fn error_text_starts_with_code() {
        for e in [SendError::Empty, SendError::TooLarge, SendError::QueueFull] {
            assert!(e.to_string().starts_with(&format!("{}: ", e.code())));
        }
    }
}
