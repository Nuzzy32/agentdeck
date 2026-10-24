//! `pane_spawn` (MCP-TOOLS, SPN-01..05, T3.2 + T4.1): an agent asks to open another agent as its child.
//!
//! Order: arguments → switch, nesting and limits (`policy::check_spawn`) → the caller's Claude 5-hour quota
//! (`policy::check_quota`, D-033) → agent from the catalogue and installed →
//! `cwd` canonicalised and inside the caller's folder → (worktree) a git repository → a rate slot and a place among the
//! caller's children are taken → the person's card (`approvals.rs`, timeout from Settings) → the child runs through
//! the same launch path as a pane the person opens (`panes::start`: no shell, env from scratch, MCP injected, task
//! after `--`), with the caller as parent. The call returns once the process runs. Every step that decides something
//! is written to the audit log (ARCHITECTURE §8); nothing here ever handles a token.
//!
//! The tool takes no command, flag, environment, or permission mode: the child gets exactly the catalogue's argv.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::model::JsonObject;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::approvals::{Approvals, Outcome, Request};
use crate::catalogue::{self, Catalogue};
use crate::delivery::Engine;
use crate::launch;
use crate::panes::{self, Panes, SpawnCaller, Spec};
use crate::policy::{self, Limit, PolicyError, TASK_MAX_BYTES};
use crate::pty::PaneEvent;
use crate::settings::{self, Settings};
use crate::store::Audit;
use crate::usage;
use crate::worktree;

/// `title`: 1–60 characters (MCP-TOOLS).
pub const TITLE_MAX_CHARS: usize = 60;
/// Size of the child's PTY until its view attaches and fits it.
const ROWS: u16 = 24;
const COLS: u16 = 80;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    agent: String,
    title: String,
    task: String,
    cwd: Option<String>,
    #[serde(default)]
    worktree: bool,
}

/// A child the core opened, for the UI to place beside its parent (`agentdeck://pane-spawned`).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Spawned {
    pub pane: String,
    pub pty: u32,
    pub workspace: u32,
    pub parent: String,
    /// Catalogue id and display name.
    pub agent: String,
    pub agent_name: String,
    pub title: String,
    pub cwd: String,
    /// Branch of the child's own worktree (T4.1).
    pub branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    Spawned(Spawned),
    /// Status or exit of a child (the UI has no `Channel` for a pane it did not create). Always after its `Spawned`.
    Event {
        pane: String,
        event: PaneEvent,
    },
    /// Refused by the switch or a limit, for a toast at the caller (UI-UX §5). `limit`: the configured number.
    Refused {
        pane: String,
        code: &'static str,
        limit: Option<u32>,
    },
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;
/// From-scratch env of a child (`launch::env`); its `PATH` also decides whether the agent is installed.
pub type EnvFn = Arc<dyn Fn() -> Vec<(String, OsString)> + Send + Sync>;

/// Everything `pane_spawn` needs besides the engine. Cheap to clone.
#[derive(Clone)]
pub struct Spawner {
    pub approvals: Approvals,
    /// `http://127.0.0.1:<port>/mcp`, set by `server::serve_with` once bound.
    pub url: String,
    pub env: EnvFn,
    /// Current settings (switch, card timeout); tests pass their own.
    pub settings: fn() -> Settings,
    pub notify: Notify,
    /// Claude 5-hour usage of a pane in percent, `None` without a reading (D-033); tests pass their own.
    pub five_hour: fn(&str) -> Option<u8>,
}

/// From the pane's statusline (D-029); only Claude panes have one, and only after their first turn.
fn five_hour(pane: &str) -> Option<u8> {
    usage::read(pane)?.five_hour.map(|w| w.used)
}

fn bad(why: &str) -> String {
    format!("BAD_ARGS: {why}. Fix the arguments.")
}

