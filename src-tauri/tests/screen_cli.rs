//! Manual (D-035): a real agent without OSC 7501 run through `PaneManager` with herdr screen rules. It only watches:
//! nothing is typed, because an agent may open a dialog (trust, sign-in) where any key chooses something. Prints every
//! published status and the final screen. Run with
//! `AGENTDECK_SCREEN_AGENT=gemini cargo test --test screen_cli -- --ignored --nocapture`.

use std::path::Path;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use agentdeck_lib::catalogue;
use agentdeck_lib::launch::{self, Launch};
use agentdeck_lib::pty::{PaneEvent, PaneManager};
use agentdeck_lib::screen::Screen;

#[test]
#[ignore]
fn real_agent_status_from_screen() {
    let agent = std::env::var("AGENTDECK_SCREEN_AGENT").expect("AGENTDECK_SCREEN_AGENT");
    let path = catalogue::login_path();
    let program = path
        .split(':')
        .map(|d| Path::new(d).join(&agent))
        .find(|p| p.is_file())
        .expect("agent on the login PATH");
    let dir = std::env::temp_dir().join(format!("agentdeck-screen-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pm = PaneManager::default();
    let (tx, rx) = mpsc::channel();
    let screen = Arc::new(Mutex::new(Screen::new(40, 120)));
    let l = Launch {
        argv: vec![program.to_str().unwrap().into()],
        env: launch::env(|k| std::env::var_os(k), path),
        osc_status: false,
        detect: Some(agent.clone()),
    };
    pm.spawn(
        &l,
        &dir,
        (40, 120),
        Some(screen.clone()),
        Box::new(move |ev| {
            let _ = tx.send(ev);
        }),
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(PaneEvent::Status { status, .. }) => seen.push(status),
            Ok(other) => eprintln!("event: {other:?}"),
            Err(_) => break,
        }
    }
    eprintln!("--- screen ---\n{}", screen.lock().unwrap().read(40));
    eprintln!("published: {seen:?}");
    pm.close_all();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!seen.is_empty(), "no status published");
}
