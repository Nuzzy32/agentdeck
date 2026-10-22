//! Message and audit logs (ARCHITECTURE §8, D-010): `logs/messages-YYYY-MM-DD.jsonl` and `logs/audit-YYYY-MM-DD.jsonl`
//! under the app data directory, one JSON object per line, dated in UTC, kept 14 days. No token anywhere: the input
//! types carry none, and every line is scrubbed of 64-hex runs (the pane token shape) before it is written, so a token
//! an agent pastes into a message does not reach the disk either. Time is passed in, so tests use a fake clock.
//! Wiring to `pane_send` is T2.7; audit events are written from T3.x.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::delivery::State;

/// Days a log file is kept, today included.
pub const RETENTION_DAYS: i64 = 14;
const TOKEN_HEX: usize = 64;
const REDACTED: &str = "[redacted]";

/// One state change of a message, as given by the caller.
#[derive(Clone, Copy, Debug)]
pub struct Msg<'a> {
    pub workspace: &'a str,
    pub from: &'a str,
    pub to: &'a str,
    pub id: &'a str,
    pub state: State,
    pub text: &'a str,
}

/// A line of `messages-*.jsonl`; also one row of the Messages panel after `read_messages`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageLine {
    pub ts: String,
    pub workspace: String,
    pub from: String,
    pub to: String,
    pub message_id: String,
    pub state: String,
    pub reason: Option<String>,
    pub text: String,
}

/// Audit events (ARCHITECTURE §8). Pane ids, agent names, and folders only.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Audit<'a> {
    SpawnRequested {
        from: &'a str,
        agent: &'a str,
        cwd: &'a str,
    },
    SpawnApproved {
        from: &'a str,
        agent: &'a str,
        cwd: &'a str,
        pane: &'a str,
    },
    SpawnDenied {
        from: &'a str,
        agent: &'a str,
        cwd: &'a str,
        reason: &'a str,
    },
    LoopPaused {
        count: u32,
    },
    PaneClosed {
        pane: &'a str,
    },
}

#[derive(Serialize)]
struct AuditLine<'a> {
    ts: String,
    workspace: &'a str,
    #[serde(flatten)]
    event: &'a Audit<'a>,
}

/// Appends are serialised by one lock, so lines from different threads never interleave.
#[derive(Debug)]
pub struct Store {
    logs: PathBuf,
    lock: Mutex<()>,
}

