//! Pemindai OSC 7501 (Program Status Protocol rev 0.4) dan pemetaannya ke status pane (ARCHITECTURE §6).
//!
//! Logika murni: thread baca PTY memanggil [`Scanner::feed`] dengan potongan byte apa adanya, menulis
//! [`QUERY_REPLY`] ke PTY untuk setiap [`Event::Query`], dan meneruskan [`Event::Report`] ke [`Tracker`].
//! Aliran byte ke xterm.js tidak diubah. Batas dan aturan mengikuti spesifikasi rev 0.4 (D-016).

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

/// Jawaban query `OSC 7501;?`. Tanpa jawaban ini Claude Code tidak pernah melapor (D-016).
pub const QUERY_REPLY: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

const PREFIX: &[u8] = b"7501;";
/// Seluruh sekuens, termasuk `ESC ]` dan terminator.
const MAX_SEQUENCE: usize = 4096;
/// Badan terpanjang yang mungkin sah (`ESC ]` + badan + `BEL`).
const MAX_BODY: usize = MAX_SEQUENCE - 3;
const MAX_KEY: usize = 16;
const MAX_APP: usize = 32;
const MAX_ID: usize = 128;
const MAX_ID_SEGMENT: usize = 32;
const MAX_ID_DEPTH: usize = 8;
const MAX_MSG_ENCODED: usize = 2732;
const MAX_MSG: usize = 2048;
const MAX_TITLE_ENCODED: usize = 256;
const MAX_TITLE: usize = 192;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;

/// Base64 standar dengan padding opsional (spesifikasi).
const BASE64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Permission,
    Question,
    Auth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub state: State,
    pub kind: Option<Kind>,
    /// Teks dari agen, sudah didekode dan bebas karakter kontrol. Hanya untuk ditampilkan.
    pub msg: Option<String>,
    pub app: Option<String>,
    pub id: Option<String>,
    /// 0–100; `None` berarti tak tentu.
    pub progress: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Program bertanya dukungan; pemanggil wajib menulis [`QUERY_REPLY`] ke PTY.
    Query,
    Report(Report),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ground,
    Esc,
    Osc,
    OscEsc,
}

/// Pemindai streaming. Memori dibatasi [`MAX_BODY`] byte; OSC lain dan sekuens kepanjangan dilewati tanpa disimpan.
#[derive(Debug)]
pub struct Scanner {
    mode: Mode,
    body: Vec<u8>,
    body_len: usize,
    skip: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            mode: Mode::Ground,
            body: Vec::new(),
            body_len: 0,
            skip: false,
        }
    }
}

impl Scanner {
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        for &b in chunk {
            match self.mode {
                Mode::Ground => {
                    if b == ESC {
                        self.mode = Mode::Esc;
                    }
                }
                Mode::Esc => self.after_esc(b),
                Mode::Osc => match b {
                    BEL => self.finish(1, &mut events),
                    ESC => self.mode = Mode::OscEsc,
                    CAN | SUB => self.mode = Mode::Ground,
                    _ => self.push(b),
                },
                Mode::OscEsc => {
                    if b == b'\\' {
                        self.finish(2, &mut events);
                    } else {
                        // ESC tanpa `\` membatalkan OSC dan memulai escape baru.
                        self.after_esc(b);
                    }
                }
            }
        }
        events
    }

    fn after_esc(&mut self, b: u8) {
        self.mode = match b {
            b']' => {
                self.body.clear();
                self.body_len = 0;
                self.skip = false;
                Mode::Osc
            }
            ESC => Mode::Esc,
            _ => Mode::Ground,
        };
    }

    fn push(&mut self, b: u8) {
        self.body_len += 1;
        if self.skip {
            return;
        }
        let i = self.body.len();
        if self.body_len > MAX_BODY || (i < PREFIX.len() && PREFIX[i] != b) {
            self.skip = true;
            self.body = Vec::new();
            return;
        }
        self.body.push(b);
    }

    fn finish(&mut self, terminator_len: usize, events: &mut Vec<Event>) {
        self.mode = Mode::Ground;
        if self.skip || self.body.len() < PREFIX.len() {
            return;
        }
        if 2 + self.body_len + terminator_len > MAX_SEQUENCE {
            return;
        }
        if let Some(event) = parse(&self.body[PREFIX.len()..]) {
            events.push(event);
        }
    }
}

fn is_value_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b)
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"_.+-".contains(&b)
}

