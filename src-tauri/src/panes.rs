//! Pane registry (ARCHITECTURE §2–§4, §5.4): the core's one source of truth for which panes exist. Per pane: id `pN`
//! (given here, never reused), workspace, agent or shell, title, cwd, parent, PTY, token (agent panes only, revoked
//! when the process exits or the pane closes), headless screen, last official status. The MCP server maps tokens to
//! panes here; `list` is what `pane_list` returns. Per workspace: the peer switch and the loop guard. Messages
//! (queues, records, loop guard) live under the same lock, so a relation check, an enqueue, and a pause never race.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use crate::catalogue::{self, Catalogue};
use crate::delivery::{
    self, Action, Delivery, DropReason, PaneStatus, Poll, Sender, State as MsgState, Target,
};
use crate::launch::{self, LaunchError, Mcp};
use crate::mcp::server::McpStatus;
use crate::policy::{
    self, Limit, Limits, LoopGuard, PaneKind, PolicyError, READS_PER_MINUTE, Relation,
};
use crate::pty::{self, OnEvent, PaneEvent, PaneManager};
use crate::screen::Screen;
use crate::tokens::{Token, TokenRegistry};

/// Same as the WebView's `STARTING_MS`: an agent that has not reported by then is `unknown`.
const STARTING: Duration = Duration::from_secs(5);
/// `pane_read` rate window (MCP-TOOLS: 30 per minute per caller).
const READ_WINDOW: Duration = Duration::from_secs(60);

pub type Panes = Arc<Mutex<Registry>>;
pub type SharedScreen = Arc<Mutex<Screen>>;

/// A poisoned lock only means another thread panicked mid-update; every update leaves the registry consistent.
pub fn lock(panes: &Panes) -> MutexGuard<'_, Registry> {
    panes.lock().unwrap_or_else(|e| e.into_inner())
}

struct Entry {
    id: String,
    workspace: u32,
    /// Catalogue id; `None` for a shell.
    agent: Option<String>,
    title: String,
    #[allow(dead_code)] // read by pane_spawn (T3.2): a child's cwd must be inside its parent's
    cwd: PathBuf,
    /// Set by `pane_spawn` (T3.2); `None` when the person opened the pane.
    parent: Option<String>,
    pty: Option<u32>,
    /// Bumped on every (re)start, so events of an earlier process are ignored.
    generation: u64,
    screen: Option<Arc<Mutex<Screen>>>,
    /// Last official status (OSC 7501) by its MCP-TOOLS name; `None` before the first report or after `clear`.
    status: Option<&'static str>,
    started: Instant,
    exited: bool,
}

impl Entry {
    fn status(&self, now: Instant) -> &'static str {
        match self.status {
            _ if self.exited => "exited",
            Some(s) => s,
            None if self.agent.is_some() && now.duration_since(self.started) < STARTING => {
                "starting"
            }
            None => "unknown",
        }
    }

    fn policy(&self) -> policy::Pane {
        policy::Pane {
            id: self.id.clone(),
            workspace: self.workspace.to_string(),
            kind: match self.agent {
                Some(_) => PaneKind::Agent,
                None => PaneKind::Shell,
            },
            parent: self.parent.clone(),
            exited: self.exited,
        }
    }
}

/// What the UI asks for (and, later, `pane_spawn`).
#[derive(Debug, Clone, Deserialize)]
pub struct Spec {
    /// Set by `pane_spawn` (T3.2) and tests; never from the UI.
    #[serde(skip)]
    pub parent: Option<String>,
    pub workspace: u32,
    /// Run again in this existing pane (same id, new process and token) instead of opening a new one.
    pub pane: Option<String>,
    /// Catalogue id; `None` for a shell.
    pub agent: Option<String>,
    pub title: String,
    pub cwd: String,
    pub task: Option<String>,
    pub rows: u16,
    pub cols: u16,
}

/// Result of [`Registry::open`]: what starting the process needs.
pub struct Opened {
    pub id: String,
    pub generation: u64,
    pub token: Option<Token>,
    pub screen: Option<Arc<Mutex<Screen>>>,
    /// PTY of the process this restart replaces.
    pub old_pty: Option<u32>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PaneInfo {
    pub id: String,
    pub title: String,
    pub agent: Option<String>,
    pub status: &'static str,
    pub relation: &'static str,
    pub can_send: bool,
    pub can_read: bool,
}

/// `pane_list` result (MCP-TOOLS).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PaneList {
    pub you: String,
    pub panes: Vec<PaneInfo>,
    pub peer_messaging: bool,
}

#[derive(Debug, Default)]
struct Workspace {
    /// "Agen boleh saling kirim", off by default (MSG-06).
    peer: bool,
    guard: LoopGuard,
}

/// One agent message as the Messages panel and the log see it. `text` is the sanitised body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub id: String,
    /// Enqueue time, ms since the epoch.
    pub ts: u64,
    pub workspace: String,
    pub from: String,
    pub to: String,
    pub state: MsgState,
    pub text: String,
}

/// UI shape of a [`Record`] (`messages_list`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageRow {
    pub id: String,
    pub ts: u64,
    pub workspace: String,
    pub from: String,
    pub to: String,
    pub state: &'static str,
    pub reason: Option<&'static str>,
    pub text: String,
}

