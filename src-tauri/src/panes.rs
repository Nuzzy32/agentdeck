//! Pane registry (ARCHITECTURE §2–§4, §5.4): the core's one source of truth for which panes exist. Per pane: id `pN`
//! (given here, never reused), workspace, agent or shell, title, cwd, parent, PTY, token (agent panes only, revoked
//! when the process exits or the pane closes), headless screen, last official status. The MCP server maps tokens to
//! panes here; `list` is what `pane_list` returns.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use crate::catalogue::{self, Catalogue};
use crate::launch::{self, LaunchError, Mcp};
use crate::mcp::server::McpStatus;
use crate::policy::{self, PaneKind, Relation};
use crate::pty::{self, OnEvent, PaneEvent, PaneManager};
use crate::screen::Screen;
use crate::tokens::{Token, TokenRegistry};

/// Same as the WebView's `STARTING_MS`: an agent that has not reported by then is `unknown`.
const STARTING: Duration = Duration::from_secs(5);

pub type Panes = Arc<Mutex<Registry>>;

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
pub struct Registry {
    /// Workspace id → "Agen boleh saling kirim" (off by default; the switch comes in T2.8).
    workspaces: HashMap<u32, bool>,
    // ponytail: linear scans in creation order, fine for the few dozen panes an app holds.
    panes: Vec<Entry>,
    tokens: TokenRegistry,
    next_pane: u32,
    next_workspace: u32,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

impl Registry {
    pub fn create_workspace(&mut self) -> u32 {
        self.next_workspace += 1;
        self.workspaces.insert(self.next_workspace, false);
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
                    parent: None,
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
        let peer = self
            .workspaces
            .get(&me_entry.workspace)
            .copied()
            .unwrap_or(false);
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

        r.workspaces.insert(ws, true);
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
}
