mod pty;
pub mod policy;
pub mod tokens;
pub mod status;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .manage(pty::PaneManager::default())
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