impl Record {
    pub fn row(&self) -> MessageRow {
        MessageRow {
            id: self.id.clone(),
            ts: self.ts,
            workspace: self.workspace.clone(),
            from: self.from.clone(),
            to: self.to.clone(),
            state: self.state.as_str(),
            reason: match self.state {
                MsgState::Dropped(r) => Some(r.as_str()),
                _ => None,
            },
            text: self.text.clone(),
        }
    }
}

/// Loop guard of a workspace for the UI (`loop_guard_get`, event `agentdeck://loop-guard`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GuardInfo {
    pub workspace: String,
    pub count: u32,
    pub limit: u32,
    pub paused: bool,
}

/// Result of [`Registry::send`].
#[derive(Debug)]
pub struct Sent {
    pub record: Record,
    /// No delivery worker runs for the target yet: the caller starts one.
    pub start_worker: bool,
}

/// Result of [`Registry::poll`], one step of a target's delivery worker.
#[derive(Debug)]
pub enum Step {
    Empty,
    Wait,
    /// Write `actions` to `pty`, then publish `changed` (the delivered record first) and `guard`.
    Deliver {
        id: String,
        pty: u32,
        actions: Vec<Action>,
        changed: Vec<Record>,
        guard: GuardInfo,
        /// This delivery reached the limit: the workspace is now paused (audit line).
        paused_now: bool,
    },
    Dropped(Vec<Record>),
}

#[derive(Debug, Default)]
pub struct Registry {
    workspaces: HashMap<u32, Workspace>,
    // ponytail: linear scans in creation order, fine for the few dozen panes an app holds.
    panes: Vec<Entry>,
    tokens: TokenRegistry,
    next_pane: u32,
    next_workspace: u32,
    /// Configurable limits; settings (T3.3) will set them. Only the loop guard is read here.
    pub limits: Limits,
    delivery: Delivery,
    // ponytail: every message of the run stays in memory for the panel; cap it if runs ever get that long.
    messages: Vec<Record>,
    /// Targets whose delivery worker is running.
    workers: HashSet<String>,
    /// `pane_read` call times per caller, for READ_RATE.
    reads: HashMap<String, VecDeque<Instant>>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

impl Registry {
    pub fn create_workspace(&mut self) -> u32 {
        self.next_workspace += 1;
        self.workspaces
            .insert(self.next_workspace, Workspace::default());
        self.next_workspace
    }

    /// The UI closes a workspace after its last pane; a pane left behind keeps its id, peer messaging falls to off.
    pub fn close_workspace(&mut self, ws: u32) {
        self.workspaces.remove(&ws);
    }

    fn entry(&mut self, id: &str) -> Option<&mut Entry> {
        self.panes.iter_mut().find(|e| e.id == id)
    }

