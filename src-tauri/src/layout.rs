//! `layout.json` in the app data directory (ARCHITECTURE §8, PRD TERM-06): workspaces, their split trees and their
//! panes, so the next run opens the same layout. No process comes back: every restored pane is registered as exited
//! and waits for the person (D-025). The file is read once in `setup`, before the WebView asks; a missing, unreadable
//! or invalid file gives an empty layout and a log line, and is left alone until the next save. Pane and workspace
//! ids are kept, so the counters continue above them and an id is still never reused within a run. Worktrees come
//! from the registry, never from the WebView, and are only re-attached when they are still a worktree of their repo.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::panes::{self, Panes};
use crate::usage;
use crate::worktree::{self, Worktree};

pub const FILE: &str = "layout.json";
const VERSION: u32 = 1;
pub const MAX_WORKSPACES: usize = 16;
pub const MAX_PANES: usize = 64;
const TEXT_MAX: usize = 200;
const PATH_MAX: usize = 4096;
/// Deeper than any split a 64-pane layout needs; stops a hand-made file from nesting without end.
const DEPTH_MAX: usize = 32;
/// Highest pane or workspace number accepted, far above any real run; the counters continue from it without overflow.
pub const ID_MAX: u32 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub version: u32,
    /// Workspace shown on open; `None` when there is none.
    pub active: Option<u32>,
    pub workspaces: Vec<SavedWorkspace>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedWorkspace {
    pub id: u32,
    pub name: String,
    pub folder: String,
    pub tree: Tree,
    pub focused: String,
    pub panes: Vec<SavedPane>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedPane {
    pub id: String,
    /// Catalogue id; `None` for a shell.
    pub agent: Option<String>,
    /// Display name of the agent, so the header reads right before the agent list loads.
    pub name: String,
    pub title: String,
    pub cwd: String,
    pub parent: Option<String>,
    /// Filled in from the registry on save; whatever the WebView sends here is replaced.
    #[serde(default)]
    pub worktree: Option<Worktree>,
    /// Claude session to resume (D-030). Filled in from the registry on save, like `worktree`.
    #[serde(default)]
    pub session: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dir {
    Row,
    Col,
}

/// Same shape as `tree.ts`: a leaf `{pane}` or a split `{dir, ratio, a, b}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Tree {
    Leaf {
        pane: String,
    },
    Split {
        dir: Dir,
        ratio: f64,
        a: Box<Tree>,
        b: Box<Tree>,
    },
}

impl Tree {
    /// Leaf ids in order, or the reason the tree is refused.
    fn leaves(&self, depth: usize, out: &mut Vec<String>) -> Result<(), String> {
        if depth > DEPTH_MAX {
            return Err("pohon split terlalu dalam".into());
        }
        match self {
            Tree::Leaf { pane } => out.push(pane.clone()),
            Tree::Split { ratio, a, b, .. } => {
                if !(ratio.is_finite() && *ratio > 0.0 && *ratio < 1.0) {
                    return Err(format!("rasio split {ratio} di luar 0–1"));
                }
                a.leaves(depth + 1, out)?;
                b.leaves(depth + 1, out)?;
            }
        }
        Ok(())
    }
}

fn text(what: &str, s: &str, max: usize) -> Result<(), String> {
    if s.chars().count() > max || s.chars().any(char::is_control) {
        return Err(format!(
            "{what} terlalu panjang atau berisi karakter kontrol"
        ));
    }
    Ok(())
}

fn path(what: &str, s: &str) -> Result<(), String> {
    text(what, s, PATH_MAX)?;
    if !Path::new(s).is_absolute() {
        return Err(format!("{what} bukan path absolut: {s}"));
    }
    Ok(())
}

/// `pN` with N ≥ 1, the only ids the registry gives.
fn pane_number(id: &str) -> Option<u32> {
    let n = id.strip_prefix('p')?;
    if n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|&n| n > 0 && n <= ID_MAX)
}

impl Layout {
    pub fn empty() -> Self {
        Self {
            version: VERSION,
            active: None,
            workspaces: Vec::new(),
        }
    }

