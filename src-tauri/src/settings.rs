//! `settings.json` in the app data directory (ARCHITECTURE §8, PRD SET-01..02, TERM-07). One process-wide store, so
//! `pane_spawn` and the approval cards read the current switches and limits when they need them
//! ([`spawn_enabled`], [`approval_timeout`], [`limits`]). Every value is validated here; the UI only mirrors the
//! ranges. A missing file gives the defaults; an unreadable or invalid one gives the defaults and a warning, and is
//! left alone until the next change. Writes go to a temporary file that is renamed over the old one.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::panes::Registry;
use crate::policy::{Limit, Limits};

pub const FILE: &str = "settings.json";
pub const FONT_SIZE: std::ops::RangeInclusive<u32> = 10..=24;
pub const LINE_HEIGHT: std::ops::RangeInclusive<f64> = 1.0..=2.0;
pub const SCROLLBACK: std::ops::RangeInclusive<u32> = 0..=100_000;
const FONT_FAMILY_MAX: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "Agen boleh membuka pane" (SET-01), off by default.
    pub spawn_enabled: bool,
    /// "Saling kirim" of a new workspace.
    pub peer_default: bool,
    pub limits: LimitValues,
    pub terminal: Terminal,
}

/// MCP-TOOLS "Batas bawaan"; ranges are [`Limit::range`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LimitValues {
    pub children: u32,
    pub global: u32,
    pub spawn_rate: u32,
    pub approval_timeout_s: u32,
    pub loop_guard: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Dark,
    Light,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    pub font_family: String,
    pub font_size: u32,
    pub line_height: f64,
    pub scrollback: u32,
    pub theme: Theme,
    pub screen_reader: bool,
}

impl Default for LimitValues {
    fn default() -> Self {
        Self::from(&Limits::default())
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            font_family: r#""SF Mono", Menlo, monospace"#.into(), // --font-mono (UI-UX §6)
            font_size: 13,
            line_height: 1.2,
            scrollback: 10_000,
            theme: Theme::System,
            screen_reader: false,
        }
    }
}

impl From<&Limits> for LimitValues {
    fn from(l: &Limits) -> Self {
        Self {
            children: l.get(Limit::ChildrenPerPane),
            global: l.get(Limit::AgentOpenedGlobal),
            spawn_rate: l.get(Limit::SpawnsPerWindow),
            approval_timeout_s: l.get(Limit::ApprovalTimeoutSecs),
            loop_guard: l.get(Limit::LoopGuard),
        }
    }
}

impl LimitValues {
    fn pairs(&self) -> [(Limit, u32); 5] {
        [
            (Limit::ChildrenPerPane, self.children),
            (Limit::AgentOpenedGlobal, self.global),
            (Limit::SpawnsPerWindow, self.spawn_rate),
            (Limit::ApprovalTimeoutSecs, self.approval_timeout_s),
            (Limit::LoopGuard, self.loop_guard),
        ]
    }
}

/// Settings › Kolaborasi labels, used in validation errors.
fn label(limit: Limit) -> &'static str {
    match limit {
        Limit::ChildrenPerPane => "Anak hidup per pane",
        Limit::AgentOpenedGlobal => "Pane agen dibuka agen",
        Limit::SpawnsPerWindow => "Spawn per 10 menit",
        Limit::ApprovalTimeoutSecs => "Batas waktu kartu persetujuan (detik)",
        Limit::LoopGuard => "Pengaman putaran",
    }
}

impl Settings {
    /// Limits for policy; out-of-range values cannot get here past [`Settings::validate`].
    pub fn limits(&self) -> Limits {
        let mut l = Limits::default();
        for (limit, v) in self.limits.pairs() {
            let _ = l.set(limit, v);
        }
        l
    }

    /// The first value outside its range, as a sentence for the person.
    pub fn validate(&self) -> Result<(), String> {
        for (limit, v) in self.limits.pairs() {
            let r = limit.range();
            if !r.contains(&v) {
                return Err(format!(
                    "{} harus {}–{} (diberi {v}).",
                    label(limit),
                    r.start(),
                    r.end()
                ));
            }
        }
        let t = &self.terminal;
        let family = t.font_family.trim();
        if family.is_empty()
            || family.len() > FONT_FAMILY_MAX
            || family.chars().any(char::is_control)
        {
            return Err(format!(
                "Font terminal harus berisi 1–{FONT_FAMILY_MAX} karakter."
            ));
        }
        if !FONT_SIZE.contains(&t.font_size) {
            return Err(format!(
                "Ukuran font harus {}–{} (diberi {}).",
                FONT_SIZE.start(),
                FONT_SIZE.end(),
                t.font_size
            ));
        }
        if !LINE_HEIGHT.contains(&t.line_height) {
            return Err(format!(
                "Tinggi baris harus {}–{} (diberi {}).",
                LINE_HEIGHT.start(),
                LINE_HEIGHT.end(),
                t.line_height
            ));
        }
        if !SCROLLBACK.contains(&t.scrollback) {
            return Err(format!(
                "Scrollback harus {}–{} (diberi {}).",
                SCROLLBACK.start(),
                SCROLLBACK.end(),
                t.scrollback
            ));
        }
        Ok(())
    }