fn check_text(title: &str, task: &str) -> Result<(), String> {
    if title.trim().is_empty() || title.chars().count() > TITLE_MAX_CHARS {
        return Err(bad("title must be 1 to 60 characters"));
    }
    if title.chars().any(char::is_control) {
        return Err(bad("title must be one line without control characters"));
    }
    if task.trim().is_empty() || task.len() > TASK_MAX_BYTES {
        return Err(bad("task must be 1 to 16384 bytes"));
    }
    if task.contains('\0') {
        return Err(bad("task must not contain NUL"));
    }
    Ok(())
}

/// The child's folder: `requested` (absolute, no `..`) after resolving symlinks, which must be inside the caller's
/// folder (also resolved); the caller's folder when `None`.
pub fn child_cwd(caller: &Path, requested: Option<&str>) -> Result<PathBuf, String> {
    let base = dunce::canonicalize(caller).map_err(|_| {
        "CWD_OUTSIDE: your own folder no longer exists, so no folder is inside it. Tell the person.".to_owned()
    })?;
    let Some(req) = requested else {
        return Ok(base);
    };
    let p = Path::new(req);
    if !p.is_absolute() || p.components().any(|c| c == Component::ParentDir) {
        return Err(bad("cwd must be an absolute path without \"..\""));
    }
    let real = dunce::canonicalize(p).map_err(|_| bad("cwd does not exist"))?;
    if !real.starts_with(&base) {
        return Err(format!(
            "CWD_OUTSIDE: {req} is outside your working folder {}. Fix the path, or leave cwd out.",
            base.display()
        ));
    }
    if !real.is_dir() {
        return Err(bad("cwd is not a folder"));
    }
    Ok(real)
}

/// Gives back the caller's place among its children when the request ends, however it ends.
struct Slot {
    panes: Panes,
    caller: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        panes::lock(&self.panes).spawn_done(&self.caller);
    }
}

impl Spawner {
    /// The app's spawner: settings from `settings.rs`, env like a pane the person opens. `url` comes with the server.
    pub fn new(approvals: Approvals, notify: Notify) -> Self {
        Self {
            approvals,
            url: String::new(),
            env: Arc::new(|| launch::env(|k| std::env::var_os(k), catalogue::login_path())),
            settings: settings::get,
            notify,
            five_hour,
        }
    }

    /// Switch, nesting, limits, and (before anything is reserved) the quota guard; a refusal is audited and, for the
    /// switch, the limits, and the quota, told to the UI.
    fn policy(
        &self,
        engine: &Engine,
        caller: &str,
        enabled: bool,
        reserve: bool,
    ) -> Result<SpawnCaller, String> {
        let checked =
            panes::lock(&engine.panes).spawn_check(caller, Instant::now(), enabled, reserve);
        let checked = checked.and_then(|who| {
            if reserve {
                return Ok(who); // checked by the first call; failing now would keep the slot
            }
            let guard = panes::lock(&engine.panes).limits.quota_guard();
            policy::check_quota((self.five_hour)(caller), guard).map(|()| who)
        });
        checked.map_err(|e| {
            let (ws, limits) = {
                let reg = panes::lock(&engine.panes);
                (reg.workspace_of(caller), reg.limits.clone())
            };
            let limit = match e {
                PolicyError::CapChildren => Some(limits.get(Limit::ChildrenPerPane)),
                PolicyError::CapGlobal => Some(limits.get(Limit::AgentOpenedGlobal)),
                PolicyError::Rate { .. } => Some(limits.get(Limit::SpawnsPerWindow)),
                PolicyError::QuotaLow { guard, .. } => Some(u32::from(guard)),
                _ => None,
            };
            if matches!(
                e,
                PolicyError::SpawnOff
                    | PolicyError::CapChildren
                    | PolicyError::CapGlobal
                    | PolicyError::Rate { .. }
                    | PolicyError::QuotaLow { .. }
            ) {
                (self.notify)(Notice::Refused {
                    pane: caller.to_owned(),
                    code: e.code(),
                    limit,
                });
            }
            if let Some(ws) = ws {
                let audit = Audit::SpawnRefused {
                    from: caller,
                    code: e.code(),
                };
                engine.audit(&ws.to_string(), &audit);
            }
            e.to_string()
        })
    }

