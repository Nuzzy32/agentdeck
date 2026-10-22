//! SPIKE S6 (branch spike/s6, not merged): PTY bytes -> tauri::ipc::Channel -> xterm.js throughput.
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

/// Max bytes sent to the WebView that xterm.js has not yet confirmed (write callback).
const WINDOW: u64 = 256 << 10;

/// `(sent, acked)` byte totals. Acks carry the cumulative total, so a lost ack is repaired by the next one.
#[derive(Default)]
struct Credit {
    totals: Mutex<(u64, u64)>,
    cv: Condvar,
}

type PtyEntry = (Box<dyn Write + Send>, Arc<Credit>);

#[derive(Default)]
struct Ptys(Mutex<HashMap<u32, PtyEntry>>);

#[tauri::command]
fn pty_spawn(
    ptys: State<'_, Ptys>,
    id: u32,
    argv: Vec<String>,
    rows: u16,
    cols: u16,
    on_data: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| e.to_string())?;
    let (prog, args) = argv.split_first().ok_or("empty argv")?;
    let mut cmd = CommandBuilder::new(prog);
    cmd.args(args);
    cmd.env("TERM", "xterm-256color");
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let credit = Arc::new(Credit::default());
    ptys.0.lock().map_err(|e| e.to_string())?.insert(id, (writer, credit.clone()));
    std::thread::spawn(move || {
        let _keep = (pair.master, child);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    // Backpressure: stop reading while the WebView is behind; the PTY buffer then blocks the program.
                    let Ok(mut t) = credit.totals.lock() else { break };
                    while t.0 - t.1 > WINDOW {
                        let Ok(g) = credit.cv.wait(t) else { return };
                        t = g;
                    }
                    t.0 += n as u64;
                    drop(t);
                    if on_data.send(InvokeResponseBody::Raw(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
    Ok(())
}

#[tauri::command]
async fn pty_write(ptys: State<'_, Ptys>, id: u32, data: String) -> Result<(), String> {
    let mut map = ptys.0.lock().map_err(|e| e.to_string())?;
    let (w, _) = map.get_mut(&id).ok_or("no pty")?;
    w.write_all(data.as_bytes()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn pty_ack(ptys: State<'_, Ptys>, id: u32, total: u64) -> Result<(), String> {
    let credit = ptys.0.lock().map_err(|e| e.to_string())?.get(&id).ok_or("no pty")?.1.clone();
    let mut t = credit.totals.lock().map_err(|e| e.to_string())?;
    t.1 = t.1.max(total);
    credit.cv.notify_one();
    Ok(())
}

#[tauri::command]
fn report(line: String) {
    println!("S6 {line}");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        .setup(|app| {
            // Spike only: keep the window visible so WebKit does not throttle timers or rendering.
            use tauri::Manager;
            if let Some(w) = app.get_webview_window("main") {
                w.center()?;
                w.set_always_on_top(true)?;
                w.set_focus()?;
            }
            Ok(())
        })
        .manage(Ptys::default())
        .invoke_handler(tauri::generate_handler![pty_spawn, pty_write, pty_ack, report])
        .run(tauri::generate_context!())
}
