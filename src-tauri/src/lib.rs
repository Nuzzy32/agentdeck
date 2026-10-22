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

#[derive(Clone, serde::Serialize)]
struct WorkspaceChanged {
    workspace: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
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
            // A failed start leaves the app usable (agents run without MCP); the status bar shows "MCP mati".
            let status =
                match tauri::async_runtime::block_on(mcp::server::serve(engine.clone(), on_report))
                {
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
            loop_guard_get,
            loop_guard_continue,
            loop_guard_stop,
            workspace_set_peer,
            workspace_get_peer,
            pane_report_last,
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
