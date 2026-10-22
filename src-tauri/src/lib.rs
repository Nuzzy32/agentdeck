pub mod catalogue;
pub mod mcp;
pub mod delivery;
pub mod policy;
mod pty;
pub mod screen;
pub mod status;
pub mod tokens;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(pty::PaneManager::default())
        .setup(|app| {
            use tauri::Manager;
            let tokens = mcp::server::Tokens::default();
            // A failed start leaves the app usable; the status bar shows "MCP mati".
            let status = match tauri::async_runtime::block_on(mcp::server::start(tokens.clone())) {
                Ok(port) => mcp::server::McpStatus::Running { port },
                Err(e) => {
                    eprintln!("agentdeck: MCP server not started: {e}");
                    mcp::server::McpStatus::Off {
                        reason: e.to_string(),
                    }
                }
            };
            app.manage(tokens);
            app.manage(status);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pty::pane_create_shell,
            pty::pane_attach,
            pty::pane_detach,
            pty::pane_write,
            pty::pane_resize,
            pty::pane_ack,
            pty::pane_close,
        ])
        .run(tauri::generate_context!())
}