    /// `pane_spawn` for pane `caller` (from its token). `Ok` and `Err` are both the text the agent reads.
    pub async fn call(
        &self,
        engine: &Engine,
        caller: &str,
        raw: Option<JsonObject>,
    ) -> Result<String, String> {
        let a: Args = serde_json::from_value(Value::Object(raw.unwrap_or_default()))
            .map_err(|e| bad(&e.to_string()))?;
        check_text(&a.title, &a.task)?;
        let s = (self.settings)();
        let caller_cwd = self.policy(engine, caller, s.spawn_enabled, false)?.cwd;

        let cat = Catalogue::builtin().map_err(|e| {
            format!(
                "AGENT_NOT_INSTALLED: the agent catalogue cannot be read ({e}). Tell the person."
            )
        })?;
        let Some(agent) = cat.get(&a.agent) else {
            let ids: Vec<&str> = cat.agents.iter().map(|g| g.id.as_str()).collect();
            return Err(format!(
                "UNKNOWN_AGENT: no agent {:?}. Use one of: {}.",
                a.agent,
                ids.join(", ")
            ));
        };
        let env = (self.env)();
        let path = env
            .iter()
            .find(|(k, _)| k == "PATH")
            .and_then(|(_, v)| v.to_str())
            .unwrap_or_default();
        if catalogue::resolve(agent, path).is_err() {
            return Err(format!(
                "AGENT_NOT_INSTALLED: {} is not installed on this machine. Pick another agent or tell the person.",
                agent.name
            ));
        }
        let cwd = child_cwd(&caller_cwd, a.cwd.as_deref())?;
        let repo = match a.worktree {
            false => None,
            true => Some(worktree::repo_root(&cwd).ok_or_else(|| {
                bad("worktree: true needs a cwd inside a git repository; leave worktree out")
            })?),
        };

        let who = self.policy(engine, caller, s.spawn_enabled, true)?;
        let _slot = Slot {
            panes: engine.panes.clone(),
            caller: caller.to_owned(),
        };
        let ws = who.workspace.to_string();
        let shown = cwd.display().to_string();
        engine.audit(
            &ws,
            &Audit::SpawnRequested {
                from: caller,
                agent: &agent.id,
                cwd: &shown,
                worktree: a.worktree,
            },
        );
        let req = Request {
            pane: caller.to_owned(),
            requester_agent: cat
                .get(&who.agent)
                .map_or_else(|| who.agent.clone(), |g| g.name.clone()),
            agent: agent.name.clone(),
            title: a.title.clone(),
            cwd: if a.worktree {
                format!("{shown} (worktree baru)")
            } else {
                shown.clone()
            },
            task: a.task.clone(),
        };
        let timeout = Duration::from_secs(u64::from(s.limits.approval_timeout_s));
        let alive = {
            let panes = engine.panes.clone();
            let me = caller.to_owned();
            move || panes::lock(&panes).running(&me)
        };
        let denied = |reason: &str| {
            let audit = Audit::SpawnDenied {
                from: caller,
                agent: &agent.id,
                cwd: &shown,
                reason,
            };
            engine.audit(&ws, &audit);
        };
        match self.approvals.request(req, timeout, alive).await {
            Outcome::Approved => {}
            Outcome::Denied => {
                denied("denied");
                return Err("DENIED: the person declined this spawn. Do the work yourself; do not ask again \
                            for the same spawn."
                    .into());
            }
            Outcome::Timeout => {
                denied("timeout");
                return Err(format!(
                    "APPROVAL_TIMEOUT: the person did not answer the card within {} seconds. Ask the person \
                     whether to try again.",
                    timeout.as_secs()
                ));
            }
        }

        let job = Launch {
            spawner: self.clone(),
            engine: engine.clone(),
            caller: caller.to_owned(),
            workspace: who.workspace,
            agent: agent.id.clone(),
            agent_name: agent.name.clone(),
            title: a.title,
            task: a.task,
            cwd,
            repo,
            env,
        };
        let launched = tokio::task::spawn_blocking(move || job.run())
            .await
            .map_err(|e| format!("SPAWN_FAILED: {e}. Tell the person."))?;
        match launched {
            Ok(child) => {
                let audit = Audit::SpawnApproved {
                    from: caller,
                    agent: &agent.id,
                    cwd: &child.cwd,
                    pane: &child.pane,
                };
                engine.audit(&ws, &audit);
                let mut out = json!({"pane": child.pane, "cwd": child.cwd, "status": "starting"});
                if let Some(b) = &child.branch {
                    out["branch"] = json!(b);
                }
                Ok(out.to_string())
            }
            Err(e) => {
                denied("launch failed");
                Err(e)
            }
        }
    }
}