/// Mengurai isi setelah `7501;`. `None` berarti laporan diabaikan utuh.
fn parse(payload: &[u8]) -> Option<Event> {
    if payload == b"?" {
        return Some(Event::Query);
    }
    let (mut state, mut kind, mut msg, mut title, mut app, mut id, mut progress) =
        (None, None, None, None, None, None, None);
    for pair in payload.split(|&b| b == b':') {
        let Some(eq) = pair.iter().position(|&b| b == b'=') else {
            continue;
        };
        let key = pair[..eq].trim_ascii();
        let value = pair[eq + 1..].trim_ascii();
        if key.len() > MAX_KEY {
            return None;
        }
        if key.is_empty()
            || !key.iter().all(u8::is_ascii_lowercase)
            || !value.iter().copied().all(is_value_byte)
        {
            continue;
        }
        let slot = match key {
            b"state" => &mut state,
            b"kind" => &mut kind,
            b"msg" => &mut msg,
            b"title" => &mut title,
            b"app" => &mut app,
            b"id" => &mut id,
            b"progress" => &mut progress,
            _ => continue,
        };
        *slot = Some(value);
    }

    let state = match state? {
        b"idle" => State::Idle,
        b"working" => State::Working,
        b"done" => State::Done,
        b"blocked" => State::Blocked,
        b"error" => State::Error,
        b"clear" => State::Clear,
        _ => return None,
    };
    let kind = match kind {
        Some(b"permission") => Some(Kind::Permission),
        Some(b"question") => Some(Kind::Question),
        Some(b"auth") => Some(Kind::Auth),
        _ => None,
    };
    let progress = progress
        .filter(|v| !v.is_empty() && v.iter().all(u8::is_ascii_digit))
        .and_then(|v| std::str::from_utf8(v).ok()?.parse::<u8>().ok())
        .filter(|p| *p <= 100);
    let app = match app {
        Some(v) if v.len() > MAX_APP => return None,
        Some(v) if !v.is_empty() && v.iter().copied().all(is_name_byte) => Some(ascii(v)),
        _ => None,
    };
    let id = match id {
        Some(v) if !valid_id(v) => return None,
        v => v.map(ascii),
    };
    let msg = match msg {
        Some(v) => Some(decode_text(v, MAX_MSG_ENCODED, MAX_MSG)?),
        None => None,
    };
    if let Some(v) = title {
        decode_text(v, MAX_TITLE_ENCODED, MAX_TITLE)?;
    }
    Some(Event::Report(Report {
        state,
        kind,
        msg,
        app,
        id,
        progress,
    }))
}

fn valid_id(v: &[u8]) -> bool {
    v.len() <= MAX_ID
        && v.split(|&b| b == b'/').count() <= MAX_ID_DEPTH
        && v.split(|&b| b == b'/')
            .all(|s| (1..=MAX_ID_SEGMENT).contains(&s.len()) && s.iter().copied().all(is_name_byte))
}

/// Byte sudah divalidasi ASCII oleh `is_value_byte`.
fn ascii(v: &[u8]) -> String {
    String::from_utf8_lossy(v).into_owned()
}

/// Base64 → UTF-8 tanpa karakter kontrol (C0, DEL, C1). Ukuran terkode diperiksa sebelum dekode.
fn decode_text(v: &[u8], max_encoded: usize, max_decoded: usize) -> Option<String> {
    if v.len() > max_encoded {
        return None;
    }
    let bytes = BASE64.decode(v).ok()?;
    if bytes.len() > max_decoded {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text)
}

/// Status pane dari sinyal resmi agen (ARCHITECTURE §6). `starting`, `exited`, `unknown` ditentukan pemanggil.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    NeedsYou,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mapped {
    pub status: AgentStatus,
    /// Label header untuk `needs_you`.
    pub label: Option<&'static str>,
    /// `error` dipetakan ke `idle` dengan tanda galat di header.
    pub error: bool,
}

impl Report {
    /// `None` untuk `clear`.
    pub fn mapped(&self) -> Option<Mapped> {
        let (status, error) = match self.state {
            State::Idle | State::Done => (AgentStatus::Idle, false),
            State::Working => (AgentStatus::Working, false),
            State::Blocked => (AgentStatus::NeedsYou, false),
            State::Error => (AgentStatus::Idle, true),
            State::Clear => return None,
        };
        let label = (status == AgentStatus::NeedsYou).then_some(match self.kind {
            Some(Kind::Permission) => "butuh izin",
            Some(Kind::Question) => "butuh jawaban",
            Some(Kind::Auth) => "perlu login",
            None => "butuh kamu",
        });
        Some(Mapped {
            status,
            label,
            error,
        })
    }
}

