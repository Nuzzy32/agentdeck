pub mod approvals;
pub mod catalogue;
pub mod delivery;
pub mod launch;
pub mod mcp;
pub mod panes;
pub mod policy;
pub mod pty;
pub mod screen;
pub mod status;
pub mod store;
pub mod tokens;

/// The launcher's agent list (T1.6): installed or not, version, install command. Off the async runtime, since
/// `--version` probes can take seconds.
#[tauri::command]
async fn agents_list() -> Result<Vec<catalogue::AgentInfo>, String> {
    tauri::async_runtime::spawn_blocking(catalogue::agents_list)
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
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
            let panes = panes::Panes::default();
            let pm = pty::PaneManager::default();
            // Without a data dir messages still flow, only unlogged.
            let store = match app.path().app_data_dir() {
                Ok(dir) => {
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
            // A failed start leaves the app usable (agents run without MCP); the status bar shows "MCP mati".
            let status = match tauri::async_runtime::block_on(mcp::server::start(engine.clone())) {
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
            app.manage(approvals::Approvals::new(approval_notify(
                app.handle().clone(),
            )));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            panes::pane_create,
            panes::pane_close,
            panes::workspace_create,
            panes::workspace_close,
            agents_list,
            pty::pane_attach,
            pty::pane_detach,
            pty::pane_write,
            pty::pane_resize,
            pty::pane_ack,
            messages_list,
            message_drop,
            loop_guard_get,
            loop_guard_continue,
            loop_guard_stop,
            workspace_set_peer,
            workspace_get_peer,
            approval_answer,
            approvals_pending,
            #[cfg(debug_assertions)]
            approval_demo,
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