/// The approved child, started off the async runtime (git and PTY calls block).
struct Launch {
    spawner: Spawner,
    engine: Engine,
    caller: String,
    workspace: u32,
    agent: String,
    agent_name: String,
    title: String,
    task: String,
    cwd: PathBuf,
    repo: Option<PathBuf>,
    env: Vec<(String, OsString)>,
}

/// Events of a child before its id is known wait here, so the UI hears `Spawned` first.
type Early = Arc<Mutex<(Option<String>, Vec<PaneEvent>)>>;

fn early_lock(e: &Early) -> std::sync::MutexGuard<'_, (Option<String>, Vec<PaneEvent>)> {
    e.lock().unwrap_or_else(|p| p.into_inner())
}

impl Launch {
    fn run(self) -> Result<Spawned, String> {
        let failed = |e: &str| format!("SPAWN_FAILED: {e}. Tell the person.");
        let Some(cwd_str) = self.cwd.to_str().map(str::to_owned) else {
            return Err(failed("the folder path is not UTF-8"));
        };
        let spec = Spec {
            parent: Some(self.caller.clone()),
            workspace: self.workspace,
            pane: None,
            agent: Some(self.agent.clone()),
            title: self.title.clone(),
            cwd: cwd_str,
            task: Some(self.task.clone()),
            rows: ROWS,
            cols: COLS,
            // Never from an agent (D-034); the worktree, when asked for, comes from `start_in_worktree` below.
            worktree: false,
            bypass: false,
        };
        let early: Early = Arc::default();
        let on_event = {
            let early = early.clone();
            let notify = self.spawner.notify.clone();
            Box::new(move |event: PaneEvent| {
                let mut e = early_lock(&early);
                match &e.0 {
                    Some(pane) => notify(Notice::Event {
                        pane: pane.clone(),
                        event,
                    }),
                    None => e.1.push(event),
                }
            })
        };
        let (panes, pm, url) = (
            &self.engine.panes,
            &self.engine.pm,
            Some(self.spawner.url.as_str()),
        );
        let started = match &self.repo {
            None => panes::start(panes, pm, url, self.env, &spec, on_event),
            Some(root) => panes::start_in_worktree(panes, pm, url, self.env, &spec, root, on_event),
        }
        .map_err(|e| failed(&e.message))?;
        let (cwd_str, branch) = (started.cwd.clone(), started.branch.clone());
        let child = Spawned {
            pane: started.pane.clone(),
            pty: started.pty,
            workspace: self.workspace,
            parent: self.caller,
            agent: self.agent,
            agent_name: self.agent_name,
            title: self.title,
            cwd: cwd_str,
            branch,
        };
        let mut e = early_lock(&early);
        (self.spawner.notify)(Notice::Spawned(child.clone()));
        for event in std::mem::take(&mut e.1) {
            (self.spawner.notify)(Notice::Event {
                pane: started.pane.clone(),
                event,
            });
        }
        e.0 = Some(started.pane);
        Ok(child)
    }
}

