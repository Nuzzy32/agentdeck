pub mod approvals;
pub mod catalogue;
pub mod delivery;
pub mod launch;
pub mod mcp;
pub mod panes;
pub mod policy;
pub mod pty;
pub mod screen;
pub mod settings;
pub mod spawn;
pub mod status;
pub mod store;
pub mod tokens;
pub mod worktree;

/// The launcher's agent list (T1.6): installed or not, version, install command. Off the async runtime, since
/// `--version` probes can take seconds.
#[tauri::command]
async fn agents_list() -> Result<Vec<catalogue::AgentInfo>, String> {
    tauri::async_runtime::spawn_blocking(catalogue::agents_list)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Settings → Agen (T4.4/T4.5): the list plus manual path, login method from the CLI's own status command, and the
/// names (never values) of API key variables in the login shell. Runs login shell and CLI probes, so off the runtime.
#[tauri::command]
async fn agents_info() -> Result<Vec<catalogue::AgentDetails>, String> {
    tauri::async_runtime::spawn_blocking(catalogue::agents_info)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

/// Sets (`path`) or clears (`null`) an agent's manual CLI path (SET-03); the path must be absolute and executable.
#[tauri::command]
async fn agent_set_path(
    id: String,
    path: Option<String>,
) -> Result<catalogue::AgentDetails, String> {
    tauri::async_runtime::spawn_blocking(move || catalogue::agent_set_path(&id, path.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Workspace id from the UI: the messaging contract sends it as a string; a number is accepted too.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Ws {
    Num(u32),
    Str(String),
}

impl Ws {
    fn id(&self) -> Result<u32, String> {
        match self {
            Self::Num(n) => Ok(*n),
            Self::Str(s) => s.parse().map_err(|_| format!("bad workspace id {s:?}")),
        }
    }
}

fn unknown_ws() -> String {
    "Workspace sudah ditutup.".into()
}

/// Messages panel rows of this run, oldest first (MSG-05). Text is shown to the person; no token is ever in it.
#[tauri::command]
async fn messages_list(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
) -> Result<Vec<panes::MessageRow>, String> {
    Ok(panes::lock(&engine.panes).messages(workspace.id()?))
}

/// "Buang": a waiting message becomes dropped. A message that already left the queue is a no-op.
#[tauri::command]
async fn message_drop(
    engine: tauri::State<'_, delivery::Engine>,
    id: String,
) -> Result<(), String> {
    engine.discard(&id);
    Ok(())
}

/// "Kirim sekarang" (MSG-07): the next paste to its target, without waiting for idle, typing, or an empty input box.
/// Refused while the loop guard holds it. Blocks nothing: the target's worker pastes it.
#[tauri::command]
async fn message_force(
    engine: tauri::State<'_, delivery::Engine>,
    id: String,
) -> Result<(), String> {
    engine.force(&id)
}

#[tauri::command]
async fn loop_guard_get(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
) -> Result<panes::GuardInfo, String> {
    panes::lock(&engine.panes)
        .guard(workspace.id()?)
        .ok_or_else(unknown_ws)
}

#[tauri::command]
async fn loop_guard_continue(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
) -> Result<(), String> {
    engine
        .guard_continue(workspace.id()?)
        .map(drop)
        .ok_or_else(unknown_ws)
}

#[tauri::command]
async fn loop_guard_stop(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
) -> Result<(), String> {
    engine
        .guard_stop(workspace.id()?)
        .map(drop)
        .ok_or_else(unknown_ws)
}

#[tauri::command]
async fn workspace_set_peer(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
    on: bool,
) -> Result<(), String> {
    panes::lock(&engine.panes)
        .set_peer(workspace.id()?, on)
        .then_some(())
        .ok_or_else(unknown_ws)
}

#[tauri::command]
async fn workspace_get_peer(
    engine: tauri::State<'_, delivery::Engine>,
    workspace: Ws,
) -> Result<bool, String> {
    Ok(panes::lock(&engine.panes).peer(workspace.id()?))
}

/// The last `pane_report` of a child pane, for its header; `null` before the first.
#[tauri::command]
async fn pane_report_last(
    engine: tauri::State<'_, delivery::Engine>,
    pane: String,
) -> Result<Option<mcp::report::Report>, String> {
    Ok(panes::lock(&engine.panes).report(&pane))
}

#[tauri::command]
async fn settings_get() -> settings::Settings {
    settings::get()
}

/// Validated and saved before it takes effect; the registry and the WebView learn the new values at once.
#[tauri::command]
async fn settings_set(
    app: tauri::AppHandle,
    engine: tauri::State<'_, delivery::Engine>,
    patch: serde_json::Value,
) -> Result<settings::Settings, String> {
    use tauri::Emitter;
    let s = settings::set(patch)?;
    settings::apply(&s, &mut panes::lock(&engine.panes));
    let _ = app.emit("agentdeck://settings-changed", &s);
    Ok(s)
}

/// Settings › Tentang: shows the log folder in the file manager.
#[tauri::command]
async fn logs_open(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(drop)
        .map_err(|e| format!("Folder log tidak bisa dibuka: {e}"))
}

#[derive(Clone, serde::Serialize)]
struct WorkspaceChanged {
    workspace: String,
}

/// The person's answer to a spawn card; false when it was no longer pending (answered, timed out, or gone).
#[tauri::command]
async fn approval_answer(
    approvals: tauri::State<'_, approvals::Approvals>,
    id: String,
    allow: bool,
) -> Result<bool, String> {
    Ok(approvals.answer(&id, allow))
}

/// Pending spawn cards, oldest first, for a WebView that (re)loads.
#[tauri::command]
async fn approvals_pending(
    approvals: tauri::State<'_, approvals::Approvals>,
) -> Result<Vec<approvals::Approval>, String> {
    Ok(approvals.pending())
}

/// Debug builds only: one sample card above `pane`, to try the cards before `pane_spawn` (T3.2) exists. It closes
/// like a real request, including when the pane closes or exits. `secs` defaults to 120.
#[cfg(debug_assertions)]
#[tauri::command]
async fn approval_demo(
    approvals: tauri::State<'_, approvals::Approvals>,
    panes: tauri::State<'_, panes::Panes>,
    pane: String,
    secs: Option<u64>,
) -> Result<(), String> {
    let timeout = std::time::Duration::from_secs(secs.unwrap_or(120));
    let running = {
        let panes = panes.inner().clone();
        let pane = pane.clone();
        move || {
            panes::lock(&panes)
                .list(&pane)
                .is_some_and(|l| l.panes.iter().any(|p| p.id == pane && p.status != "exited"))
        }
    };
    let requester = panes::lock(&panes)
        .list(&pane)
        .and_then(|l| l.panes.into_iter().find(|p| p.id == pane))
        .ok_or_else(|| format!("pane {pane} tidak ada"))?
        .agent
        .unwrap_or_else(|| "Shell".into());
    let req = approvals::Request {
        pane,
        requester_agent: requester,
        agent: "Codex".into(),
        title: "review: auth".into(),
        cwd: std::env::var("HOME").unwrap_or_default(),
        task: (1..=12)
            .map(|i| {
                format!("{i}. Review the diff on branch feat/auth for bugs. Do not edit files.")
            })
            .collect::<Vec<_>>()
            .join("\n"),
    };
    let approvals = approvals.inner().clone();
    tauri::async_runtime::spawn(async move {
        let outcome = approvals.request(req, timeout, running).await;
        eprintln!("agentdeck: approval_demo -> {outcome:?}");
    });
    Ok(())
}

/// Card events to the WebView, plus a macOS notification when the window is not focused (UI-UX §4.2). Clicking a
/// notification activates the app, which brings its window and the card to the front.
fn approval_notify(handle: tauri::AppHandle) -> approvals::Notify {
    use tauri::{Emitter, Manager};
    use tauri_plugin_notification::NotificationExt;
    std::sync::Arc::new(move |n| match n {
        approvals::Notice::Opened(a) => {
            let focused = handle
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            if !focused {
                let body = format!(
                    "{} ({}) ingin membuka {}",
                    a.requester_agent, a.pane, a.agent
                );
                if let Err(e) = handle
                    .notification()
                    .builder()
                    .title("agentdeck")
                    .body(body)
                    .show()
                {
                    eprintln!("agentdeck: notification not shown: {e}");
                }
            }
            let _ = handle.emit("agentdeck://approval", a);
        }
        approvals::Notice::Closed { id, outcome } => {
            let _ = handle.emit(
                "agentdeck://approval-closed",
                ApprovalClosed { id, outcome },
            );
        }
    })
}

/// `pane_spawn` to the WebView: a child to place beside its parent, its status and exit, and refusals for a toast.
fn spawn_notify(handle: tauri::AppHandle) -> spawn::Notify {
    use tauri::Emitter;
    std::sync::Arc::new(move |n| {
        let _ = match n {
            spawn::Notice::Spawned(s) => handle.emit("agentdeck://pane-spawned", s),
            spawn::Notice::Event { pane, event } => {
                handle.emit("agentdeck://pane-event", ChildEvent { pane, event })
            }
            spawn::Notice::Refused { pane, code, limit } => handle.emit(
                "agentdeck://spawn-refused",
                SpawnRefused { pane, code, limit },
            ),
        };
    })
}

#[derive(Clone, serde::Serialize)]
struct ChildEvent {
    pane: String,
    event: pty::PaneEvent,
}

#[derive(Clone, serde::Serialize)]
struct SpawnRefused {
    pane: String,
    code: &'static str,
    limit: Option<u32>,
}

#[derive(Clone, serde::Serialize)]
struct ApprovalClosed {
    id: String,
    outcome: approvals::Outcome,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            use tauri::{Emitter, Manager};
            // The login-shell PATH probe takes up to seconds; start it now so the first pane does not wait for it.
            std::thread::spawn(|| {
                catalogue::login_path();
            });
            let pm = pty::PaneManager::default();
            let data_dir = app.path().app_data_dir();
            if let Some(warning) = settings::init(data_dir.as_deref().ok()) {
                eprintln!("agentdeck: {warning}");
            }
            let panes = panes::Panes::default();
            settings::apply(&settings::get(), &mut panes::lock(&panes));
            // Without a data dir messages still flow, only unlogged.
            let store = match data_dir {
                Ok(dir) => {
                    catalogue::set_data_dir(dir.clone());
                    let store = store::Store::new(&dir);
                    if let Err(e) = store.prune(std::time::SystemTime::now()) {
                        eprintln!("agentdeck: old logs not pruned: {e}");
                    }
                    Some(store)
                }
                Err(e) => {
                    eprintln!("agentdeck: no app data dir, messages not logged: {e}");
                    None
                }
            };
            let handle = app.handle().clone();
            let notify: delivery::Notify = std::sync::Arc::new(move |n| {
                let _ = match n {
                    delivery::Notice::Messages { workspace } => handle.emit(
                        "agentdeck://messages-changed",
                        WorkspaceChanged { workspace },
                    ),
                    delivery::Notice::LoopGuard(g) => handle.emit("agentdeck://loop-guard", g),
                };
            });
            let engine = delivery::Engine::new(panes.clone(), pm.clone(), store, notify);
            let handle = app.handle().clone();
            let on_report: mcp::report::OnReport = std::sync::Arc::new(move |r| {
                let _ = handle.emit("agentdeck://pane-report", r);
            });
            let approvals = approvals::Approvals::new(approval_notify(app.handle().clone()));
            let spawner =
                spawn::Spawner::new(approvals.clone(), spawn_notify(app.handle().clone()));
            // A failed start leaves the app usable (agents run without MCP); the status bar shows "MCP mati".
            let serve = mcp::server::serve_with(engine.clone(), on_report, Some(spawner));
            let status = match tauri::async_runtime::block_on(serve) {
                Ok(port) => mcp::server::McpStatus::Running { port },
                Err(e) => {
                    eprintln!("agentdeck: MCP server not started: {e}");
                    mcp::server::McpStatus::Off {
                        reason: e.to_string(),
                    }
                }
            };
            app.manage(panes);
            app.manage(pm);
            app.manage(engine);
            app.manage(status);
            app.manage(approvals);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            panes::pane_create,
            panes::pane_close,
            panes::workspace_create,
            panes::workspace_close,
            agents_list,
            agents_info,
            agent_set_path,
            pty::pane_attach,
            pty::pane_detach,
            pty::pane_write,
            pty::pane_resize,
            pty::pane_ack,
            messages_list,
            message_drop,
            message_force,
            loop_guard_get,
            loop_guard_continue,
            loop_guard_stop,
            workspace_set_peer,
            workspace_get_peer,
            pane_report_last,
            approval_answer,
            approvals_pending,
            #[cfg(debug_assertions)]
            approval_demo,
            settings_get,
            settings_set,
            logs_open,
        ])
        .build(tauri::generate_context!())?
        .run(|app, event| {
            // Exit fires once on every way out: last window closed, ⌘Q, app.exit (TERM-05).
            if let tauri::RunEvent::Exit = event {
                use tauri::Manager;
                app.state::<pty::PaneManager>().close_all();
            }
        });
    Ok(())
}