    /// Refuses anything the registry could not hold as it is (sizes, ids, paths, a tree that does not match its
    /// panes). Mends what only points at something missing: `focused`, `parent` and `active` fall back.
    pub fn validated(mut self) -> Result<Self, String> {
        if self.version != VERSION {
            return Err(format!("versi {} tidak dikenal", self.version));
        }
        if self.workspaces.len() > MAX_WORKSPACES {
            return Err(format!("lebih dari {MAX_WORKSPACES} workspace"));
        }
        let total: usize = self.workspaces.iter().map(|w| w.panes.len()).sum();
        if total > MAX_PANES {
            return Err(format!("lebih dari {MAX_PANES} pane"));
        }
        let mut ws_ids = HashSet::new();
        let mut pane_ids = HashSet::new();
        for w in &mut self.workspaces {
            if w.id == 0 || w.id > ID_MAX || !ws_ids.insert(w.id) {
                return Err(format!("id workspace {} ganda atau nol", w.id));
            }
            text("nama workspace", &w.name, TEXT_MAX)?;
            path("folder workspace", &w.folder)?;
            let mut leaves = Vec::new();
            w.tree.leaves(0, &mut leaves)?;
            if leaves.is_empty() || leaves.len() != w.panes.len() {
                return Err(format!(
                    "pohon workspace {} tidak cocok dengan panenya",
                    w.id
                ));
            }
            let own: HashSet<&str> = w.panes.iter().map(|p| p.id.as_str()).collect();
            if own.len() != w.panes.len() || leaves.iter().any(|l| !own.contains(l.as_str())) {
                return Err(format!(
                    "pohon workspace {} tidak cocok dengan panenya",
                    w.id
                ));
            }
            for p in &w.panes {
                if pane_number(&p.id).is_none() || !pane_ids.insert(p.id.clone()) {
                    return Err(format!("id pane {} tidak sah atau ganda", p.id));
                }
                if let Some(a) = &p.agent {
                    text("id agen", a, TEXT_MAX)?;
                }
                text("nama agen", &p.name, TEXT_MAX)?;
                text("judul pane", &p.title, TEXT_MAX)?;
                path("folder pane", &p.cwd)?;
            }
            if !own.contains(w.focused.as_str()) {
                w.focused = leaves[0].clone();
            }
        }
        // A child whose parent is not restored becomes a pane of its own; a parent in another workspace never
        // happens (`pane_spawn` keeps the workspace) and is dropped the same way.
        for w in &mut self.workspaces {
            let own: HashSet<String> = w.panes.iter().map(|p| p.id.clone()).collect();
            for p in &mut w.panes {
                if p.parent
                    .as_ref()
                    .is_some_and(|q| !own.contains(q) || *q == p.id)
                {
                    p.parent = None;
                }
            }
        }
        if self.active.is_none_or(|a| !ws_ids.contains(&a)) {
            self.active = self.workspaces.first().map(|w| w.id);
        }
        Ok(self)
    }
}

/// Reads `path`. Missing → empty; unreadable or invalid → empty and the warning to log.
pub fn load(path: &Path) -> (Layout, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Layout::empty(), None),
        Err(e) => return (Layout::empty(), Some(format!("{FILE} tidak terbaca: {e}"))),
    };
    match serde_json::from_str::<Layout>(&text)
        .map_err(|e| e.to_string())
        .and_then(Layout::validated)
    {
        Ok(mut l) => {
            for p in l.workspaces.iter_mut().flat_map(|w| w.panes.iter_mut()) {
                if p.worktree
                    .as_ref()
                    .is_some_and(|wt| !worktree::still_ours(wt))
                {
                    p.worktree = None;
                }
                if p.session.as_deref().is_some_and(|id| !usage::is_uuid(id)) {
                    p.session = None;
                }
            }
            (l, None)
        }
        Err(e) => (
            Layout::empty(),
            Some(format!("{FILE} rusak, mulai kosong: {e}")),
        ),
    }
}

/// Write-then-rename, so a crash leaves either the old file or the new one, never half of it.
pub fn save(path: &Path, l: &Layout) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut json = serde_json::to_vec_pretty(l)?;
    json.push(b'\n');
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&json)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