/// Rekaman status per pane.
// ponytail: hanya rekaman akar (tanpa `id`) yang dilacak; Claude dan Codex tidak memakai `id`.
// Tambah peta rekaman per `id` (maks. 256) bila ada agen yang melapor sub-tugas.
#[derive(Debug, Default)]
pub struct Tracker {
    root: Option<Report>,
}

impl Tracker {
    pub fn apply(&mut self, report: Report) {
        match (report.state, &report.id) {
            (State::Clear, None) => self.root = None,
            (_, Some(_)) => {}
            (_, None) => self.root = Some(report),
        }
    }

    /// Proses agen keluar: `working`/`blocked` dibuang, `idle`/`done`/`error` bertahan (rev 0.4).
    pub fn process_exited(&mut self) {
        if matches!(
            self.root.as_ref().map(|r| r.state),
            Some(State::Working | State::Blocked)
        ) {
            self.root = None;
        }
    }

    pub fn current(&self) -> Option<&Report> {
        self.root.as_ref()
    }

    /// `None` bila belum ada sinyal resmi; pemanggil memakai heuristik.
    pub fn mapped(&self) -> Option<Mapped> {
        self.root.as_ref().and_then(Report::mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: &[u8] = b"\x1b]7501;state=idle:app=claude-code\x1b\\";
    const PERM: &[u8] = b"\x1b]7501;state=blocked:app=claude-code:kind=permission:msg=YXBwcm92ZSBXcml0ZTogL1VzZXJzL2Fkcmlhbi9kZXYvYWdlbnRkZWNrL3NwaWtlcy9vdXQvczQudHh0\x1b\\";
    const WORKING: &[u8] =
        b"\x1b]7501;state=working:app=claude-code:msg=V3JpdGluZyBzcGlrZXMvb3V0L3M0LnR4dA==\x1b\\";
    const CODEX_BLOCKED: &[u8] = b"\x1b]7501;state=blocked:app=codex\x07";
    const QUERY: &[u8] = b"\x1b]7501;?\x1b\\";

    fn scan(bytes: &[u8]) -> Vec<Event> {
        Scanner::default().feed(bytes)
    }

    fn one(bytes: &[u8]) -> Option<Report> {
        match scan(bytes).as_slice() {
            [Event::Report(r)] => Some(r.clone()),
            [] => None,
            other => panic!("unexpected events {other:?}"),
        }
    }

    fn osc(body: &str) -> Vec<u8> {
        format!("\x1b]7501;{body}\x1b\\").into_bytes()
    }

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn query_reply_is_exact() {
        assert_eq!(
            QUERY_REPLY,
            b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\"
        );
        assert_eq!(scan(QUERY), vec![Event::Query]);
        assert_eq!(scan(b"\x1b]7501;?\x07"), vec![Event::Query]);
    }

    #[test]
    fn spike_reports() {
        let r = one(IDLE).unwrap();
        assert_eq!(r.state, State::Idle);
        assert_eq!(r.app.as_deref(), Some("claude-code"));
        assert_eq!(r.msg, None);

        let r = one(PERM).unwrap();
        assert_eq!(r.state, State::Blocked);
        assert_eq!(r.kind, Some(Kind::Permission));
        assert_eq!(
            r.msg.as_deref(),
            Some("approve Write: /Users/adrian/dev/agentdeck/spikes/out/s4.txt")
        );
        let m = r.mapped().unwrap();
        assert_eq!(m.status, AgentStatus::NeedsYou);
        assert_eq!(m.label, Some("butuh izin"));

        let r = one(WORKING).unwrap();
        assert_eq!(r.msg.as_deref(), Some("Writing spikes/out/s4.txt"));

        let r = one(CODEX_BLOCKED).unwrap();
        assert_eq!((r.state, r.kind, &r.msg), (State::Blocked, None, &None));
        assert_eq!(r.app.as_deref(), Some("codex"));
        assert_eq!(r.mapped().unwrap().label, Some("butuh kamu"));
    }

    #[test]
    fn split_at_every_byte() {
        let mut stream = b"hello \x1b]0;title\x07".to_vec();
        for part in [QUERY, IDLE, PERM, WORKING, CODEX_BLOCKED] {
            stream.extend_from_slice(part);
            stream.extend_from_slice(b"\x1b[31mtext\x1b]11;?\x1b\\");
        }
        let whole = scan(&stream);
        assert_eq!(whole.len(), 5);
        for i in 0..=stream.len() {
            for j in i..=stream.len() {
                let mut s = Scanner::default();
                let mut got = s.feed(&stream[..i]);
                got.extend(s.feed(&stream[i..j]));
                got.extend(s.feed(&stream[j..]));
                assert_eq!(got, whole, "split at {i}/{j}");
            }
        }
        // Satu byte per potongan.
        let mut s = Scanner::default();
        let got: Vec<Event> = stream.iter().flat_map(|b| s.feed(&[*b])).collect();
        assert_eq!(got, whole);
    }

    #[test]
    fn other_oscs_ignored() {
        assert!(scan(b"\x1b]0;7501;state=idle\x07").is_empty());
        assert!(scan(b"\x1b]11;?\x1b\\\x1b]10;?\x07").is_empty());
        assert!(scan(b"\x1b]75011;state=idle\x07").is_empty());
        assert!(scan(b"\x1b]7501\x07").is_empty());
        assert!(scan(b"7501;state=idle\x07").is_empty());
    }

    #[test]
    fn unknown_and_repeated_keys() {
        let r = one(&osc("state=working:foo=bar:state=idle:zz=1")).unwrap();
        assert_eq!(r.state, State::Idle);
        let r = one(&osc("state=blocked:kind=permission:kind=question")).unwrap();
        assert_eq!(r.kind, Some(Kind::Question));
        let r = one(&osc(" state = done : app = codex ")).unwrap();
        assert_eq!((r.state, r.app.as_deref()), (State::Done, Some("codex")));
    }

    #[test]
    fn malformed_pairs_skipped() {
        // Tanpa `=`, kunci kosong, kunci huruf besar, byte di luar himpunan nilai.
        let r = one(&osc("state=idle:junk:=x:App=x:app=co dex")).unwrap();
        assert_eq!((r.state, r.app), (State::Idle, None));
        // Pasangan state rusak dilewati, yang sah sebelumnya tetap.
        assert_eq!(
            one(&osc("state=working:state=id!e")).unwrap().state,
            State::Working
        );
    }

    #[test]
    fn missing_or_unknown_state_ignored() {
        assert_eq!(one(&osc("app=codex")), None);
        assert_eq!(one(&osc("state=sleeping")), None);
        assert_eq!(one(&osc("")), None);
    }

    #[test]
    fn values_out_of_set_become_absent() {
        let r = one(&osc("state=working:kind=other:progress=101")).unwrap();
        assert_eq!((r.kind, r.progress), (None, None));
        assert_eq!(
            one(&osc("state=working:progress=+5")).unwrap().progress,
            None
        );
        assert_eq!(
            one(&osc("state=working:progress=42")).unwrap().progress,
            Some(42)
        );
        assert_eq!(one(&osc("state=working:app=a,b")).unwrap().app, None);
    }

    #[test]
    fn bad_base64_discards_report() {
        assert_eq!(
            one(&osc("state=blocked:msg=!!!")).map(|r| r.msg),
            Some(None)
        ); // pasangan rusak dilewati
        assert_eq!(one(&osc("state=blocked:msg=YWJj=")), None);
        assert_eq!(one(&osc("state=blocked:msg=a")), None);
        assert_eq!(one(&osc("state=blocked:title=a")), None);
        // Padding opsional.
        assert_eq!(
            one(&osc("state=working:msg=V3JpdGluZyBzcGlrZXMvb3V0L3M0LnR4dA"))
                .unwrap()
                .msg
                .as_deref(),
            Some("Writing spikes/out/s4.txt")
        );
        // Bukan UTF-8.
        assert_eq!(one(&osc("state=blocked:msg=/w==")), None);
    }

    #[test]
    fn control_chars_discard_report() {
        for text in ["a\nb", "tab\there", "esc\x1b[2J", "del\x7f", "c1\u{9b}"] {
            assert_eq!(
                one(&osc(&format!("state=blocked:msg={}", b64(text)))),
                None,
                "{text:?}"
            );
            assert_eq!(one(&osc(&format!("state=idle:title={}", b64(text)))), None);
        }
        let r = one(&osc(&format!(
            "state=blocked:kind=auth:msg={}",
            b64("Login expired · Please run /login")
        )))
        .unwrap();
        assert_eq!(r.msg.as_deref(), Some("Login expired · Please run /login"));
        assert_eq!(r.mapped().unwrap().label, Some("perlu login"));
    }

    #[test]
    fn limits() {
        let ok = b64(&"x".repeat(MAX_MSG));
        assert!(one(&osc(&format!("state=blocked:msg={ok}"))).is_some());
        assert_eq!(
            one(&osc(&format!(
                "state=blocked:msg={}",
                b64(&"x".repeat(MAX_MSG + 1))
            ))),
            None
        );
        assert!(
            one(&osc(&format!(
                "state=idle:title={}",
                b64(&"t".repeat(MAX_TITLE))
            )))
            .is_some()
        );
        assert_eq!(
            one(&osc(&format!(
                "state=idle:title={}",
                b64(&"t".repeat(MAX_TITLE + 1))
            ))),
            None
        );
        assert!(
            one(&osc(&format!("state=idle:app={}", "a".repeat(32))))
                .unwrap()
                .app
                .is_some()
        );
        assert_eq!(
            one(&osc(&format!("state=idle:app={}", "a".repeat(33)))),
            None
        );
        assert_eq!(one(&osc(&format!("state=idle:{}=1", "k".repeat(17)))), None);
        assert!(one(&osc(&format!("state=idle:{}=1", "k".repeat(16)))).is_some());

        // Seluruh sekuens: tepat 4096 lolos, 4097 dibuang.
        let fill = |n: usize| format!("state=idle:pad={}", "p".repeat(n));
        let base = osc(&fill(0)).len();
        assert!(one(&osc(&fill(MAX_SEQUENCE - base))).is_some());
        assert_eq!(one(&osc(&fill(MAX_SEQUENCE - base + 1))), None);
        let bel = format!("\x1b]7501;{}\x07", fill(MAX_SEQUENCE - base + 1));
        assert_eq!(bel.len(), MAX_SEQUENCE);
        assert!(one(bel.as_bytes()).is_some());
    }

    #[test]
    fn huge_sequence_stays_bounded_and_scanner_recovers() {
        let mut s = Scanner::default();
        s.feed(b"\x1b]7501;state=idle:pad=");
        for _ in 0..1000 {
            assert!(s.feed(&[b'p'; 1024]).is_empty());
        }
        assert!(s.body.capacity() <= MAX_BODY);
        assert!(s.feed(b"\x07").is_empty());
        assert_eq!(s.feed(IDLE).len(), 1);
        // OSC asing panjang tidak disimpan sama sekali.
        s.feed(b"\x1b]52;c;");
        s.feed(&[b'A'; 100_000]);
        assert_eq!(s.body.capacity(), 0);
        assert_eq!(s.feed(b"\x07").len(), 0);
    }

    #[test]
    fn aborted_sequences() {
        // ESC lain di tengah OSC membatalkannya; `ESC ]` langsung memulai OSC baru.
        assert_eq!(scan(b"\x1b]7501;state=working\x1b[0m").len(), 0);
        let mut v = b"\x1b]7501;state=working\x1b".to_vec();
        v.extend_from_slice(&IDLE[1..]);
        assert_eq!(one(&v).unwrap().state, State::Idle);
        assert!(scan(b"\x1b]7501;state=idle\x18\x07").is_empty());
    }

    #[test]
    fn id_rules() {
        let r = one(&osc("state=working:id=build/step-1")).unwrap();
        assert_eq!(r.id.as_deref(), Some("build/step-1"));
        assert_eq!(one(&osc("state=working:id=a//b")), None);
        assert_eq!(
            one(&osc(&format!("state=working:id={}", "a".repeat(33)))),
            None
        );
        assert_eq!(
            one(&osc(&format!("state=working:id={}", ["a"; 9].join("/")))),
            None
        );
        assert_eq!(one(&osc("state=working:id=a,b")), None);
    }

    #[test]
    fn mapping() {
        let m = |s: &str| one(&osc(s)).unwrap().mapped();
        assert_eq!(m("state=idle").unwrap().status, AgentStatus::Idle);
        assert_eq!(m("state=done").unwrap().status, AgentStatus::Idle);
        assert_eq!(m("state=working").unwrap().status, AgentStatus::Working);
        assert_eq!(m("state=working").unwrap().label, None);
        assert_eq!(
            m("state=blocked:kind=question").unwrap().label,
            Some("butuh jawaban")
        );
        let e = m("state=error").unwrap();
        assert_eq!((e.status, e.error), (AgentStatus::Idle, true));
        assert_eq!(m("state=clear"), None);
    }

    #[test]
    fn tracker_drops_working_and_blocked_on_exit() {
        let report = |s: &str| one(&osc(s)).unwrap();
        let mut t = Tracker::default();
        assert_eq!(t.mapped(), None);
        for (state, survives) in [
            ("idle", true),
            ("done", true),
            ("error", true),
            ("working", false),
            ("blocked", false),
        ] {
            t.apply(report(&format!("state={state}")));
            t.process_exited();
            assert_eq!(t.current().is_some(), survives, "{state}");
        }

        t.apply(report("state=working"));
        t.apply(report("state=idle:id=sub"));
        assert_eq!(t.current().unwrap().state, State::Working);
        t.apply(report("state=clear:id=sub"));
        assert!(t.current().is_some());
        t.apply(report("state=clear"));
        assert_eq!(t.current(), None);
    }
}
