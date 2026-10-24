//! `pane_spawn { worktree: true }` (SPN-05, T4.1): the child gets its own `git worktree` at `<repo>/.worktrees/<name>`
//! on a new branch `agent/<name>`, started from the caller's `HEAD`. When the person closes the child, a worktree with no
//! changes (clean tree, no new commits) is removed together with its branch; one with changes stays for the person.
//!
//! git runs directly (no shell) with `GIT_*` variables of agentdeck's own environment removed, so a `GIT_DIR` inherited
//! from a dev session never points it at another repository. Hooks are off for the `worktree add` checkout: an agent's
//! request should not run the repository's hook scripts.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Folder under the repository root, kept out of `git status` through `.git/info/exclude`.
pub const DIR: &str = ".worktrees";
const BRANCH_PREFIX: &str = "agent/";
/// Variables that redirect git to another repository or index.
const GIT_ENV: [&str; 6] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_COMMON_DIR",
    "GIT_OBJECT_DIRECTORY",
    "GIT_CEILING_DIRECTORIES",
];

/// Saved in `layout.json` (T4.3) so a restored child still cleans up its worktree when closed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Worktree {
    /// Main working tree of the repository (canonical).
    pub repo: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    /// Commit the branch started from; new commits on the branch count as changes.
    pub base: String,
}

/// What closing the child did with its worktree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cleanup {
    Removed,
    /// Left in place: changes, or git refused to remove it (`why`).
    Kept {
        why: String,
    },
}

fn git(dir: &Path, args: &[&str]) -> Result<Output, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    for k in GIT_ENV {
        cmd.env_remove(k);
    }
    cmd.output()
        .map_err(|e| format!("git tidak bisa dijalankan: {e}"))
}