/// The file of this run and the layout read at startup, which the WebView takes once.
#[derive(Debug)]
pub struct Store {
    /// `None` without an app data dir: the layout is not kept.
    path: Option<PathBuf>,
    restored: Mutex<Option<Layout>>,
    /// Last layout the WebView saved, written once more on exit ([`Self::save_last`]).
    last: Mutex<Option<Layout>>,
    /// Saves come from async commands; one write at a time keeps the temporary file theirs alone.
    writing: Mutex<()>,
}

impl Store {
    /// Loads the file and registers its workspaces and panes, all exited, before anything else opens.
    pub fn open(data_dir: Option<&Path>, panes: &Panes) -> Self {
        let path = data_dir.map(|d| d.join(FILE));
        let (layout, warning) = path.as_deref().map_or((Layout::empty(), None), load);
        if let Some(w) = warning {
            eprintln!("agentdeck: {w}");
        }
        panes::lock(panes).restore(&layout);
        Self {
            path,
            restored: Mutex::new(Some(layout)),
            last: Mutex::new(None),
            writing: Mutex::new(()),
        }
    }

    pub fn take(&self) -> Option<Layout> {
        self.restored
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Validates, takes each pane's worktree from the registry, writes.
    pub fn save(&self, layout: Layout, panes: &Panes) -> Result<(), String> {
        let mut layout = layout.validated()?;
        *self.last.lock().unwrap_or_else(PoisonError::into_inner) = Some(layout.clone());
        {
            let reg = panes::lock(panes);
            for p in layout
                .workspaces
                .iter_mut()
                .flat_map(|w| w.panes.iter_mut())
            {
                p.worktree = reg.worktree_of(&p.id);
                p.session = reg.session_of(&p.id);
            }
        }
        let Some(path) = &self.path else {
            return Ok(());
        };
        let _one = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        save(path, &layout).map_err(|e| format!("{FILE} tidak bisa disimpan: {e}"))
    }

    /// On exit, before the panes stop: the WebView saves only when the layout changes, but a Claude session id
    /// (D-030) appears after the first turn, so the last layout is written again with the sessions of now.
    // ponytail: a crash loses sessions that appeared since the last change; save on each statusline if that bites.
    pub fn save_last(&self, panes: &Panes) -> Result<(), String> {
        let last = self
            .last
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        last.map_or(Ok(()), |l| self.save(l, panes))
    }
}

/// The layout read at startup, once; `null` afterwards (StrictMode's second effect, a WebView reload).
#[tauri::command]
pub async fn layout_restore(store: State<'_, Store>) -> Result<Option<Layout>, String> {
    Ok(store.take())
}

#[tauri::command]
pub async fn layout_save(
    store: State<'_, Store>,
    panes: State<'_, Panes>,
    layout: Layout,
) -> Result<(), String> {
    store.save(layout, &panes)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);

    impl Dir {
        fn new(name: &str) -> Self {
            let d = std::env::temp_dir()
                .join(format!("agentdeck-layout-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).ok();
            Self(d)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn pane(id: &str, parent: Option<&str>) -> SavedPane {
        SavedPane {
            id: id.into(),
            agent: Some("claude-code".into()),
            name: "Claude Code".into(),
            title: "feat: auth".into(),
            cwd: "/Users/u/app".into(),
            parent: parent.map(Into::into),
            worktree: None,
            session: None,
        }
    }

    fn leaf(id: &str) -> Tree {
        Tree::Leaf { pane: id.into() }
    }

    fn two() -> Layout {
        Layout {
            version: 1,
            active: Some(3),
            workspaces: vec![SavedWorkspace {
                id: 3,
                name: "app".into(),
                folder: "/Users/u/app".into(),
                tree: Tree::Split {
                    dir: super::Dir::Row,
                    ratio: 0.5,
                    a: Box::new(leaf("p1")),
                    b: Box::new(leaf("p4")),
                },
                focused: "p4".into(),
                panes: vec![pane("p1", None), pane("p4", Some("p1"))],
            }],
        }
    }

    #[test]
    fn same_shape_as_the_webview() {
        let json = r#"{"version":1,"active":3,"workspaces":[{"id":3,"name":"app","folder":"/Users/u/app",
            "tree":{"dir":"row","ratio":0.5,"a":{"pane":"p1"},"b":{"pane":"p4"}},"focused":"p4",
            "panes":[{"id":"p1","agent":"claude-code","name":"Claude Code","title":"feat: auth","cwd":"/Users/u/app",
            "parent":null},{"id":"p4","agent":"claude-code","name":"Claude Code","title":"feat: auth",
            "cwd":"/Users/u/app","parent":"p1"}]}]}"#;
        let l: Layout = serde_json::from_str(json).expect("parses");
        assert_eq!(l, two());
    }

