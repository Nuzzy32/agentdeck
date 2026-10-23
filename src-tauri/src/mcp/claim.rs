//! `pane_claim` (MCP-TOOLS, D-033): an agent marks the files or folders it is changing, so a sibling working in
//! parallel leaves them alone. Paths are relative to the caller's folder and must stay inside it, symlinks included.
//! A claim that overlaps (same path, or one inside the other) a claim of another live pane in the same workspace is
//! refused with that pane's id. Each call replaces the caller's claims; they go away when its process exits or the
//! pane closes (`panes.rs`), and `pane_list` shows them. Advisory: nothing stops a write, the agents keep to them.

use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

/// Paths per pane (MCP-TOOLS).
pub const CLAIMS_MAX: usize = 10;
const PATH_MAX_BYTES: usize = 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Args {
    pub paths: Vec<String>,
}

fn bad(why: &str) -> String {
    format!("BAD_ARGS: {why}. Fix the arguments.")
}

fn outside(path: &str, base: &Path) -> String {
    format!(
        "PATH_OUTSIDE: {path} is not inside your folder {}. Give paths relative to your folder, without \"..\".",
        base.display()
    )
}

/// `p` with the symlinks of its deepest existing ancestor resolved; the part that does not exist yet is appended as
/// given. `None` for a dangling symlink, whose target nobody can check.
fn real(p: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(found) = cur.canonicalize() {
            return Some(missing.iter().rev().fold(found, |acc, n| acc.join(n)));
        }
        if cur.symlink_metadata().is_ok() {
            return None;
        }
        missing.push(cur.file_name()?);
        cur = cur.parent()?;
    }
}

/// The claimed paths as absolute paths under `cwd` (resolved), without duplicates. `"."` claims the whole folder.
pub fn resolve(cwd: &Path, raw: &[String]) -> Result<Vec<PathBuf>, String> {
    if raw.len() > CLAIMS_MAX {
        return Err(bad("paths holds at most 10 entries"));
    }
    let base = cwd.canonicalize().map_err(|_| {
        "PATH_OUTSIDE: your own folder no longer exists, so nothing can be claimed in it. Tell the person.".to_owned()
    })?;
    let mut out: Vec<PathBuf> = Vec::new();
    for r in raw {
        if r.is_empty() || r.len() > PATH_MAX_BYTES || r.chars().any(char::is_control) {
            return Err(bad(
                "each path must be 1 to 1024 bytes without control characters",
            ));
        }
        let mut rel = PathBuf::new();
        for c in Path::new(r).components() {
            match c {
                Component::Normal(n) => rel.push(n),
                Component::CurDir => {}
                Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                    return Err(outside(r, &base));
                }
            }
        }
        let abs = real(&base.join(&rel)).filter(|a| a.starts_with(&base));
        let abs = abs.ok_or_else(|| outside(r, &base))?;
        if !out.contains(&abs) {
            out.push(abs);
        }
    }
    Ok(out)
}

/// Same path, or one inside the other (component-wise: `src/a` does not overlap `src/ab`).
pub fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// `CLAIMED` for the first of `mine` that overlaps a claim in `others` (pane id, its claims).
pub fn conflict<'a>(
    mine: &[PathBuf],
    others: impl IntoIterator<Item = (&'a str, &'a [PathBuf])>,
) -> Result<(), String> {
    for (pane, theirs) in others {
        for m in mine {
            if let Some(t) = theirs.iter().find(|t| overlaps(m, t)) {
                return Err(format!(
                    "CLAIMED: {} overlaps {}, claimed by {pane}. Leave those files to {pane}, or ask your parent \
                     to split the work.",
                    m.display(),
                    t.display()
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "agentdeck-claim-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("work/src")).unwrap();
        d.canonicalize().unwrap()
    }

    fn strs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    fn code(r: Result<Vec<PathBuf>, String>) -> String {
        let e = r.expect_err("expected an error");
        assert!(e.ends_with('.'), "{e}");
        e.split_once(": ").unwrap().0.to_owned()
    }

    #[test]
    fn relative_paths_resolve_inside_the_folder() {
        let d = tmp("ok");
        let work = d.join("work");
        let got = resolve(
            &work,
            &strs(&["src/a.rs", "./src/a.rs", "src/", "new/dir/file.rs", "."]),
        )
        .unwrap();
        assert_eq!(
            got,
            [
                work.join("src/a.rs"),
                work.join("src"),
                work.join("new/dir/file.rs"),
                work.clone()
            ]
        );
        assert_eq!(resolve(&work, &[]), Ok(vec![]), "an empty list releases");
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        let d = tmp("out");
        let work = d.join("work");
        std::fs::create_dir_all(d.join("outside")).unwrap();
        std::os::unix::fs::symlink(d.join("outside"), work.join("escape")).unwrap();
        std::os::unix::fs::symlink(d.join("gone"), work.join("dangling")).unwrap();
        std::os::unix::fs::symlink(work.join("src"), work.join("inner")).unwrap();
        for p in [
            "/etc/passwd",
            "../outside",
            "src/../../outside",
            "src/..",
            "escape/x.rs",
            "escape",
            "dangling",
            "dangling/x",
        ] {
            assert_eq!(code(resolve(&work, &strs(&[p]))), "PATH_OUTSIDE", "{p}");
        }
        // A symlink that stays inside resolves to its target.
        assert_eq!(
            resolve(&work, &strs(&["inner/a.rs"])),
            Ok(vec![work.join("src/a.rs")])
        );
        assert_eq!(
            code(resolve(&d.join("missing"), &strs(&["a"]))),
            "PATH_OUTSIDE"
        );
    }

    #[test]
    fn shape_and_count_are_checked() {
        let d = tmp("shape");
        let work = d.join("work");
        let eleven: Vec<String> = (0..11).map(|i| format!("f{i}")).collect();
        assert_eq!(code(resolve(&work, &eleven)), "BAD_ARGS");
        assert_eq!(resolve(&work, &eleven[..10]).unwrap().len(), 10);
        for p in ["", "a\nb", "a\0b", &"a".repeat(1025)] {
            assert_eq!(code(resolve(&work, &strs(&[p]))), "BAD_ARGS", "{p:?}");
        }
    }

    #[test]
    fn overlap_is_component_wise() {
        let p = |s: &str| PathBuf::from(s);
        assert!(overlaps(&p("/w/src"), &p("/w/src/a.rs")));
        assert!(overlaps(&p("/w/src/a.rs"), &p("/w/src")));
        assert!(overlaps(&p("/w/src"), &p("/w/src")));
        assert!(!overlaps(&p("/w/src/a"), &p("/w/src/ab")));
        assert!(!overlaps(&p("/w/src/a.rs"), &p("/w/src/b.rs")));
        let theirs = [p("/w/docs"), p("/w/src")];
        let e = conflict(&[p("/w/a.rs"), p("/w/src/x.rs")], [("p3", &theirs[..])]).unwrap_err();
        assert!(
            e.starts_with("CLAIMED: /w/src/x.rs overlaps /w/src, claimed by p3.")
                && e.ends_with('.'),
            "{e}"
        );
        assert_eq!(conflict(&[p("/w/lib")], [("p3", &theirs[..])]), Ok(()));
    }
}