    /// `patch` is a partial `Settings` object; unknown keys and wrong types are refused, nothing changes on error.
    pub fn patched(&self, patch: Value) -> Result<Self, String> {
        let mut v = serde_json::to_value(self).map_err(|e| e.to_string())?;
        merge(&mut v, patch, "")?;
        let next: Self =
            serde_json::from_value(v).map_err(|e| format!("Nilai pengaturan tidak valid: {e}"))?;
        next.validate()?;
        Ok(next)
    }
}

fn merge(dst: &mut Value, patch: Value, at: &str) -> Result<(), String> {
    let (Value::Object(dst), Value::Object(patch)) = (dst, patch) else {
        return Err(format!(
            "Pengaturan {} harus berupa objek.",
            if at.is_empty() { "(akar)" } else { at }
        ));
    };
    for (key, val) in patch {
        let path = if at.is_empty() {
            key.clone()
        } else {
            format!("{at}.{key}")
        };
        let slot = dst
            .get_mut(&key)
            .ok_or_else(|| format!("Pengaturan tidak dikenal: {path}."))?;
        if slot.is_object() {
            merge(slot, val, &path)?;
        } else {
            *slot = val;
        }
    }
    Ok(())
}

/// Reads `path`. Missing → defaults; unreadable or invalid → defaults and the warning to show.
pub fn load(path: &Path) -> (Settings, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return (Settings::default(), None),
        Err(e) => {
            return (
                Settings::default(),
                Some(format!("{FILE} tidak terbaca: {e}")),
            );
        }
    };
    let parsed = serde_json::from_str::<Settings>(&text)
        .map_err(|e| e.to_string())
        .and_then(|s| s.validate().map(|()| s));
    match parsed {
        Ok(s) => (s, None),
        Err(e) => (
            Settings::default(),
            Some(format!("{FILE} rusak, memakai bawaan: {e}")),
        ),
    }
}

/// Write-then-rename, so a crash leaves either the old file or the new one, never half of it.
pub fn save(path: &Path, s: &Settings) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let mut json = serde_json::to_vec_pretty(s)?;
    json.push(b'\n');
    let mut f = fs::File::create(&tmp)?;
    f.write_all(&json)?;
    f.sync_all()?;
    fs::rename(&tmp, path)
}

/// The settings of this run. `path: None` (no app data dir) keeps changes in memory only.
#[derive(Debug)]
pub struct Store {
    path: Option<PathBuf>,
    cur: Mutex<Settings>,
}

impl Store {
    pub fn open(data_dir: Option<&Path>) -> (Self, Option<String>) {
        let path = data_dir.map(|d| d.join(FILE));
        let (s, warning) = path.as_deref().map_or((Settings::default(), None), load);
        (
            Self {
                path,
                cur: Mutex::new(s),
            },
            warning,
        )
    }