    #[test]
    fn round_trip_and_missing_file() {
        let d = Dir::new("round");
        let f = d.0.join(FILE);
        assert_eq!(load(&f), (Layout::empty(), None));
        save(&f, &two()).expect("saved");
        assert_eq!(load(&f), (two(), None));
        assert!(!d.0.join("layout.json.tmp").exists());
    }

    #[test]
    fn broken_or_old_file_starts_empty_with_a_warning() {
        let d = Dir::new("broken");
        let f = d.0.join(FILE);
        for text in [
            "{not json",
            r#"{"version":2,"active":null,"workspaces":[]}"#,
        ] {
            fs::write(&f, text).expect("written");
            let (l, warning) = load(&f);
            assert_eq!(l, Layout::empty());
            assert!(warning.is_some_and(|w| w.contains("rusak")));
        }
    }

    #[test]
    fn refuses_what_the_registry_cannot_hold() {
        type Change = Box<dyn Fn(&mut Layout)>;
        let bad: Vec<(&str, Change)> = vec![
            (
                "leaf twice",
                Box::new(|l| l.workspaces[0].panes[1].id = "p1".into()),
            ),
            (
                "unknown leaf",
                Box::new(|l| l.workspaces[0].tree = leaf("p9")),
            ),
            (
                "pane not in tree",
                Box::new(|l| l.workspaces[0].panes.push(pane("p7", None))),
            ),
            (
                "relative cwd",
                Box::new(|l| l.workspaces[0].panes[0].cwd = "app".into()),
            ),
            (
                "relative folder",
                Box::new(|l| l.workspaces[0].folder = "~/app".into()),
            ),
            (
                "bad id",
                Box::new(|l| l.workspaces[0].panes[0].id = "p01".into()),
            ),
            ("zero id", Box::new(|l| l.workspaces[0].id = 0)),
            (
                "huge workspace id",
                Box::new(|l| l.workspaces[0].id = u32::MAX),
            ),
            (
                "huge pane id",
                Box::new(|l| l.workspaces[0].panes[0].id = "p4294967295".into()),
            ),
            (
                "long title",
                Box::new(|l| l.workspaces[0].panes[0].title = "x".repeat(201)),
            ),
            (
                "control char",
                Box::new(|l| l.workspaces[0].name = "a\u{1b}[2J".into()),
            ),
            (
                "NaN ratio",
                Box::new(|l| {
                    if let Tree::Split { ratio, .. } = &mut l.workspaces[0].tree {
                        *ratio = f64::NAN;
                    }
                }),
            ),
            (
                "too many workspaces",
                Box::new(|l| {
                    let w = l.workspaces[0].clone();
                    l.workspaces = (1..=17)
                        .map(|id| SavedWorkspace { id, ..w.clone() })
                        .collect();
                }),
            ),
        ];
        for (what, change) in bad {
            let mut l = two();
            change(&mut l);
            assert!(l.validated().is_err(), "{what} accepted");
        }
        // 65 panes over several workspaces.
        let mut l = Layout::empty();
        for w in 0..5u32 {
            let ids: Vec<String> = (0..13).map(|k| format!("p{}", w * 13 + k + 1)).collect();
            let tree = ids.iter().skip(1).fold(leaf(&ids[0]), |t, id| Tree::Split {
                dir: super::Dir::Col,
                ratio: 0.5,
                a: Box::new(t),
                b: Box::new(leaf(id)),
            });
            l.workspaces.push(SavedWorkspace {
                id: w + 1,
                name: "w".into(),
                folder: "/w".into(),
                tree,
                focused: ids[0].clone(),
                panes: ids.iter().map(|id| pane(id, None)).collect(),
            });
        }
        assert!(l.validated().is_err());
    }

