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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(pty::PaneManager::default())
        .setup(|app| {
            use tauri::Manager;
            // The login-shell PATH probe takes up to seconds; start it now so the first pane does not wait for it.
            std::thread::spawn(|| {
                catalogue::login_path();
            });
            let panes = panes::Panes::default();
            // A failed start leaves the app usable (agents run without MCP); the status bar shows "MCP mati".
            let status = match tauri::async_runtime::block_on(mcp::server::start(panes.clone())) {
                Ok(port) => mcp::server::McpStatus::Running { port },
                Err(e) => {
                    eprintln!("agentdeck: MCP server not started: {e}");
                    mcp::server::McpStatus::Off {
                        reason: e.to_string(),
                    }
                }
            };
            app.manage(panes);
            app.manage(status);
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