// Uses Unix permissions, symlinks and #!/bin/sh fakes.
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::pty::PaneManager;
    use crate::settings::LimitValues;
    use crate::store::Store;
    use std::os::unix::fs::PermissionsExt;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "agentdeck-spawn-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        dunce::canonicalize(&d).unwrap()
    }

    fn on() -> Settings {
        Settings {
            spawn_enabled: true,
            limits: LimitValues {
                approval_timeout_s: 1,
                ..LimitValues::default()
            },
            ..Settings::default()
        }
    }

    struct Fx {
        dir: PathBuf,
        engine: Engine,
        spawner: Spawner,
        heard: Arc<Mutex<Vec<Notice>>>,
        token: String,
    }

    /// p1 claude-code (person-opened) in `dir/work`, p2 its codex child, p3 a shell. No processes. `bin` holds an
    /// executable `codex` only, so claude-code counts as not installed.
    fn fx(tag: &str, settings: fn() -> Settings) -> Fx {
        fx_with(tag, settings, |_| None)
    }

    fn fx_with(tag: &str, settings: fn() -> Settings, five_hour: fn(&str) -> Option<u8>) -> Fx {
        let dir = tmp(tag);
        let work = dir.join("work");
        std::fs::create_dir_all(work.join("sub")).unwrap();
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("codex"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(bin.join("codex"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
        let panes = Panes::default();
        let token = {
            let mut r = panes::lock(&panes);
            let ws = r.create_workspace();
            let spec = |agent: Option<&str>, parent: Option<&str>| Spec {
                parent: parent.map(Into::into),
                workspace: ws,
                pane: None,
                agent: agent.map(Into::into),
                title: "t".into(),
                cwd: work.display().to_string(),
                task: None,
                rows: 24,
                cols: 80,
                worktree: false,
                bypass: false,
            };
            let t = r
                .open(&spec(Some("claude-code"), None))
                .unwrap()
                .token
                .unwrap();
            r.open(&spec(Some("codex"), Some("p1"))).unwrap();
            r.open(&spec(None, None)).unwrap();
            t.expose().to_owned()
        };
        let engine = Engine::new(
            panes,
            PaneManager::default(),
            Some(Store::new(&dir.join("data"))),
            Arc::new(|_| {}),
        );
        let heard: Arc<Mutex<Vec<Notice>>> = Arc::default();
        let sink = heard.clone();
        let path = bin.display().to_string();
        let spawner = Spawner {
            approvals: Approvals::new(Arc::new(|_| {})),
            url: "http://127.0.0.1:1/mcp".into(),
            env: Arc::new(move || vec![("PATH".into(), path.clone().into())]),
            settings,
            notify: Arc::new(move |n| sink.lock().unwrap().push(n)),
            five_hour,
        };
        Fx {
            dir,
            engine,
            spawner,
            heard,
            token,
        }
    }

    impl Fx {
        async fn call(&self, caller: &str, args: Value) -> Result<String, String> {
            let Value::Object(a) = args else { panic!() };
            self.spawner.call(&self.engine, caller, Some(a)).await
        }

        fn work(&self) -> PathBuf {
            self.dir.join("work")
        }

        fn audit(&self) -> String {
            let logs = self.dir.join("data/logs");
            std::fs::read_dir(&logs)
                .map(|d| {
                    d.flatten()
                        .map(|f| std::fs::read_to_string(f.path()).unwrap())
                        .collect()
                })
                .unwrap_or_default()
        }
    }

    fn code(r: Result<String, String>) -> String {
        let e = r.expect_err("expected an error");
        let (code, rest) = e.split_once(": ").unwrap();
        assert!(rest.ends_with('.'), "{e}");
        code.to_owned()
    }

    fn args(cwd: Option<&str>) -> Value {
        let mut a = json!({"agent": "codex", "title": "review: auth", "task": "Review the diff."});
        if let Some(c) = cwd {
            a["cwd"] = json!(c);
        }
        a
    }

    #[tokio::test]
    async fn bad_args_and_validation_codes() {
        let f = fx("args", on);
        let work = f.work();
        let w = work.display().to_string();
        for a in [
            json!({"agent": "codex", "title": "t"}),
            json!({"agent": "codex", "title": "t", "task": "x", "command": "rm -rf /"}),
            json!({"agent": "codex", "title": "t", "task": "x", "env": {"A": "1"}}),
            json!({"agent": "codex", "title": "t", "task": "x", "flags": ["--yolo"]}),
            json!({"agent": "codex", "title": "", "task": "x"}),
            json!({"agent": "codex", "title": "a".repeat(61), "task": "x"}),
            json!({"agent": "codex", "title": "a\nb", "task": "x"}),
            json!({"agent": "codex", "title": "t", "task": " "}),
            json!({"agent": "codex", "title": "t", "task": "a".repeat(16 * 1024 + 1)}),
            json!({"agent": "codex", "title": "t", "task": "a\0b"}),
            args(Some("work/sub")),
            args(Some(&format!("{w}/sub/../sub"))),
            args(Some(&format!("{w}/missing"))),
            json!({"agent": "codex", "title": "t", "task": "x", "worktree": true}),
        ] {
            assert_eq!(code(f.call("p1", a.clone()).await), "BAD_ARGS", "{a}");
        }
        // 60 characters (not bytes) and exactly 16 KB pass validation; they fail later only for the card timeout.
        let ok = json!({"agent": "codex", "title": "é".repeat(60), "task": "a".repeat(16 * 1024)});
        assert_eq!(code(f.call("p1", ok).await), "APPROVAL_TIMEOUT");
        assert_eq!(
            code(
                f.call("p1", json!({"agent": "gemini", "title": "t", "task": "x"}))
                    .await
            ),
            "UNKNOWN_AGENT"
        );
        let e = f
            .call("p1", json!({"agent": "gemini", "title": "t", "task": "x"}))
            .await
            .unwrap_err();
        assert!(e.contains("claude-code, codex"), "{e}");
        assert_eq!(
            code(
                f.call(
                    "p1",
                    json!({"agent": "claude-code", "title": "t", "task": "x"})
                )
                .await
            ),
            "AGENT_NOT_INSTALLED"
        );
    }

    #[tokio::test]
    async fn cwd_must_stay_inside_callers_folder_symlinks_included() {
        let f = fx("cwd", on);
        let work = f.work();
        let outside = f.dir.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, work.join("escape")).unwrap();
        std::os::unix::fs::symlink(work.join("sub"), f.dir.join("into")).unwrap();
        assert_eq!(
            code(
                f.call("p1", args(Some(&outside.display().to_string())))
                    .await
            ),
            "CWD_OUTSIDE"
        );
        assert_eq!(
            code(
                f.call("p1", args(Some(&format!("{}/escape", work.display()))))
                    .await
            ),
            "CWD_OUTSIDE"
        );
        assert_eq!(code(f.call("p1", args(Some("/"))).await), "CWD_OUTSIDE");
        // A sibling whose name only starts like the folder is outside too.
        let twin = f.dir.join("work2");
        std::fs::create_dir_all(&twin).unwrap();
        assert_eq!(
            code(f.call("p1", args(Some(&twin.display().to_string()))).await),
            "CWD_OUTSIDE"
        );
        // Inside, also through a symlink that resolves inside: the card is shown (and times out here).
        assert_eq!(
            child_cwd(&work, Some(&format!("{}/into", f.dir.display()))),
            Ok(work.join("sub"))
        );
        assert_eq!(child_cwd(&work, None), Ok(work.clone()));
        assert_eq!(
            code(
                f.call("p1", args(Some(&format!("{}/sub", work.display()))))
                    .await
            ),
            "APPROVAL_TIMEOUT"
        );
    }

    #[tokio::test]
    async fn switch_nesting_and_limits_refuse_before_the_card() {
        fn off() -> Settings {
            Settings::default()
        }
        let f = fx("off", off);
        assert_eq!(code(f.call("p1", args(None)).await), "SPAWN_OFF");
        assert_eq!(
            f.heard.lock().unwrap().as_slice(),
            [Notice::Refused {
                pane: "p1".into(),
                code: "SPAWN_OFF",
                limit: None
            }]
        );
        assert!(f.spawner.approvals.pending().is_empty());

        let f = fx("caps", on);
        assert_eq!(code(f.call("p2", args(None)).await), "NESTED");
        assert_eq!(code(f.call("p3", args(None)).await), "NOT_ALLOWED");
        // p1 has one live child; with a cap of 1 it may not open another.
        panes::lock(&f.engine.panes)
            .limits
            .set(Limit::ChildrenPerPane, 1)
            .unwrap();
        assert_eq!(code(f.call("p1", args(None)).await), "CAP_CHILDREN");
        panes::lock(&f.engine.panes)
            .limits
            .set(Limit::AgentOpenedGlobal, 1)
            .unwrap();
        panes::lock(&f.engine.panes)
            .limits
            .set(Limit::ChildrenPerPane, 5)
            .unwrap();
        assert_eq!(code(f.call("p1", args(None)).await), "CAP_GLOBAL");
        assert!(matches!(
            f.heard.lock().unwrap().last(),
            Some(Notice::Refused {
                code: "CAP_GLOBAL",
                limit: Some(1),
                ..
            })
        ));
        assert!(
            f.audit()
                .contains(r#""event":"spawn_refused","from":"p1","code":"CAP_GLOBAL""#)
        );
    }

    #[tokio::test]
    async fn rate_counts_requests_shown_and_pending_cards_count_as_children() {
        let f = fx("rate", on);
        {
            let mut r = panes::lock(&f.engine.panes);
            r.limits.set(Limit::SpawnsPerWindow, 2).unwrap();
            r.limits.set(Limit::ChildrenPerPane, 2).unwrap();
        }
        // While one card waits, p1 has 1 live child + 1 pending = the cap of 2.
        let a = f.clone_spawner_call("p1");
        while f.spawner.approvals.pending().is_empty() {
            tokio::task::yield_now().await;
        }
        assert_eq!(code(f.call("p1", args(None)).await), "CAP_CHILDREN");
        let id = f.spawner.approvals.pending()[0].id.clone();
        assert!(f.spawner.approvals.answer(&id, false));
        assert_eq!(code(a.await.unwrap()), "DENIED");
        // The denied request used a rate slot; the second card times out and uses the other.
        assert_eq!(code(f.call("p1", args(None)).await), "APPROVAL_TIMEOUT");
        let e = f.call("p1", args(None)).await.unwrap_err();
        assert!(e.starts_with("RATE: ") && e.contains("seconds"), "{e}");
    }

    impl Fx {
        fn clone_spawner_call(
            &self,
            caller: &str,
        ) -> tokio::task::JoinHandle<Result<String, String>> {
            let (s, e, c) = (self.spawner.clone(), self.engine.clone(), caller.to_owned());
            tokio::spawn(async move {
                let Value::Object(a) = args(None) else {
                    panic!()
                };
                s.call(&e, &c, Some(a)).await
            })
        }
    }

    #[tokio::test]
    async fn denied_and_timeout_are_audited_without_token() {
        let f = fx("audit", on);
        let task = f.clone_spawner_call("p1");
        while f.spawner.approvals.pending().is_empty() {
            tokio::task::yield_now().await;
        }
        let card = f.spawner.approvals.pending()[0].clone();
        assert_eq!(
            (
                card.pane.as_str(),
                card.requester_agent.as_str(),
                card.agent.as_str()
            ),
            ("p1", "Claude Code", "Codex")
        );
        assert_eq!(card.cwd, f.work().display().to_string());
        assert_eq!(
            card.expires_at - card.created_at,
            1000,
            "timeout from settings"
        );
        f.spawner.approvals.answer(&card.id, false);
        let e = task.await.unwrap().unwrap_err();
        assert!(e.starts_with("DENIED: "), "{e}");
        assert_eq!(code(f.call("p1", args(None)).await), "APPROVAL_TIMEOUT");
        let log = f.audit();
        let events: Vec<&str> = log
            .lines()
            .map(|l| {
                l.split(r#""event":""#)
                    .nth(1)
                    .unwrap()
                    .split('"')
                    .next()
                    .unwrap()
            })
            .collect();
        assert_eq!(
            events,
            [
                "spawn_requested",
                "spawn_denied",
                "spawn_requested",
                "spawn_denied"
            ]
        );
        assert!(log.contains(r#""reason":"denied""#) && log.contains(r#""reason":"timeout""#));
        assert!(!log.contains(&f.token));
        assert!(f.token.len() == 64);
    }

    #[tokio::test]
    async fn quota_guard_refuses_before_the_card_and_skips_panes_without_a_reading() {
        // p1 (claude-code) is at 90 %, p4 below the guard; p2/p3 and the rest have no reading.
        let f = fx_with("quota", on, |p| match p {
            "p1" => Some(90),
            "p4" => Some(84),
            _ => None,
        });
        assert_eq!(code(f.call("p1", args(None)).await), "QUOTA_LOW");
        let e = f.call("p1", args(None)).await.unwrap_err();
        assert!(e.contains("90%") && e.contains("85%"), "{e}");
        assert!(f.spawner.approvals.pending().is_empty(), "no card");
        assert!(matches!(
            f.heard.lock().unwrap().last(),
            Some(Notice::Refused {
                code: "QUOTA_LOW",
                limit: Some(85),
                ..
            })
        ));
        assert!(
            f.audit()
                .contains(r#""event":"spawn_refused","from":"p1","code":"QUOTA_LOW""#)
        );
        // The refusal reserved nothing: once the guard is raised the same caller gets its card.
        panes::lock(&f.engine.panes)
            .limits
            .set_quota_guard(95)
            .unwrap();
        assert_eq!(code(f.call("p1", args(None)).await), "APPROVAL_TIMEOUT");
        panes::lock(&f.engine.panes)
            .limits
            .set_quota_guard(85)
            .unwrap();
        // Switch, nesting, and limits still come first.
        assert_eq!(code(f.call("p2", args(None)).await), "NESTED");
        // A person-opened pane without a reading (Codex, or Claude before its first turn) and one below the guard
        // both reach the card.
        let ws = panes::lock(&f.engine.panes).workspace_of("p1").unwrap();
        for agent in ["codex", "claude-code"] {
            let spec = Spec {
                parent: None,
                workspace: ws,
                pane: None,
                agent: Some(agent.into()),
                title: "t".into(),
                cwd: f.work().display().to_string(),
                task: None,
                rows: 24,
                cols: 80,
                worktree: false,
                bypass: false,
            };
            panes::lock(&f.engine.panes).open(&spec).unwrap();
        }
        assert_eq!(code(f.call("p4", args(None)).await), "APPROVAL_TIMEOUT");
        assert_eq!(code(f.call("p5", args(None)).await), "APPROVAL_TIMEOUT");
    }

    #[tokio::test]
    async fn requester_gone_closes_the_card_as_denied() {
        let f = fx("gone", on);
        let task = f.clone_spawner_call("p1");
        while f.spawner.approvals.pending().is_empty() {
            tokio::task::yield_now().await;
        }
        panes::lock(&f.engine.panes).close("p1");
        assert_eq!(code(task.await.unwrap()), "DENIED");
    }
}