    /// Registers a new pane, or prepares `spec.pane` for a restart, and issues an agent pane its token.
    pub fn open(&mut self, spec: &Spec) -> Result<Opened, LaunchError> {
        let now = Instant::now();
        let screen = spec
            .agent
            .is_some()
            .then(|| Arc::new(Mutex::new(Screen::new(spec.rows, spec.cols))));
        let (id, generation, old_pty) = match &spec.pane {
            Some(id) => {
                let e = self
                    .entry(id)
                    .ok_or_else(|| LaunchError::new(format!("Pane {id} sudah ditutup.")))?;
                e.generation += 1;
                e.agent = spec.agent.clone();
                e.title = spec.title.clone();
                e.cwd = spec.cwd.clone().into();
                e.screen = screen.clone();
                e.status = None;
                e.started = now;
                e.exited = false;
                (id.clone(), e.generation, e.pty.take())
            }
            None => {
                if !self.workspaces.contains_key(&spec.workspace) {
                    return Err(LaunchError::new("Workspace sudah ditutup."));
                }
                self.next_pane += 1;
                let id = format!("p{}", self.next_pane);
                self.panes.push(Entry {
                    id: id.clone(),
                    workspace: spec.workspace,
                    agent: spec.agent.clone(),
                    title: spec.title.clone(),
                    cwd: spec.cwd.clone().into(),
                    parent: spec.parent.clone(),
                    pty: None,
                    generation: 0,
                    screen: screen.clone(),
                    status: None,
                    started: now,
                    exited: false,
                });
                (id, 0, None)
            }
        };
        self.tokens.revoke(&id);
        let token = match spec.agent {
            Some(_) => match self.tokens.issue(&id) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.failed(&id, generation, spec.pane.is_none());
                    return Err(LaunchError::new(format!(
                        "Token pane tidak bisa dibuat: {e}"
                    )));
                }
            },
            None => None,
        };
        Ok(Opened {
            id,
            generation,
            token,
            screen,
            old_pty,
        })
    }

    /// The process did not start: a new pane goes away, a restarted one stays as exited. The token goes either way.
    fn failed(&mut self, id: &str, generation: u64, new: bool) {
        if !self.entry(id).is_some_and(|e| e.generation == generation) {
            return;
        }
        self.tokens.revoke(id);
        if new {
            self.panes.retain(|e| e.id != id);
        } else if let Some(e) = self.entry(id) {
            e.exited = true;
        }
    }

    /// Records the PTY; false when the pane was closed or restarted meanwhile (the caller then stops the process).
    fn started(&mut self, id: &str, generation: u64, pty: u32) -> bool {
        match self.entry(id) {
            Some(e) if e.generation == generation => {
                e.pty = Some(pty);
                true
            }
            _ => false,
        }
    }

    /// Status and exit from the pane's reader thread. Exit revokes the token (ARCHITECTURE §3 step 4).
    pub fn event(&mut self, id: &str, generation: u64, ev: &PaneEvent) {
        let Some(e) = self.entry(id).filter(|e| e.generation == generation) else {
            return;
        };
        match ev {
            PaneEvent::Status { status, .. } => {
                e.status = (*status != "unknown").then_some(*status)
            }
            PaneEvent::Exited { .. } => {
                e.exited = true;
                self.tokens.revoke(id);
            }
        }
    }

    /// Removes the pane (only the person closes panes) and revokes its token. Returns its PTY to stop.
    pub fn close(&mut self, id: &str) -> Option<u32> {
        self.tokens.revoke(id);
        let i = self.panes.iter().position(|e| e.id == id)?;
        self.panes.remove(i).pty
    }

    /// Pane id owning `token`; `None` once the pane exited or closed.
    pub fn resolve(&self, token: &str) -> Option<String> {
        self.tokens.resolve(token).map(str::to_owned)
    }

    pub fn screen(&self, id: &str) -> Option<Arc<Mutex<Screen>>> {
        self.panes.iter().find(|e| e.id == id)?.screen.clone()
    }

    /// `pane_list` for `caller`: the panes of its workspace with relation and permissions from `policy`.
    pub fn list(&self, caller: &str) -> Option<PaneList> {
        let all: Vec<policy::Pane> = self.panes.iter().map(Entry::policy).collect();
        let (me, me_entry) = all.iter().zip(&self.panes).find(|(p, _)| p.id == caller)?;
        let peer = self.peer(me_entry.workspace);
        let now = Instant::now();
        let panes = all
            .iter()
            .zip(&self.panes)
            .filter_map(|(p, e)| {
                let relation = policy::relation(me, p);
                (relation != Relation::OtherWorkspace).then(|| PaneInfo {
                    id: e.id.clone(),
                    title: e.title.clone(),
                    agent: e.agent.clone(),
                    status: e.status(now),
                    relation: relation.as_str(),
                    can_send: policy::can_send(me, p, peer).is_ok(),
                    can_read: policy::can_read(me, p, peer).is_ok(),
                })
            })
            .collect();
        Some(PaneList {
            you: caller.to_owned(),
            panes,
            peer_messaging: peer,
        })
    }

    /// "Agen boleh saling kirim" of `ws`; a closed workspace counts as off.
    pub fn peer(&self, ws: u32) -> bool {
        self.workspaces.get(&ws).is_some_and(|w| w.peer)
    }

    /// False when the workspace does not exist.
    pub fn set_peer(&mut self, ws: u32, on: bool) -> bool {
        self.workspaces.get_mut(&ws).map(|w| w.peer = on).is_some()
    }

    fn guard_info(&self, ws: u32) -> GuardInfo {
        let (count, paused) = self
            .workspaces
            .get(&ws)
            .map_or((0, false), |w| (w.guard.count(), w.guard.is_paused()));
        GuardInfo {
            workspace: ws.to_string(),
            count,
            limit: self.limits.get(Limit::LoopGuard),
            paused,
        }
    }

    pub fn guard(&self, ws: u32) -> Option<GuardInfo> {
        self.workspaces
            .contains_key(&ws)
            .then(|| self.guard_info(ws))
    }

    pub fn messages(&self, ws: u32) -> Vec<MessageRow> {
        let ws = ws.to_string();
        self.messages
            .iter()
            .filter(|r| r.workspace == ws)
            .map(Record::row)
            .collect()
    }

    /// Sets a record's state; returns the updated copy for the log and the UI.
    fn mark(&mut self, id: &str, state: MsgState) -> Option<Record> {
        let r = self.messages.iter_mut().rev().find(|r| r.id == id)?;
        r.state = state;
        Some(r.clone())
    }

    /// Waiting messages of `ws` from `from` to `to` (paused while the loop guard holds them, queued otherwise).
    fn restate(&mut self, ws: u32, from: MsgState, to: MsgState) -> Vec<Record> {
        let ws = ws.to_string();
        self.messages
            .iter_mut()
            .filter(|r| r.workspace == ws && r.state == from)
            .map(|r| {
                r.state = to;
                r.clone()
            })
            .collect()
    }

    /// `pane_send` from `caller` (MCP-TOOLS): relation (§5.4), then text and queue limits, then enqueue. Errors are
    /// the agent-facing `CODE: ...` text.
    pub fn send(&mut self, caller: &str, to: &str, text: &str, ts: u64) -> Result<Sent, String> {
        let all: Vec<policy::Pane> = self.panes.iter().map(Entry::policy).collect();
        let (me, me_entry) = all
            .iter()
            .zip(&self.panes)
            .find(|(p, _)| p.id == caller)
            .ok_or_else(|| PolicyError::UnknownPane.to_string())?;
        let target = policy::resolve_target(&all, me, to).map_err(|e| e.to_string())?;
        let ws = me_entry.workspace;
        policy::can_send(me, target, self.peer(ws)).map_err(|e| e.to_string())?;
        let body = delivery::sanitize(text).map_err(|e| e.to_string())?;
        let sender = Sender {
            pane: caller,
            agent: me_entry.agent.as_deref().unwrap_or("shell"),
            title: Some(&me_entry.title),
        };
        let spare = LoopGuard::default();
        let guard = self.workspaces.get(&ws).map_or(&spare, |w| &w.guard);
        let (id, state) = self
            .delivery
            .enqueue(&sender, &target.id, &body, guard)
            .map_err(|e| e.to_string())?;
        let record = Record {
            id,
            ts,
            workspace: ws.to_string(),
            from: caller.to_owned(),
            to: target.id.clone(),
            state,
            text: body,
        };
        self.messages.push(record.clone());
        let start_worker = self.workers.insert(target.id.clone());
        Ok(Sent {
            record,
            start_worker,
        })
    }

    /// The worker for `to` ends only when its queue is empty; checked under the lock `send` uses, so a message
    /// enqueued meanwhile is never left without a worker.
    pub fn worker_done(&mut self, to: &str) -> bool {
        let done = self.delivery.pending(to) == 0;
        if done {
            self.workers.remove(to);
        }
        done
    }

    /// One delivery step for `to` (§5.2): readiness from status, screen and `typed_at` (last real keystroke per
    /// PTY). A delivered target is set `working` at once, so the next message waits for its next `idle` (§5.1).
    pub fn poll(
        &mut self,
        to: &str,
        typed_at: impl Fn(u32) -> Option<Instant>,
        now: Instant,
    ) -> Step {
        let Some(i) = self.panes.iter().position(|e| e.id == to) else {
            let gone = self.delivery.drop_all(to, DropReason::TargetExited);
            return self.dropped(gone);
        };
        let e = &self.panes[i];
        let ws = e.workspace;
        let (input, last_output, bracketed_paste) = match &e.screen {
            Some(screen) => {
                let s = screen.lock().unwrap_or_else(|p| p.into_inner());
                (
                    s.input_state(e.agent.as_deref().unwrap_or_default()),
                    s.last_output(),
                    s.bracketed_paste(),
                )
            }
            None => (delivery::InputState::Unknown, None, false),
        };
        let target = Target {
            alive: !e.exited,
            status: match (e.pty, e.status(now)) {
                (None, _) => PaneStatus::Busy, // not running yet
                (_, "idle") => PaneStatus::Idle,
                (_, "unknown") => PaneStatus::Unknown,
                _ => PaneStatus::Busy,
            },
            last_output: last_output.unwrap_or(e.started),
            last_typed: e.pty.and_then(&typed_at),
            input,
            bracketed_paste,
        };
        let pty = e.pty;
        let limit = self.limits.get(Limit::LoopGuard);
        let mut spare = LoopGuard::default();
        let guard = self
            .workspaces
            .get_mut(&ws)
            .map_or(&mut spare, |w| &mut w.guard);
        let was_paused = guard.is_paused();
        match self.delivery.poll(to, &target, guard, limit, now) {
            Poll::Empty => Step::Empty,
            Poll::Wait(_) => Step::Wait,
            Poll::Dropped(gone) => self.dropped(Poll::Dropped(gone)),
            Poll::Deliver { id, actions } => {
                let paused_now = !was_paused && guard.is_paused();
                self.panes[i].status = Some("working");
                let mut changed: Vec<Record> =
                    self.mark(&id, MsgState::Delivered).into_iter().collect();
                if paused_now {
                    changed.extend(self.restate(ws, MsgState::Queued, MsgState::Paused));
                }
                Step::Deliver {
                    id,
                    // `Busy` above when `pty` is None, so a delivery always has one.
                    pty: pty.unwrap_or_default(),
                    actions,
                    changed,
                    guard: self.guard_info(ws),
                    paused_now,
                }
            }
        }
    }

    fn dropped(&mut self, poll: Poll) -> Step {
        match poll {
            Poll::Dropped(gone) => Step::Dropped(
                gone.into_iter()
                    .filter_map(|(id, why)| self.mark(&id, MsgState::Dropped(why)))
                    .collect(),
            ),
            _ => Step::Empty,
        }
    }

    /// A message that was counted as delivered could not be written (the PTY went away meanwhile).
    pub fn undeliverable(&mut self, id: &str) -> Option<Record> {
        self.mark(id, MsgState::Dropped(DropReason::TargetExited))
    }

    /// Clears the loop guard of `ws` and releases what it held. `None` when there was nothing to clear.
    fn release(&mut self, ws: u32) -> Option<(GuardInfo, Vec<Record>)> {
        let w = self.workspaces.get_mut(&ws)?;
        if w.guard.count() == 0 && !w.guard.is_paused() {
            return None;
        }
        let was_paused = w.guard.is_paused();
        w.guard.reset();
        let changed = if was_paused {
            self.restate(ws, MsgState::Paused, MsgState::Queued)
        } else {
            Vec::new()
        };
        Some((self.guard_info(ws), changed))
    }

    /// The person pressed Enter in the pane on `pty` (§5.5): its workspace's loop guard starts over.
    pub fn human_enter(&mut self, pty: u32) -> Option<(GuardInfo, Vec<Record>)> {
        let ws = self.panes.iter().find(|e| e.pty == Some(pty))?.workspace;
        self.release(ws)
    }

    /// "Lanjutkan" on the loop-guard card.
    pub fn guard_continue(&mut self, ws: u32) -> Option<(GuardInfo, Vec<Record>)> {
        self.release(ws)
    }

    /// "Hentikan": every waiting message of `ws` is dropped and the guard starts over.
    pub fn guard_stop(&mut self, ws: u32) -> Option<(GuardInfo, Vec<Record>)> {
        self.workspaces.get_mut(&ws)?.guard.reset();
        let w = ws.to_string();
        let mut targets: Vec<String> = self
            .messages
            .iter()
            .filter(|r| r.workspace == w && matches!(r.state, MsgState::Queued | MsgState::Paused))
            .map(|r| r.to.clone())
            .collect();
        targets.sort();
        targets.dedup();
        let mut changed = Vec::new();
        for to in targets {
            let gone = self.delivery.drop_all(&to, DropReason::Stopped);
            if let Step::Dropped(r) = self.dropped(gone) {
                changed.extend(r);
            }
        }
        Some((self.guard_info(ws), changed))
    }

    /// "Buang" in the Messages panel. `None` when the message is no longer waiting.
    pub fn discard(&mut self, id: &str) -> Option<Record> {
        self.delivery.remove(id)?;
        self.mark(id, MsgState::Dropped(DropReason::Discarded))
    }

    /// `pane_read` permission and READ_RATE (30 calls per minute per caller, counted when accepted by the rate
    /// check). Returns the target's status and screen; the caller reads the screen outside this lock.
    pub fn read(
        &mut self,
        caller: &str,
        pane: &str,
        now: Instant,
    ) -> Result<(&'static str, Option<SharedScreen>), String> {
        let times = self.reads.entry(caller.to_owned()).or_default();
        while times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= READ_WINDOW)
        {
            times.pop_front();
        }
        if times.len() >= READS_PER_MINUTE as usize {
            let left = times.front().map_or(Duration::ZERO, |t| {
                (*t + READ_WINDOW).saturating_duration_since(now)
            });
            let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
            return Err(format!(
                "READ_RATE: more than {READS_PER_MINUTE} pane_read calls in a minute. Wait {secs} seconds; \
                 the person sees the screen too, so you rarely need to poll it."
            ));
        }
        times.push_back(now);
        let all: Vec<policy::Pane> = self.panes.iter().map(Entry::policy).collect();
        let (me, me_entry) = all
            .iter()
            .zip(&self.panes)
            .find(|(p, _)| p.id == caller)
            .ok_or_else(|| PolicyError::UnknownPane.to_string())?;
        let (target, entry) = all
            .iter()
            .zip(&self.panes)
            .find(|(p, _)| p.id == pane)
            .ok_or_else(|| PolicyError::UnknownPane.to_string())?;
        policy::can_read(me, target, self.peer(me_entry.workspace)).map_err(|e| e.to_string())?;
        Ok((entry.status(now), entry.screen.clone()))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Started {
    pub pane: String,
    pub pty: u32,
}