    #[test]
    fn mends_dangling_focus_parent_and_active() {
        let mut l = two();
        l.active = Some(99);
        l.workspaces[0].focused = "p9".into();
        l.workspaces[0].panes[0].parent = Some("p8".into());
        let l = l.validated().expect("valid");
        assert_eq!(l.active, Some(3));
        assert_eq!(l.workspaces[0].focused, "p1");
        assert_eq!(l.workspaces[0].panes[0].parent, None);
        assert_eq!(l.workspaces[0].panes[1].parent.as_deref(), Some("p1"));
        assert_eq!(Layout::empty().validated().expect("valid").active, None);
    }

    #[test]
    fn a_worktree_that_is_gone_is_not_reattached() {
        let d = Dir::new("wt");
        let f = d.0.join(FILE);
        let mut l = two();
        l.workspaces[0].panes[1].worktree = Some(Worktree {
            repo: d.0.clone(),
            path: d.0.join(".worktrees/p4"),
            branch: "agent/p4".into(),
            base: "abc".into(),
        });
        save(&f, &l).expect("saved");
        let (got, _) = load(&f);
        assert_eq!(got.workspaces[0].panes[1].worktree, None);
    }

    /// D-030: a session that is not a UUID never comes back from the file (it would become an argv element); one the
    /// WebView sends is replaced by the registry's on save.
    #[test]
    fn sessions_are_uuids_and_come_from_the_registry() {
        let d = Dir::new("session");
        let f = d.0.join(FILE);
        let id = "3fe10b28-c0b7-47aa-8cb4-dfe4947debd1";
        let mut l = two();
        l.workspaces[0].panes[0].session = Some(id.into());
        l.workspaces[0].panes[1].session = Some("--dangerously-skip-permissions".into());
        save(&f, &l).expect("saved");
        let (got, _) = load(&f);
        assert_eq!(got.workspaces[0].panes[0].session.as_deref(), Some(id));
        assert_eq!(got.workspaces[0].panes[1].session, None);

        let reg = Panes::default();
        let store = Store::open(Some(&d.0), &reg);
        let mut planted = two();
        planted.workspaces[0].panes[1].session = Some(id.into());
        store.save(planted, &reg).expect("saved");
        let (again, _) = load(&f);
        assert_eq!(
            again.workspaces[0].panes[0].session.as_deref(),
            Some(id),
            "restored, not used yet"
        );
        assert_eq!(
            again.workspaces[0].panes[1].session, None,
            "the WebView cannot plant one"
        );
        // On exit the last layout is written again: what the registry knows now is kept.
        fs::remove_file(&f).expect("removed");
        store.save_last(&reg).expect("saved again");
        assert_eq!(
            load(&f).0.workspaces[0].panes[0].session.as_deref(),
            Some(id)
        );
    }

    #[test]
    fn store_registers_exited_panes_and_saves_without_tokens() {
        let d = Dir::new("store");
        save(&d.0.join(FILE), &two()).expect("saved");
        let reg = Panes::default();
        let store = Store::open(Some(&d.0), &reg);
        assert_eq!(store.take(), Some(two()));
        assert_eq!(store.take(), None);
        assert!(!panes::lock(&reg).running("p1"));
        assert_eq!(panes::lock(&reg).workspace_of("p4"), Some(3));
        // The WebView cannot plant a worktree; the file holds no token or status.
        let mut l = two();
        l.workspaces[0].panes[0].worktree = Some(Worktree {
            repo: "/".into(),
            path: "/".into(),
            branch: "b".into(),
            base: "c".into(),
        });
        store.save(l, &reg).expect("saved");
        let text = fs::read_to_string(d.0.join(FILE)).expect("read");
        assert!(!text.contains("token") && !text.contains("status"));
        assert_eq!(load(&d.0.join(FILE)).0, two());
    }
}