impl Store {
    /// `data_dir` is the app data directory; logs go to `data_dir/logs`, created on first write.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            logs: data_dir.join("logs"),
            lock: Mutex::new(()),
        }
    }

    pub fn log_message(&self, now: SystemTime, m: &Msg) -> io::Result<()> {
        let reason = match m.state {
            State::Dropped(r) => Some(r.as_str().to_owned()),
            _ => None,
        };
        let line = MessageLine {
            ts: timestamp(now),
            workspace: m.workspace.to_owned(),
            from: m.from.to_owned(),
            to: m.to.to_owned(),
            message_id: m.id.to_owned(),
            state: m.state.as_str().to_owned(),
            reason,
            text: m.text.to_owned(),
        };
        self.append("messages", now, &line)
    }

    pub fn log_audit(&self, now: SystemTime, workspace: &str, event: &Audit) -> io::Result<()> {
        let line = AuditLine {
            ts: timestamp(now),
            workspace,
            event,
        };
        self.append("audit", now, &line)
    }

    fn append(&self, kind: &str, now: SystemTime, line: &impl Serialize) -> io::Result<()> {
        // Scrubbing the serialised line covers every field; JSON escaping never splits or creates a hex run.
        let mut json = redact(&serde_json::to_string(line)?);
        json.push('\n');
        let path = self.logs.join(format!("{kind}-{}.jsonl", date(now)));
        let _guard = self.lock.lock().unwrap_or_else(PoisonError::into_inner);
        fs::create_dir_all(&self.logs)?;
        let mut open = OpenOptions::new();
        open.create(true).append(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600); // message text is the person's work
        open.open(path)?.write_all(json.as_bytes())
    }

    /// Messages of `workspace`, oldest first, one per message: the first line's time and text, the last line's
    /// state and reason. Ids (`m<N>`) restart with each run, so an id seen again after it was delivered or dropped
    /// starts a new message. Unreadable lines (a write cut short by a crash) are skipped.
    pub fn read_messages(&self, workspace: &str) -> io::Result<Vec<MessageLine>> {
        let mut out: Vec<MessageLine> = Vec::new();
        let mut open: HashMap<String, usize> = HashMap::new();
        for (_, path) in self.files("messages")? {
            for line in fs::read_to_string(path)?.lines() {
                let Ok(l) = serde_json::from_str::<MessageLine>(line) else {
                    continue;
                };
                if l.workspace != workspace {
                    continue;
                }
                let id = l.message_id.clone();
                let done = matches!(l.state.as_str(), "delivered" | "dropped");
                match open.get(&id).and_then(|&i| out.get_mut(i)) {
                    Some(m) => {
                        m.state = l.state;
                        m.reason = l.reason;
                    }
                    None => {
                        open.insert(id.clone(), out.len());
                        out.push(l);
                    }
                }
                if done {
                    open.remove(&id);
                }
            }
        }
        Ok(out)
    }

    /// Removes message and audit files older than 14 days. Returns how many were removed.
    pub fn prune(&self, now: SystemTime) -> io::Result<usize> {
        let today = days(now);
        let mut removed = 0;
        for kind in ["messages", "audit"] {
            for (day, path) in self.files(kind)? {
                if today - day >= RETENTION_DAYS {
                    fs::remove_file(path)?;
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    /// `kind-YYYY-MM-DD.jsonl` files with their day number, oldest first. No directory yet = no files.
    fn files(&self, kind: &str) -> io::Result<Vec<(i64, PathBuf)>> {
        let entries = match fs::read_dir(&self.logs) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut files = Vec::new();
        for entry in entries {
            let path = entry?.path();
            let day = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| {
                    n.strip_prefix(kind)?
                        .strip_prefix('-')?
                        .strip_suffix(".jsonl")
                })
                .and_then(parse_date);
            if let Some(day) = day {
                files.push((day, path));
            }
        }
        files.sort();
        Ok(files)
    }
}

/// Replaces every run of 64 or more ASCII hex digits with `[redacted]`.
fn redact(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0; // byte length of the current hex run, which is ASCII
    for (i, c) in s.char_indices() {
        if c.is_ascii_hexdigit() {
            run += 1;
            continue;
        }
        flush(&mut out, &s[i - run..i]);
        run = 0;
        out.push(c);
    }
    flush(&mut out, &s[s.len() - run..]);
    out
}

fn flush(out: &mut String, run: &str) {
    out.push_str(if run.len() >= TOKEN_HEX {
        REDACTED
    } else {
        run
    });
}

fn secs(t: SystemTime) -> (i64, u32) {
    // A clock before 1970 is a broken clock; log it as the epoch rather than fail the write.
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    (
        i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        d.subsec_millis(),
    )
}

fn days(t: SystemTime) -> i64 {
    secs(t).0.div_euclid(86_400)
}

/// `YYYY-MM-DD` in UTC.
fn date(t: SystemTime) -> String {
    let (y, m, d) = civil(days(t));
    format!("{y:04}-{m:02}-{d:02}")
}

/// RFC 3339 in UTC with milliseconds, e.g. `2026-10-22T14:09:03.120Z`.
fn timestamp(t: SystemTime) -> String {
    let (s, ms) = secs(t);
    let sod = s.rem_euclid(86_400);
    format!(
        "{}T{:02}:{:02}:{:02}.{ms:03}Z",
        date(t),
        sod / 3600,
        sod / 60 % 60,
        sod % 60
    )
}

/// Day number since 1970-01-01 to (year, month, day); Howard Hinnant's `civil_from_days`.
fn civil(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Inverse of `civil`; `days_from_civil`.
fn days_from(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y = s[..4].parse().ok()?;
    let m = s[5..7].parse().ok()?;
    let d = s[8..].parse().ok()?;
    let day = days_from(y, m, d);
    (civil(day) == (y, m, d)).then_some(day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delivery::DropReason;
    use crate::tokens::Token;
    use std::sync::Arc;
    use std::time::Duration;

    /// 2026-10-22T00:00:00Z.
    const DAY0: u64 = 1_792_627_200;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    struct Tmp(PathBuf);
    impl Tmp {
        fn new(name: &str) -> Self {
            let p =
                std::env::temp_dir().join(format!("agentdeck-store-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&p);
            Self(p)
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn msg<'a>(workspace: &'a str, id: &'a str, state: State, text: &'a str) -> Msg<'a> {
        Msg {
            workspace,
            from: "p2",
            to: "p1",
            id,
            state,
            text,
        }
    }

    fn read(path: PathBuf) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(date(at(DAY0)), "2026-10-22");
        assert_eq!(date(at(0)), "1970-01-01");
        assert_eq!(
            timestamp(at(DAY0 + 14 * 3600 + 9 * 60 + 3) + Duration::from_millis(120)),
            "2026-10-22T14:09:03.120Z"
        );
        for day in [-1000, 0, 59, 60, 11_016, 20_748, 100_000] {
            let (y, m, d) = civil(day);
            assert_eq!(parse_date(&format!("{y:04}-{m:02}-{d:02}")), Some(day));
        }
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("2026-1-01x"), None);
    }

    #[test]
    fn message_line_format() {
        let tmp = Tmp::new("format");
        let s = Store::new(&tmp.0);
        let now = at(DAY0 + 50_000);
        s.log_message(now, &msg("w1", "m1", State::Queued, "review\n\"auth\""))
            .unwrap_or_else(|e| panic!("{e}"));
        s.log_message(
            now,
            &msg(
                "w1",
                "m1",
                State::Dropped(DropReason::TargetExited),
                "review\n\"auth\"",
            ),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let text = read(tmp.0.join("logs/messages-2026-10-22.jsonl"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0],
            r#"{"ts":"2026-10-22T13:53:20.000Z","workspace":"w1","from":"p2","to":"p1","message_id":"m1","state":"queued","reason":null,"text":"review\n\"auth\""}"#
        );
        assert!(lines[1].contains(r#""state":"dropped","reason":"target agent exited""#));
    }

    #[test]
    fn audit_line_format() {
        let tmp = Tmp::new("audit");
        let s = Store::new(&tmp.0);
        let now = at(DAY0);
        let events = [
            Audit::SpawnRequested {
                from: "p1",
                agent: "codex",
                cwd: "/w",
            },
            Audit::SpawnApproved {
                from: "p1",
                agent: "codex",
                cwd: "/w",
                pane: "p3",
            },
            Audit::SpawnDenied {
                from: "p1",
                agent: "codex",
                cwd: "/w",
                reason: "denied by the person",
            },
            Audit::LoopPaused { count: 12 },
            Audit::PaneClosed { pane: "p3" },
        ];
        for e in &events {
            s.log_audit(now, "w1", e).unwrap_or_else(|e| panic!("{e}"));
        }
        let text = read(tmp.0.join("logs/audit-2026-10-22.jsonl"));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[1],
            r#"{"ts":"2026-10-22T00:00:00.000Z","workspace":"w1","event":"spawn_approved","from":"p1","agent":"codex","cwd":"/w","pane":"p3"}"#
        );
        assert_eq!(
            lines[3],
            r#"{"ts":"2026-10-22T00:00:00.000Z","workspace":"w1","event":"loop_paused","count":12}"#
        );
        assert_eq!(lines.len(), 5);
    }

    #[test]
    fn rotates_at_utc_midnight() {
        let tmp = Tmp::new("rotate");
        let s = Store::new(&tmp.0);
        s.log_message(at(DAY0 - 1), &msg("w", "m1", State::Queued, "a"))
            .unwrap_or_else(|e| panic!("{e}"));
        s.log_message(at(DAY0), &msg("w", "m1", State::Delivered, "a"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            read(tmp.0.join("logs/messages-2026-10-21.jsonl"))
                .lines()
                .count(),
            1
        );
        assert_eq!(
            read(tmp.0.join("logs/messages-2026-10-22.jsonl"))
                .lines()
                .count(),
            1
        );
        // The reader joins the message across the two files.
        let got = s.read_messages("w").unwrap_or_default();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].state, "delivered");
        assert_eq!(got[0].ts, "2026-10-21T23:59:59.000Z");
    }

    #[test]
    fn keeps_fourteen_days() {
        let tmp = Tmp::new("prune");
        let s = Store::new(&tmp.0);
        assert_eq!(s.prune(at(DAY0)).ok(), Some(0), "no logs directory yet");
        for back in [0u64, 13, 14, 30] {
            let t = at(DAY0 - back * 86_400);
            s.log_message(t, &msg("w", "m1", State::Queued, "a"))
                .unwrap_or_else(|e| panic!("{e}"));
            s.log_audit(t, "w", &Audit::PaneClosed { pane: "p1" })
                .unwrap_or_else(|e| panic!("{e}"));
        }
        fs::write(tmp.0.join("logs/notes.txt"), "keep").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(s.prune(at(DAY0 + 3600)).ok(), Some(4));
        let mut left: Vec<String> = fs::read_dir(tmp.0.join("logs"))
            .unwrap_or_else(|e| panic!("{e}"))
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "audit-2026-10-09.jsonl",
                "audit-2026-10-22.jsonl",
                "messages-2026-10-09.jsonl",
                "messages-2026-10-22.jsonl",
                "notes.txt"
            ]
        );
    }

    #[test]
    fn reads_one_workspace_one_row_per_message() {
        let tmp = Tmp::new("read");
        let s = Store::new(&tmp.0);
        assert_eq!(
            s.read_messages("w1").ok(),
            Some(Vec::new()),
            "no logs directory yet"
        );
        let t = at(DAY0);
        let log = |m: Msg| s.log_message(t, &m).unwrap_or_else(|e| panic!("{e}"));
        log(msg("w1", "m1", State::Queued, "first"));
        log(msg("w2", "m2", State::Queued, "other workspace"));
        log(msg("w1", "m3", State::Paused, "second"));
        log(msg("w1", "m1", State::Delivered, "first"));
        log(msg(
            "w1",
            "m3",
            State::Dropped(DropReason::Stopped),
            "second",
        ));
        // Next run: ids start again.
        log(msg("w1", "m1", State::Queued, "new run"));
        fs::OpenOptions::new()
            .append(true)
            .open(tmp.0.join("logs/messages-2026-10-22.jsonl"))
            .and_then(|mut f| f.write_all(b"{\"ts\":\"cut short"))
            .unwrap_or_else(|e| panic!("{e}"));

        let got = s.read_messages("w1").unwrap_or_else(|e| panic!("{e}"));
        let rows: Vec<(&str, &str, Option<&str>, &str)> = got
            .iter()
            .map(|m| {
                (
                    m.message_id.as_str(),
                    m.state.as_str(),
                    m.reason.as_deref(),
                    m.text.as_str(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("m1", "delivered", None, "first"),
                (
                    "m3",
                    "dropped",
                    Some("stopped by the person at the loop guard"),
                    "second"
                ),
                ("m1", "queued", None, "new run"),
            ]
        );
    }

    fn has_hex_run(s: &str, n: usize) -> bool {
        s.as_bytes()
            .windows(n)
            .any(|w| w.iter().all(u8::is_ascii_hexdigit))
    }

    #[test]
    fn no_token_reaches_any_field() {
        let tmp = Tmp::new("token");
        let s = Store::new(&tmp.0);
        let token = Token::generate().unwrap_or_else(|e| panic!("{e}"));
        let tok = token.expose();
        let upper = tok.to_uppercase();
        let text = format!("export AGENTDECK_TOKEN={tok}\nand {upper}.");
        let t = at(DAY0);
        let m = Msg {
            workspace: tok,
            from: tok,
            to: tok,
            id: tok,
            state: State::Queued,
            text: &text,
        };
        s.log_message(t, &m).unwrap_or_else(|e| panic!("{e}"));
        let events = [
            Audit::SpawnRequested {
                from: tok,
                agent: tok,
                cwd: tok,
            },
            Audit::SpawnApproved {
                from: tok,
                agent: tok,
                cwd: tok,
                pane: tok,
            },
            Audit::SpawnDenied {
                from: tok,
                agent: tok,
                cwd: tok,
                reason: tok,
            },
            Audit::PaneClosed { pane: tok },
        ];
        for e in &events {
            s.log_audit(t, tok, e).unwrap_or_else(|e| panic!("{e}"));
        }
        for kind in ["messages", "audit"] {
            let body = read(tmp.0.join(format!("logs/{kind}-2026-10-22.jsonl")));
            assert!(!body.is_empty());
            assert!(!body.contains(tok) && !body.contains(&upper), "{kind}");
            assert!(!has_hex_run(&body, TOKEN_HEX), "{kind}");
        }
        let got = s.read_messages(REDACTED).unwrap_or_default();
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0].text,
            "export AGENTDECK_TOKEN=[redacted]\nand [redacted]."
        );
    }

    #[test]
    fn redact_keeps_shorter_hex() {
        let sha1 = "a".repeat(40);
        let h63 = "b".repeat(63);
        assert_eq!(redact(&format!("{sha1} {h63}")), format!("{sha1} {h63}"));
        assert_eq!(redact(&"c".repeat(64)), REDACTED);
        assert_eq!(redact(&format!("é{}é", "D".repeat(70))), "é[redacted]é");
        assert_eq!(redact(""), "");
    }

    #[test]
    fn parallel_appends_keep_lines_whole() {
        let tmp = Tmp::new("parallel");
        let s = Arc::new(Store::new(&tmp.0));
        let threads: Vec<_> = (0..8u8)
            .map(|n| {
                let s = Arc::clone(&s);
                std::thread::spawn(move || {
                    let text = char::from(b'g' + n).to_string().repeat(5000); // not hex, so not redacted
                    for i in 0..100 {
                        let id = format!("m{n}-{i}");
                        s.log_message(at(DAY0), &msg("w", &id, State::Queued, &text))
                            .unwrap_or_else(|e| panic!("{e}"));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap_or_else(|_| panic!("writer panicked"));
        }
        let body = read(tmp.0.join("logs/messages-2026-10-22.jsonl"));
        let mut count = 0;
        for line in body.lines() {
            let l: MessageLine =
                serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line:.80}"));
            assert_eq!(l.text.len(), 5000);
            count += 1;
        }
        assert_eq!(count, 800);
        assert_eq!(s.read_messages("w").map(|v| v.len()).ok(), Some(800));
    }

    #[test]
    fn io_errors_are_returned_not_panics() {
        let tmp = Tmp::new("ioerr");
        fs::create_dir_all(&tmp.0).unwrap_or_else(|e| panic!("{e}"));
        fs::write(tmp.0.join("logs"), "a file where the directory should be")
            .unwrap_or_else(|e| panic!("{e}"));
        let s = Store::new(&tmp.0);
        assert!(
            s.log_message(at(DAY0), &msg("w", "m1", State::Queued, "a"))
                .is_err()
        );
        assert!(
            s.log_audit(at(DAY0), "w", &Audit::LoopPaused { count: 1 })
                .is_err()
        );
        assert!(s.read_messages("w").is_err());
        assert!(s.prune(at(DAY0)).is_err());
    }
}