/// Stops a PTY in the background; a pane being restarted has usually exited already, so this returns at once.
fn stop(pm: &PaneManager, pty: u32) {
    if let Ok(p) = pm.remove(pty) {
        std::thread::spawn(move || pty::terminate(&p));
    }
}

/// Opens (or restarts) a pane and runs its program with MCP injected when `mcp_url` is set: the one path behind
/// `pane_create`. `env` is the from-scratch base env (`launch::env`).
pub fn start(
    panes: &Panes,
    pm: &PaneManager,
    mcp_url: Option<&str>,
    env: Vec<(String, OsString)>,
    spec: &Spec,
    on_event: OnEvent,
) -> Result<Started, LaunchError> {
    let dir = Path::new(&spec.cwd);
    if !dir.is_dir() {
        return Err(LaunchError::new(format!(
            "Folder tidak ditemukan: {}",
            spec.cwd
        )));
    }
    let opened = lock(panes).open(spec)?;
    if let Some(old) = opened.old_pty {
        stop(pm, old);
    }
    let (id, generation) = (opened.id.clone(), opened.generation);
    let fail = |e: LaunchError| {
        lock(panes).failed(&id, generation, spec.pane.is_none());
        e
    };
    let launch = match &spec.agent {
        None => launch::shell(env),
        Some(agent) => {
            let cat = Catalogue::builtin().map_err(|e| fail(LaunchError::new(e.to_string())))?;
            let mcp = mcp_url.zip(opened.token.as_ref()).map(|(url, token)| Mcp {
                url,
                token,
                pane: &id,
            });
            launch::agent(&cat, agent, spec.task.as_deref(), env, mcp).map_err(fail)?
        }
    };
    let forward: OnEvent = {
        let panes = panes.clone();
        let id = id.clone();
        Box::new(move |ev| {
            lock(&panes).event(&id, generation, &ev);
            on_event(ev);
        })
    };
    let pty = pm
        .spawn(&launch, dir, (spec.rows, spec.cols), opened.screen, forward)
        .map_err(|e| {
            let prog = launch.argv.first().map(String::as_str).unwrap_or_default();
            let name = prog.rsplit('/').next().unwrap_or(prog);
            fail(LaunchError::new(format!(
                "Tidak bisa menjalankan {name}: {e}"
            )))
        })?;
    if !lock(panes).started(&id, generation, pty) {
        stop(pm, pty);
        return Err(LaunchError::new(format!(
            "Pane {id} ditutup saat dijalankan."
        )));
    }
    Ok(Started { pane: id, pty })
}