/// stdout of a git command that must succeed, trimmed.
fn git_ok(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = git(dir, args)?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("git {}: {}", args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Main working tree of the repository `dir` is in (also when `dir` is inside a linked worktree); `None` outside git.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    // The common dir is the main repository's `.git`; its parent is the main working tree.
    let common = git_ok(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let common = PathBuf::from(common);
    if common.file_name()? != ".git" {
        return None; // bare repository: no working tree to branch from
    }
    dunce::canonicalize(common.parent()?).ok()
}

fn branch_exists(repo: &Path, branch: &str) -> bool {
    git(
        repo,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok_and(|o| o.status.success())
}

/// Adds `.worktrees/` to `.git/info/exclude` (local to this clone, never committed) once.
fn exclude(repo: &Path) -> Result<(), String> {
    let path = PathBuf::from(git_ok(
        repo,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "info/exclude",
        ],
    )?);
    let line = format!("/{DIR}/");
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    if old.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let sep = if old.is_empty() || old.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    std::fs::write(&path, format!("{old}{sep}{line}\n")).map_err(|e| e.to_string())
}

/// Creates `<repo>/.worktrees/<name>` on branch `agent/<name>` from `HEAD`. A name already taken (a worktree kept from
/// an earlier run, whose pane ids started over) gets `-2`, `-3`, ...
pub fn create(repo: &Path, name: &str) -> Result<Worktree, String> {
    let base = git_ok(repo, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map_err(|_| "repositori belum punya commit".to_owned())?;
    exclude(repo)?;
    let root = repo.join(DIR);
    let (path, branch) = (1..)
        .map(|n| match n {
            1 => name.to_owned(),
            n => format!("{name}-{n}"),
        })
        .map(|n| (root.join(&n), format!("{BRANCH_PREFIX}{n}")))
        .find(|(p, b)| !p.exists() && !branch_exists(repo, b))
        .ok_or("nama worktree habis")?;
    let path_str = path.to_str().ok_or("path repositori bukan UTF-8")?;
    git_ok(
        repo,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "worktree",
            "add",
            "--quiet",
            "-b",
            &branch,
            path_str,
            &base,
        ],
    )?;
    Ok(Worktree {
        repo: repo.to_owned(),
        path: dunce::canonicalize(&path).unwrap_or(path),
        branch,
        base,
    })
}

/// A worktree read back from `layout.json` (T4.3) is trusted only while it is still exactly what [`create`] made:
/// `<repo>/.worktrees/<name>`, the top of its own checkout, on branch `agent/<name>`, with both paths canonical. Closing
/// a pane removes the worktree and deletes the branch, so a hand-edited file must not point that at anything else.
pub fn still_ours(wt: &Worktree) -> bool {
    let (Ok(repo), Ok(path)) = (dunce::canonicalize(&wt.repo), dunce::canonicalize(&wt.path))
    else {
        return false;
    };
    let name = path.file_name().and_then(|n| n.to_str());
    repo == wt.repo
        && path == wt.path
        && path.parent() == Some(repo.join(DIR).as_path())
        && name.is_some_and(|n| wt.branch == format!("{BRANCH_PREFIX}{n}"))
        && git_ok(&path, &["rev-parse", "--show-toplevel"]).is_ok_and(|t| Path::new(&t) == path)
        && git_ok(&path, &["symbolic-ref", "--short", "HEAD"]).is_ok_and(|b| b == wt.branch)
        && repo_root(&path).is_some_and(|r| r == repo)
}

/// Uncommitted or untracked files, or commits on the branch since it started.
pub fn changed(wt: &Worktree) -> Result<bool, String> {
    let status = git_ok(&wt.path, &["status", "--porcelain"])?;
    let commits = git_ok(
        &wt.repo,
        &[
            "rev-list",
            "--count",
            &format!("{}..{}", wt.base, wt.branch),
        ],
    )?;
    Ok(!status.is_empty() || commits != "0")
}

/// Called after the child's process stopped. Removes an unchanged worktree and its branch; keeps anything else.
pub fn cleanup(wt: &Worktree) -> Cleanup {
    match changed(wt) {
        Ok(true) => {
            return Cleanup::Kept {
                why: "ada perubahan".into(),
            };
        }
        Ok(false) => {}
        // The folder is gone (removed by hand): just forget the worktree, the branch has no new commits to lose.
        Err(_) if !wt.path.exists() => {
            let _ = git(&wt.repo, &["worktree", "prune"]);
        }
        Err(e) => return Cleanup::Kept { why: e },
    }
    if wt.path.exists() {
        let Some(path) = wt.path.to_str() else {
            return Cleanup::Kept {
                why: "path bukan UTF-8".into(),
            };
        };
        // No --force: git refuses a worktree that changed after the check above.
        if let Err(e) = git_ok(&wt.repo, &["worktree", "remove", path]) {
            return Cleanup::Kept { why: e };
        }
    }
    match git_ok(&wt.repo, &["branch", "-D", &wt.branch]) {
        Ok(_) => Cleanup::Removed,
        Err(e) => Cleanup::Kept { why: e },
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh repository with one commit, in its own temp folder.
    pub(crate) fn repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agentdeck-wt-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        for args in [
            &["init", "--quiet", "-b", "main"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
            &["config", "commit.gpgsign", "false"],
        ] {
            git_ok(&dir, args).unwrap();
        }
        std::fs::write(dir.join("src/a.txt"), "a\n").unwrap();
        git_ok(&dir, &["add", "."]).unwrap();
        git_ok(&dir, &["commit", "--quiet", "-m", "init"]).unwrap();
        dunce::canonicalize(&dir).unwrap()
    }

    #[test]
    fn create_clean_removed_on_cleanup() {
        let r = repo("clean");
        assert_eq!(repo_root(&r.join("src")), Some(r.clone()));
        let wt = create(&r, "p2").unwrap();
        assert_eq!(wt.path, r.join(".worktrees/p2"));
        assert_eq!(wt.branch, "agent/p2");
        assert!(wt.path.join("src/a.txt").is_file());
        // The main tree does not see the worktree folder; exclude is written once.
        assert_eq!(git_ok(&r, &["status", "--porcelain"]).unwrap(), "");
        let ex = std::fs::read_to_string(r.join(".git/info/exclude")).unwrap();
        assert_eq!(ex.matches("/.worktrees/").count(), 1);
        // Inside the worktree, repo_root is still the main tree.
        assert_eq!(repo_root(&wt.path), Some(r.clone()));
        assert_eq!(changed(&wt), Ok(false));
        assert_eq!(cleanup(&wt), Cleanup::Removed);
        assert!(!wt.path.exists());
        assert!(!branch_exists(&r, "agent/p2"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn only_an_unchanged_record_of_our_worktree_is_trusted() {
        let r = repo("ours");
        let wt = create(&r, "p4").unwrap();
        assert!(still_ours(&wt));
        git_ok(&r, &["branch", "feature-x"]).unwrap();
        let bad = [
            Worktree {
                branch: "feature-x".into(),
                ..wt.clone()
            },
            Worktree {
                branch: "agent/p5".into(),
                ..wt.clone()
            },
            Worktree {
                path: r.join(DIR),
                ..wt.clone()
            },
            Worktree {
                path: wt.path.join("src"),
                ..wt.clone()
            },
            Worktree {
                path: r.clone(),
                branch: "agent/".into(),
                ..wt.clone()
            },
            Worktree {
                repo: r.join("src"),
                ..wt.clone()
            },
            Worktree {
                path: r.join(".worktrees/p9"),
                branch: "agent/p9".into(),
                ..wt.clone()
            },
        ];
        for b in &bad {
            assert!(!still_ours(b), "{b:?} trusted");
        }
        // Its checkout moved to another branch: no longer ours to delete.
        git_ok(&wt.path, &["checkout", "--quiet", "-b", "agent/other"]).unwrap();
        assert!(!still_ours(&wt));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn changed_worktree_is_kept() {
        let r = repo("dirty");
        // Edited file.
        let a = create(&r, "p2").unwrap();
        std::fs::write(a.path.join("src/a.txt"), "b\n").unwrap();
        // Untracked file.
        let b = create(&r, "p3").unwrap();
        std::fs::write(b.path.join("new.txt"), "x").unwrap();
        // Clean tree, but a commit on the branch.
        let c = create(&r, "p4").unwrap();
        std::fs::write(c.path.join("src/a.txt"), "c\n").unwrap();
        git_ok(&c.path, &["commit", "--quiet", "-am", "work"]).unwrap();
        for wt in [&a, &b, &c] {
            assert_eq!(changed(wt), Ok(true), "{}", wt.branch);
            assert!(matches!(cleanup(wt), Cleanup::Kept { .. }));
            assert!(wt.path.exists() && branch_exists(&r, &wt.branch));
        }
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn taken_name_gets_suffix_and_removed_folder_is_forgotten() {
        let r = repo("suffix");
        let a = create(&r, "p2").unwrap();
        let b = create(&r, "p2").unwrap();
        assert_eq!(
            (b.path.file_name().unwrap(), b.branch.as_str()),
            (std::ffi::OsStr::new("p2-2"), "agent/p2-2")
        );
        std::fs::remove_dir_all(&a.path).unwrap();
        assert_eq!(cleanup(&a), Cleanup::Removed);
        assert!(!branch_exists(&r, "agent/p2"));
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn not_a_repo_or_no_commit() {
        let dir = std::env::temp_dir().join(format!("agentdeck-wt-none-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(repo_root(&dir), None);
        git_ok(&dir, &["init", "--quiet"]).unwrap();
        let root = repo_root(&dir).unwrap();
        assert!(
            create(&root, "p2")
                .unwrap_err()
                .contains("belum punya commit")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
