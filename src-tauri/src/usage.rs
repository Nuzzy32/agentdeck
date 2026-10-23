//! Kuota dan konteks Claude per pane dari statusline resmi Claude Code (D-029). Claude menjalankan perintah
//! `statusLine` yang disuntikkan per peluncuran (`--settings`, D-005) dan memberinya JSON lewat stdin; perintah itu
//! hanya menyalin JSON ke [`path`]. Tidak ada kredensial yang dibaca (D-006).
//!
//! Isi berkas ditulis proses agen, jadi tidak dipercaya: dibaca maksimal [`MAX_BYTES`], hanya kolom yang dipakai,
//! persentase dijepit 0–100, `session_id` hanya bila bentuknya UUID.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// Env yang dibaca perintah statusline: path berkas pane ini.
pub const ENV: &str = "AGENTDECK_STATUSLINE";
/// JSON statusline 2.1.309 ±2 KB; lebih dari ini bukan statusline.
const MAX_BYTES: u64 = 64 * 1024;

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// `<data>/statusline`, dikosongkan saat start: id pane dipakai ulang antar-run, angka lama tidak boleh ikut.
pub fn init(data_dir: &Path) {
    let dir = data_dir.join("statusline");
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = DIR.set(dir);
    }
}

/// Berkas statusline pane `pane`; `None` tanpa direktori data (agen jalan tanpa statusline).
pub fn path(pane: &str) -> Option<PathBuf> {
    DIR.get().map(|d| d.join(format!("{pane}.json")))
}

/// Pane dijalankan (ulang): angka run sebelumnya dibuang.
pub fn clear(pane: &str) {
    if let Some(p) = path(pane) {
        let _ = std::fs::remove_file(p);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Window {
    /// Persen terpakai, 0–100.
    pub used: u8,
    /// Detik sejak epoch.
    pub resets_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// Persen jendela konteks terpakai; `None` sebelum giliran pertama.
    pub context: Option<u8>,
    /// Untuk `--resume` saat tata letak dipulihkan; tidak dikirim ke UI.
    #[serde(skip)]
    pub session: Option<String>,
}

#[derive(Deserialize)]
struct RawWindow {
    used_percentage: Option<f64>,
    resets_at: Option<f64>,
}

#[derive(Deserialize)]
struct RawLimits {
    five_hour: Option<RawWindow>,
    seven_day: Option<RawWindow>,
}

#[derive(Deserialize)]
struct RawContext {
    used_percentage: Option<f64>,
}

#[derive(Deserialize)]
struct Raw {
    session_id: Option<String>,
    context_window: Option<RawContext>,
    rate_limits: Option<RawLimits>,
}

fn percent(v: Option<f64>) -> Option<u8> {
    // `as` saturates and maps NaN to 0; the clamp keeps it in 0..=100.
    v.filter(|v| v.is_finite())
        .map(|v| v.round().clamp(0.0, 100.0) as u8)
}

fn window(w: Option<RawWindow>) -> Option<Window> {
    let w = w?;
    Some(Window {
        used: percent(w.used_percentage)?,
        resets_at: w
            .resets_at
            .filter(|t| t.is_finite() && *t >= 0.0)
            .map_or(0, |t| t as u64),
    })
}

/// `8-4-4-4-12` hex, the only shape passed on to `--resume`.
fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(p, n)| p.len() == n && p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `None` bila bukan JSON statusline (berkas sedang ditulis, rusak, atau kosong).
pub fn parse(bytes: &[u8]) -> Option<Usage> {
    let raw: Raw = serde_json::from_slice(bytes).ok()?;
    let limits = raw.rate_limits;
    let (five, seven) = limits.map_or((None, None), |l| (l.five_hour, l.seven_day));
    Some(Usage {
        five_hour: window(five),
        seven_day: window(seven),
        context: percent(raw.context_window.and_then(|c| c.used_percentage)),
        session: raw.session_id.filter(|s| is_uuid(s)),
    })
}

/// Pembacaan terakhir pane `pane`, `None` bila belum ada atau tidak terbaca.
pub fn read(pane: &str) -> Option<Usage> {
    let file = std::fs::File::open(path(pane)?).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    parse(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from a real Claude Code 2.1.309 statusline after one turn.
    const REAL: &str = r#"{"session_id":"3fe10b28-c0b7-47aa-8cb4-dfe4947debd1","version":"2.1.309",
        "context_window":{"total_input_tokens":52561,"context_window_size":1000000,"used_percentage":5,
        "remaining_percentage":95},"rate_limits":{"five_hour":{"used_percentage":31,"resets_at":1792766400},
        "seven_day":{"used_percentage":15,"resets_at":1793210400}},"cost":{"total_cost_usd":0.0037}}"#;

    #[test]
    fn parses_real_statusline() {
        let u = parse(REAL.as_bytes()).unwrap();
        assert_eq!(
            u.five_hour,
            Some(Window {
                used: 31,
                resets_at: 1792766400
            })
        );
        assert_eq!(u.seven_day.map(|w| w.used), Some(15));
        assert_eq!(u.context, Some(5));
        assert_eq!(
            u.session.as_deref(),
            Some("3fe10b28-c0b7-47aa-8cb4-dfe4947debd1")
        );
        // The session id never reaches the UI.
        assert!(!serde_json::to_string(&u).unwrap().contains("3fe10b28"));
    }

    #[test]
    fn before_first_turn_has_no_numbers() {
        let u = parse(br#"{"session_id":"x","context_window":{"used_percentage":null}}"#).unwrap();
        assert_eq!((u.five_hour, u.seven_day, u.context), (None, None, None));
        assert_eq!(u.session, None, "not a UUID");
    }

    #[test]
    fn untrusted_values_are_clamped_or_dropped() {
        let u = parse(
            br#"{"session_id":"3fe10b28-c0b7-47aa-8cb4-dfe4947debd1; rm -rf ~",
                "context_window":{"used_percentage":250.7},
                "rate_limits":{"five_hour":{"used_percentage":-4,"resets_at":-1},"seven_day":{"resets_at":5}}}"#,
        )
        .unwrap();
        assert_eq!(u.context, Some(100));
        assert_eq!(
            u.five_hour,
            Some(Window {
                used: 0,
                resets_at: 0
            })
        );
        assert_eq!(u.seven_day, None, "no percentage, no window");
        assert_eq!(u.session, None);
        assert_eq!(parse(b"{\"session_id\":"), None, "half-written file");
        assert_eq!(parse(b""), None);
    }
}