/// Shell (`agent` None) or a catalogue agent, new (`pane` None) or run again in an existing pane. The core gives the
/// pane id; agents get MCP injected while the server runs.
#[tauri::command]
pub async fn pane_create(
    panes: State<'_, Panes>,
    pm: State<'_, PaneManager>,
    mcp: State<'_, McpStatus>,
    spec: Spec,
    on_event: Channel<PaneEvent>,
) -> Result<Started, LaunchError> {
    let url = match *mcp {
        McpStatus::Running { port } => Some(format!("http://127.0.0.1:{port}/mcp")),
        McpStatus::Off { .. } => None,
    };
    let env = launch::env(|k| std::env::var_os(k), catalogue::login_path());
    let on_event: OnEvent = Box::new(move |ev| {
        let _ = on_event.send(ev);
    });
    start(&panes, &pm, url.as_deref(), env, &spec, on_event)
}

/// Closes the pane: token revoked first, then the process group stopped. Unknown pane (failed launch) is a no-op.
#[tauri::command]
pub async fn pane_close(
    panes: State<'_, Panes>,
    pm: State<'_, PaneManager>,
    pane: String,
) -> Result<(), String> {
    let Some(p) = lock(&panes)
        .close(&pane)
        .and_then(|pty| pm.remove(pty).ok())
    else {
        return Ok(());
    };
    tauri::async_runtime::spawn_blocking(move || pty::terminate(&p))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn workspace_create(panes: State<'_, Panes>) -> Result<u32, String> {
    Ok(lock(&panes).create_workspace())
}

#[tauri::command]
pub async fn workspace_close(panes: State<'_, Panes>, id: u32) -> Result<(), String> {
    lock(&panes).close_workspace(id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(ws: u32, agent: Option<&str>) -> Spec {
        Spec {
            parent: None,
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

    fn exited() -> PaneEvent {
        PaneEvent::Exited { code: Some(0) }
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut r = Registry::default();
        let ws = r.create_workspace();
        let ids: Vec<String> = (0..3)
            .map(|_| r.open(&spec(ws, None)).unwrap().id)
            .collect();
        assert_eq!(ids, ["p1", "p2", "p3"]);
        r.close("p3");
        assert_eq!(r.open(&spec(ws, None)).unwrap().id, "p4");
        // A launch that fails leaves a gap, not a reusable id.
        let o = r.open(&spec(ws, Some("codex"))).unwrap();
        r.failed(&o.id, o.generation, true);
        assert_eq!(r.open(&spec(ws, None)).unwrap().id, "p6");
        // Run again keeps the id.
        let again = Spec {
            pane: Some("p2".into()),
            ..spec(ws, Some("codex"))
        };
        assert_eq!(r.open(&again).unwrap().id, "p2");
        assert!(r.open(&spec(99, None)).is_err(), "closed workspace");
        assert!(
            r.open(&Spec {
                pane: Some("p3".into()),
                ..spec(ws, None)
            })
            .is_err(),
            "closed pane"
        );
    }

    #[test]
    fn token_only_for_agents_and_revoked_on_exit_close_and_restart() {
        let mut r = Registry::default();
        let ws = r.create_workspace();
        assert!(r.open(&spec(ws, None)).unwrap().token.is_none());

        let a = r.open(&spec(ws, Some("claude-code"))).unwrap();
        let t = a.token.unwrap();
        assert_eq!(r.resolve(t.expose()).as_deref(), Some("p2"));
        assert!(r.started("p2", a.generation, 7));
        r.event("p2", a.generation, &exited());
        assert_eq!(r.resolve(t.expose()), None, "exit revokes");
        assert_eq!(r.list("p1").unwrap().panes[1].status, "exited");

        // Run again: new token; the old process's late exit must not revoke it.
        let again = Spec {
            pane: Some("p2".into()),
            ..spec(ws, Some("claude-code"))
        };
        let b = r.open(&again).unwrap();
        assert_eq!(b.old_pty, Some(7));
        let t2 = b.token.unwrap();
        r.event("p2", a.generation, &exited());
        assert_eq!(r.resolve(t2.expose()).as_deref(), Some("p2"));
        // A third run revokes the second token even before that process exits.
        let c = r.open(&again).unwrap();
        assert_eq!(r.resolve(t2.expose()), None);
        let t3 = c.token.unwrap();
        assert!(!r.started("p2", b.generation, 8), "stale start");
        assert!(r.started("p2", c.generation, 9));
        assert_eq!(r.close("p2"), Some(9));
        assert_eq!(r.resolve(t3.expose()), None, "close revokes");
        assert_eq!(r.close("p2"), None);

        let d = r.open(&spec(ws, Some("codex"))).unwrap();
        let t4 = d.token.unwrap();
        r.failed(&d.id, d.generation, true);
        assert_eq!(r.resolve(t4.expose()), None, "failed launch revokes");
        assert!(!format!("{r:?}").contains(t4.expose()));
    }

    #[test]
    fn status_follows_reports() {
        let mut r = Registry::default();
        let ws = r.create_workspace();
        let a = r.open(&spec(ws, Some("codex"))).unwrap();
        r.open(&spec(ws, None)).unwrap();
        let status = |r: &Registry| -> Vec<&'static str> {
            r.list("p1")
                .unwrap()
                .panes
                .iter()
                .map(|p| p.status)
                .collect()
        };
        assert_eq!(status(&r), ["starting", "unknown"]);
        let report = |s| PaneEvent::Status {
            status: s,
            kind: None,
            msg: None,
            error: false,
        };
        r.event("p1", a.generation, &report("working"));
        assert_eq!(status(&r), ["working", "unknown"]);
        r.event("p1", a.generation, &report("unknown"));
        r.entry("p1").unwrap().started -= STARTING;
        assert_eq!(status(&r), ["unknown", "unknown"]);
    }

    #[test]
    fn list_has_relations_and_permissions_from_policy() {
        let mut r = Registry::default();
        let ws = r.create_workspace();
        let other = r.create_workspace();
        for agent in [Some("claude-code"), Some("codex"), None, Some("codex")] {
            r.open(&spec(ws, agent)).unwrap();
        }
        r.open(&spec(other, Some("codex"))).unwrap();
        r.entry("p2").unwrap().parent = Some("p1".into());
        let rows = |r: &Registry, caller| -> Vec<(String, &'static str, bool, bool)> {
            let l = r.list(caller).unwrap();
            assert_eq!(l.you, caller);
            l.panes
                .into_iter()
                .map(|p| (p.id, p.relation, p.can_send, p.can_read))
                .collect()
        };
        let row = |id: &str, rel, send, read| (id.to_owned(), rel, send, read);
        assert_eq!(
            rows(&r, "p1"),
            [
                row("p1", "you", false, false),
                row("p2", "child", true, true),
                row("p3", "shell", false, false),
                row("p4", "peer", false, false),
            ]
        );
        assert_eq!(rows(&r, "p2")[0], row("p1", "parent", true, true));
        assert_eq!(rows(&r, "p5"), [row("p5", "you", false, false)]);
        assert!(!r.list("p1").unwrap().peer_messaging);

        assert!(r.set_peer(ws, true));
        assert!(r.list("p1").unwrap().peer_messaging);
        assert_eq!(rows(&r, "p1")[3], row("p4", "peer", true, true));
        // An exited child can still be read, not sent to.
        r.event("p2", 0, &exited());
        assert_eq!(rows(&r, "p1")[1], row("p2", "child", false, true));
        assert_eq!(r.list("p9"), None);
        // A shell caller can do nothing (it has no token anyway).
        assert!(rows(&r, "p3").iter().all(|(_, _, s, rd)| !s && !rd));
        r.close_workspace(ws);
        assert!(!r.list("p1").unwrap().peer_messaging);
    }

    // ---- messages (T2.7) ----

    /// p1 claude-code parent, p2 codex child; p2 runs on PTY 7, is idle, and shows an empty prompt.
    fn pair() -> (Registry, u32) {
        let mut r = Registry::default();
        let ws = r.create_workspace();
        r.open(&spec(ws, Some("claude-code"))).unwrap();
        let o = r
            .open(&Spec {
                parent: Some("p1".into()),
                ..spec(ws, Some("codex"))
            })
            .unwrap();
        assert!(r.started("p2", o.generation, 7));
        r.screen("p2")
            .unwrap()
            .lock()
            .unwrap()
            .process(b"\x1b[?2004h\x1b[1m\xe2\x80\xba\x1b[0m \r\n\r\n\x1b[1;3H");
        r.event("p2", o.generation, &idle());
        (r, ws)
    }

    fn idle() -> PaneEvent {
        PaneEvent::Status {
            status: "idle",
            kind: None,
            msg: None,
            error: false,
        }
    }

    fn states(r: &Registry, ws: u32) -> Vec<&'static str> {
        r.messages(ws).iter().map(|m| m.state).collect()
    }

    fn no_typing(_: u32) -> Option<Instant> {
        None
    }

    #[test]
    fn delivers_to_idle_target_then_waits_for_next_idle() {
        let (mut r, ws) = pair();
        let sent = r.send("p1", "p2", "a\x07b", 1).unwrap();
        assert!(sent.start_worker);
        assert_eq!(sent.record.text, "ab", "sanitised");
        assert!(!r.send("p1", "p2", "second", 2).unwrap().start_worker);
        let now = Instant::now();
        let Step::Deliver {
            id,
            pty,
            actions,
            changed,
            guard,
            paused_now,
        } = r.poll("p2", no_typing, now)
        else {
            panic!("not delivered")
        };
        assert_eq!((id.as_str(), pty, paused_now), ("m1", 7, false));
        let Action::Write(paste) = &actions[0] else {
            panic!()
        };
        assert!(String::from_utf8_lossy(paste).starts_with(
            "\x1b[200~[agentdeck] message from p1 (claude-code, \"t\"). Reply with pane_send to \"p1\".\nab"
        ));
        assert_eq!(changed[0].state, MsgState::Delivered);
        assert_eq!((guard.count, guard.limit, guard.paused), (1, 12, false));
        assert_eq!(r.list("p1").unwrap().panes[1].status, "working");
        assert!(matches!(r.poll("p2", no_typing, now), Step::Wait));
        r.event("p2", 0, &idle());
        // The person typed 1 s ago: wait for typing_ms.
        let typed = |_| Some(now);
        assert!(matches!(
            r.poll("p2", typed, now + Duration::from_secs(1)),
            Step::Wait
        ));
        assert!(matches!(
            r.poll("p2", typed, now + Duration::from_secs(2)),
            Step::Deliver { .. }
        ));
        assert_eq!(states(&r, ws), ["delivered", "delivered"]);
        assert!(matches!(r.poll("p2", no_typing, now), Step::Empty));
        assert!(r.worker_done("p2"));
        assert!(r.send("p1", "p2", "third", 3).unwrap().start_worker);
    }

    #[test]
    fn waits_while_input_box_has_text() {
        let (mut r, _) = pair();
        r.send("p1", "p2", "x", 1).unwrap();
        let screen = r.screen("p2").unwrap();
        screen.lock().unwrap().process(b"draft");
        let later = Instant::now() + Duration::from_secs(60);
        assert!(matches!(r.poll("p2", no_typing, later), Step::Wait));
        screen.lock().unwrap().process(b"\x1b[1;3H\x1b[K");
        assert!(matches!(
            r.poll("p2", no_typing, later),
            Step::Deliver { .. }
        ));
    }

    #[test]
    fn loop_guard_pauses_workspace_and_is_released() {
        let (mut r, ws) = pair();
        r.limits.set(Limit::LoopGuard, 2).unwrap();
        let now = Instant::now();
        let deliver = |r: &mut Registry| {
            r.event("p2", 0, &idle());
            r.poll("p2", no_typing, now)
        };
        for t in ["a", "b", "c"] {
            r.send("p1", "p2", t, 1).unwrap();
        }
        assert!(matches!(
            deliver(&mut r),
            Step::Deliver {
                paused_now: false,
                ..
            }
        ));
        let Step::Deliver {
            paused_now: true,
            guard,
            changed,
            ..
        } = deliver(&mut r)
        else {
            panic!("second delivery pauses")
        };
        assert_eq!((guard.count, guard.paused), (2, true));
        assert_eq!(changed.last().map(|c| c.state), Some(MsgState::Paused));
        assert!(matches!(deliver(&mut r), Step::Wait));
        // New messages while paused report `paused`.
        assert_eq!(
            r.send("p1", "p2", "d", 1).unwrap().record.state,
            MsgState::Paused
        );
        assert_eq!(
            states(&r, ws),
            ["delivered", "delivered", "paused", "paused"]
        );
        // Enter typed by the person in any pane of the workspace (p2 is on PTY 7).
        let (g, changed) = r.human_enter(7).unwrap();
        assert_eq!((g.count, g.paused), (0, false));
        assert_eq!(changed.len(), 2);
        assert_eq!(
            states(&r, ws),
            ["delivered", "delivered", "queued", "queued"]
        );
        assert_eq!(r.human_enter(7), None, "nothing to clear");
        assert_eq!(r.human_enter(99), None, "unknown PTY");
        assert!(matches!(deliver(&mut r), Step::Deliver { .. }));
        assert!(matches!(
            deliver(&mut r),
            Step::Deliver {
                paused_now: true,
                ..
            }
        ));
        r.send("p1", "p2", "e", 1).unwrap();
        // "Lanjutkan" releases, then "Hentikan" drops what waits.
        assert!(!r.guard_continue(ws).unwrap().0.paused);
        r.limits.set(Limit::LoopGuard, 2).unwrap();
        let (g, changed) = r.guard_stop(ws).unwrap();
        assert_eq!((g.count, g.paused), (0, false));
        assert_eq!(changed.len(), 1);
        assert_eq!(
            r.messages(ws).last().unwrap().reason,
            Some("stopped by the person at the loop guard")
        );
        assert!(matches!(r.poll("p2", no_typing, now), Step::Empty));
        assert_eq!(r.guard(ws).unwrap().count, 0);
        assert_eq!(r.guard(99), None);
        assert_eq!(r.guard_stop(99), None);
    }

    #[test]
    fn discard_and_exit_drop_with_reason() {
        let (mut r, ws) = pair();
        for t in ["a", "b", "c"] {
            r.send("p1", "p2", t, 1).unwrap();
        }
        assert_eq!(
            r.discard("m2").unwrap().state,
            MsgState::Dropped(DropReason::Discarded)
        );
        assert_eq!(r.discard("m2"), None, "already gone");
        r.event("p2", 0, &PaneEvent::Exited { code: Some(1) });
        let Step::Dropped(gone) = r.poll("p2", no_typing, Instant::now()) else {
            panic!()
        };
        assert_eq!(gone.len(), 2);
        let rows = r.messages(ws);
        assert_eq!(
            rows.iter().map(|m| m.reason).collect::<Vec<_>>(),
            [
                Some("target agent exited"),
                Some("discarded by the person"),
                Some("target agent exited")
            ]
        );
        // A closed target drops too.
        r.send("p2", "parent", "x", 1).unwrap();
        r.close("p1");
        assert!(
            matches!(r.poll("p1", no_typing, Instant::now()), Step::Dropped(g) if g.len() == 1)
        );
    }

    #[test]
    fn read_rate_window_with_fake_clock() {
        let (mut r, _) = pair();
        let t0 = Instant::now();
        for i in 0..30 {
            assert!(r.read("p1", "p2", t0 + Duration::from_secs(i)).is_ok());
        }
        let err = r
            .read("p1", "p2", t0 + Duration::from_secs(45))
            .err()
            .unwrap();
        assert!(
            err.starts_with("READ_RATE: ") && err.contains("Wait 15 seconds"),
            "{err}"
        );
        // The oldest call leaves the window at t0 + 60 s.
        assert!(r.read("p1", "p2", t0 + Duration::from_secs(60)).is_ok());
        assert!(r.read("p1", "p2", t0 + Duration::from_secs(60)).is_err());
    }
}
