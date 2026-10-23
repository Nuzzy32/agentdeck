//! Manual (D-034): real Claude Code and Codex opened the way the launcher does with "worktree per terminal" and
//! "bypass izin" on, without MCP and without any prompt, so no quota is used. Claude stops at its own bypass consent
//! screen, which this test never answers. Run with
//! `AGENTDECK_BYPASS_REPO=/path/to/git/repo cargo test --test bypass_cli -- --ignored --nocapture`.

use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use agentdeck_lib::catalogue;
use agentdeck_lib::launch;
use agentdeck_lib::panes::{self, Panes, Spec};
use agentdeck_lib::pty::PaneManager;
use agentdeck_lib::worktree;

/// Command lines of this test's own child processes (the PTYs' programs), nothing else on the machine.
fn argv_of_children(parent: u32) -> String {
    let out = Command::new("ps")
        .args(["-axo", "ppid=,command="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (ppid, cmd) = l.trim_start().split_once(' ')?;
            (ppid == parent.to_string()).then(|| cmd.to_owned())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ignore]
fn launcher_bypass_and_worktree_with_real_clis() {
    let repo = std::env::var("AGENTDECK_BYPASS_REPO").expect("AGENTDECK_BYPASS_REPO");
    let root = worktree::repo_root(Path::new(&repo)).expect("a git repo with a commit");
    let panes = Panes::default();
    let pm = PaneManager::default();
    let ws = panes::lock(&panes).create_workspace();
    let env = || launch::env(|k| std::env::var_os(k), catalogue::login_path());
    let spec = |agent: &str| Spec {
        parent: None,
        workspace: ws,
        pane: None,
        agent: Some(agent.into()),
        title: format!("bypass: {agent}"),
        cwd: repo.clone(),
        task: None,
        rows: 40,
        cols: 120,
        worktree: true,
        bypass: true,
    };
    let mut started = Vec::new();
    for agent in ["claude-code", "codex"] {
        let s = panes::start_in_worktree(
            &panes,
            &pm,
            None,
            env(),
            &spec(agent),
            &root,
            Box::new(|_| {}),
        )
        .unwrap();
        eprintln!(
            "{agent}: pane {} cwd {} branch {:?} bypass {}",
            s.pane, s.cwd, s.branch, s.bypass
        );
        assert!(s.bypass);
        assert!(s.cwd.contains("/.worktrees/"), "{}", s.cwd);
        assert!(Path::new(&s.cwd).is_dir());
        started.push(s);
    }
    thread::sleep(Duration::from_secs(8));
    let ps = argv_of_children(std::process::id());
    eprintln!("--- processes ---\n{ps}");
    assert!(ps.contains("--dangerously-skip-permissions"));
    assert!(ps.contains("--dangerously-bypass-approvals-and-sandbox"));
    for s in &started {
        let screen = panes::lock(&panes).screen(&s.pane).unwrap();
        eprintln!("--- {} ---\n{}", s.pane, screen.lock().unwrap().read(40));
    }
    let list = Command::new("git")
        .args(["worktree", "list"])
        .current_dir(&root)
        .output()
        .unwrap();
    eprintln!(
        "--- worktrees while open ---\n{}",
        String::from_utf8_lossy(&list.stdout)
    );
    // Closing removes the unchanged worktrees, as when the person closes the panes.
    for s in &started {
        let closed = panes::lock(&panes).close(&s.pane).unwrap();
        assert!(
            panes::finish_close(&pm, closed).is_none(),
            "unchanged worktree kept"
        );
        assert!(!Path::new(&s.cwd).exists(), "{} still there", s.cwd);
    }
    pm.close_all();
}