    pub fn get(&self) -> Settings {
        self.cur
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Validates, writes, then takes effect; a refused patch or a failed write changes nothing.
    pub fn set(&self, patch: Value) -> Result<Settings, String> {
        let mut cur = self.cur.lock().unwrap_or_else(PoisonError::into_inner);
        let next = cur.patched(patch)?;
        if let Some(path) = &self.path {
            save(path, &next).map_err(|e| format!("{FILE} tidak bisa disimpan: {e}"))?;
        }
        *cur = next.clone();
        Ok(next)
    }
}

static STORE: OnceLock<Store> = OnceLock::new();

/// Called once at startup, before any reader; returns the warning for a broken file.
pub fn init(data_dir: Option<&Path>) -> Option<String> {
    let (store, warning) = Store::open(data_dir);
    let _ = STORE.set(store);
    warning
}

/// Current settings; the defaults before [`init`].
pub fn get() -> Settings {
    STORE.get().map(Store::get).unwrap_or_default()
}

pub fn set(patch: Value) -> Result<Settings, String> {
    STORE
        .get()
        .ok_or_else(|| "Pengaturan belum siap.".to_owned())?
        .set(patch)
}

/// "Agen boleh membuka pane" (SET-01), read by `pane_spawn`.
pub fn spawn_enabled() -> bool {
    get().spawn_enabled
}

/// Countdown of a spawn approval card (UI-UX §4.2).
pub fn approval_timeout() -> Duration {
    Duration::from_secs(u64::from(get().limits.approval_timeout_s))
}

pub fn limits() -> Limits {
    get().limits()
}

/// Makes the registry's copies current: limits (loop guard, spawn caps) and the peer switch of new workspaces.
pub fn apply(s: &Settings, reg: &mut Registry) {
    reg.limits = s.limits();
    reg.peer_default = s.peer_default;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::{Pane, PaneKind, PolicyError, SpawnRate, check_spawn};
    use serde_json::json;
    use std::time::Instant;

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir()
                .join(format!("agentdeck-settings-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            Self(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn ok<T, E: std::fmt::Display>(r: Result<T, E>) -> T {
        r.unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn defaults_match_the_docs() {
        let s = Settings::default();
        assert!(!s.spawn_enabled && !s.peer_default);
        assert_eq!(
            serde_json::to_value(&s).ok(),
            Some(json!({
                "spawn_enabled": false,
                "peer_default": false,
                "limits": {"children": 3, "global": 8, "spawn_rate": 5, "approval_timeout_s": 120, "loop_guard": 12},
                "terminal": {
                    "font_family": "\"SF Mono\", Menlo, monospace",
                    "font_size": 13, "line_height": 1.2, "scrollback": 10000,
                    "theme": "system", "screen_reader": false
                }
            }))
        );
        assert_eq!(s.validate(), Ok(()));
        assert_eq!(s.limits(), Limits::default());
        // Before init the free functions give the defaults too.
        assert_eq!(approval_timeout(), Duration::from_secs(120));
        assert!(!spawn_enabled());
    }

    #[test]
    fn ranges_are_enforced() {
        let s = Settings::default();
        let refused = |patch: Value| s.patched(patch).err().unwrap_or_default();
        assert_eq!(
            refused(json!({"limits": {"children": 6}})),
            "Anak hidup per pane harus 1–5 (diberi 6)."
        );
        assert_eq!(
            refused(json!({"limits": {"approval_timeout_s": 29}})),
            "Batas waktu kartu persetujuan (detik) harus 30–300 (diberi 29)."
        );
        assert!(refused(json!({"limits": {"loop_guard": 1}})).starts_with("Pengaman putaran"));
        assert!(refused(json!({"limits": {"global": 13}})).contains("1–12"));
        assert!(refused(json!({"limits": {"spawn_rate": 0}})).contains("1–10"));
        assert!(refused(json!({"terminal": {"font_size": 25}})).starts_with("Ukuran font"));
        assert!(refused(json!({"terminal": {"line_height": 0.5}})).starts_with("Tinggi baris"));
        assert!(refused(json!({"terminal": {"scrollback": 100_001}})).starts_with("Scrollback"));
        assert!(refused(json!({"terminal": {"font_family": "  "}})).starts_with("Font"));
        assert!(refused(json!({"terminal": {"theme": "sepia"}})).starts_with("Nilai"));
        assert!(refused(json!({"limits": {"children": -1}})).starts_with("Nilai"));
        assert!(refused(json!({"limits": {"children": "3"}})).starts_with("Nilai"));
        assert_eq!(
            refused(json!({"terminal": {"fontSize": 14}})),
            "Pengaturan tidak dikenal: terminal.fontSize."
        );
        assert!(refused(json!({"limits": 3})).contains("limits"));
        assert!(refused(json!([1])).contains("objek"));

        let edges = ok(s.patched(json!({
            "spawn_enabled": true,
            "limits": {"children": 5, "global": 1, "spawn_rate": 10, "approval_timeout_s": 300, "loop_guard": 2},
            "terminal": {"font_size": 24, "line_height": 2.0, "scrollback": 0, "theme": "light"}
        })));
        assert!(edges.spawn_enabled);
        assert_eq!(edges.limits.loop_guard, 2);
        assert_eq!(edges.terminal.theme, Theme::Light);
        assert_eq!(
            edges.terminal.font_family,
            Terminal::default().font_family,
            "untouched keys stay"
        );
    }

    #[test]
    fn missing_broken_and_partial_files() {
        let tmp = Tmp::new("load");
        let path = tmp.0.join(FILE);
        assert_eq!(load(&path), (Settings::default(), None), "no file yet");

        ok(fs::create_dir_all(&tmp.0));
        ok(fs::write(&path, "{\"spawn_enabled\": tru"));
        let (s, warn) = load(&path);
        assert_eq!(s, Settings::default());
        assert!(warn.is_some_and(|w| w.contains("rusak")));

        ok(fs::write(&path, r#"{"limits": {"children": 99}}"#));
        let (s, warn) = load(&path);
        assert_eq!(s, Settings::default());
        assert!(warn.is_some_and(|w| w.contains("Anak hidup per pane")));

        // An older file without newer keys keeps its values and gets defaults for the rest.
        ok(fs::write(
            &path,
            r#"{"spawn_enabled": true, "limits": {"loop_guard": 20}}"#,
        ));
        let (s, warn) = load(&path);
        assert_eq!(warn, None);
        assert!(s.spawn_enabled);
        assert_eq!(s.limits.loop_guard, 20);
        assert_eq!(s.limits.children, 3);
        assert_eq!(s.terminal, Terminal::default());

        // The broken file is not rewritten by opening; the first change replaces it.
        ok(fs::write(&path, "garbage"));
        let (store, warn) = Store::open(Some(&tmp.0));
        assert!(warn.is_some());
        assert_eq!(ok(fs::read_to_string(&path)), "garbage");
        ok(store.set(json!({"peer_default": true})));
        assert!(load(&path).0.peer_default);
    }

    #[test]
    fn writes_are_atomic_and_refusals_change_nothing() {
        let tmp = Tmp::new("save");
        let (store, _) = Store::open(Some(&tmp.0));
        let path = tmp.0.join(FILE);
        let s = ok(store.set(json!({"terminal": {"font_size": 15}})));
        assert_eq!(load(&path), (s.clone(), None));
        assert!(!tmp.0.join("settings.json.tmp").exists());

        // Refused: neither memory nor file change.
        assert!(store.set(json!({"terminal": {"font_size": 9}})).is_err());
        assert_eq!(store.get(), s);
        assert_eq!(load(&path).0, s);

        // The temporary file cannot be written: the old file is intact and the change is not taken.
        ok(fs::create_dir_all(tmp.0.join("settings.json.tmp")));
        let err = store.set(json!({"terminal": {"font_size": 16}}));
        assert!(err.is_err_and(|e| e.contains("tidak bisa disimpan")));
        assert_eq!(store.get(), s);
        assert_eq!(load(&path).0, s);
    }

    #[test]
    fn memory_only_without_data_dir() {
        let (store, warn) = Store::open(None);
        assert_eq!(warn, None);
        assert!(ok(store.set(json!({"spawn_enabled": true}))).spawn_enabled);
        assert!(store.get().spawn_enabled);
    }

    #[test]
    fn limits_take_effect_in_policy_and_registry() {
        let (store, _) = Store::open(None);
        let caller = Pane {
            id: "p1".into(),
            workspace: "w".into(),
            kind: PaneKind::Agent,
            parent: None,
            exited: false,
        };
        let child = Pane {
            id: "p2".into(),
            parent: Some("p1".into()),
            ..caller.clone()
        };
        let check = |s: &Settings| {
            check_spawn(
                &caller,
                std::slice::from_ref(&child),
                &s.limits(),
                &mut SpawnRate::default(),
                Instant::now(),
                s.spawn_enabled,
            )
        };
        assert_eq!(check(&store.get()), Err(PolicyError::SpawnOff));
        let s = ok(store.set(json!({"spawn_enabled": true})));
        assert_eq!(check(&s), Ok(()));
        let s = ok(store.set(json!({"limits": {"children": 1}})));
        assert_eq!(check(&s), Err(PolicyError::CapChildren));

        let mut reg = Registry::default();
        let s = ok(store.set(json!({"peer_default": true, "limits": {"loop_guard": 30}})));
        apply(&s, &mut reg);
        assert_eq!(reg.limits.get(Limit::LoopGuard), 30);
        let ws = reg.create_workspace();
        assert!(reg.peer(ws), "new workspaces take peer_default");
        apply(&ok(store.set(json!({"peer_default": false}))), &mut reg);
        assert!(reg.peer(ws), "an existing workspace keeps its switch");
        let ws2 = reg.create_workspace();
        assert!(!reg.peer(ws2));
    }
}
