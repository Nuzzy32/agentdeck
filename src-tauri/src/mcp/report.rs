//! `pane_report` (MCP-TOOLS, MCP-06): a child's structured result. Turned into a `RESULT status=… files=… tests=…`
//! line plus the summary, sent to the parent through `pane_send`'s path (queue, readiness, loop guard, log), kept as
//! the child's last report and announced to the UI (`agentdeck://pane-report`).

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::delivery::Engine;
use crate::panes;
use crate::policy::REPORTS_PER_MINUTE;

pub const SUMMARY_MAX_BYTES: usize = 2048;
/// Joined `files` cap, so summary + RESULT line always fit `pane_send`'s 8192 bytes.
pub const FILES_MAX_BYTES: usize = 4096;
const WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Done,
    Failed,
    Blocked,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Tests {
    Pass,
    Fail,
    None,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Args {
    status: Status,
    summary: String,
    #[serde(default)]
    files: Vec<String>,
    tests: Option<Tests>,
}

/// A child's last report, as the UI gets it (event payload and `pane_report_last`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub pane: String,
    pub status: &'static str,
    pub summary: String,
    pub files: Vec<String>,
    pub tests: &'static str,
    /// Unix epoch milliseconds.
    pub ts: u64,
}

pub type OnReport = Arc<dyn Fn(&Report) + Send + Sync>;

fn clean(s: &str, keep_newline: bool) -> String {
    s.chars()
        .filter(|&c| (keep_newline && c == '\n') || !c.is_control())
        .collect()
}

fn bad(why: &str) -> String {
    format!("BAD_ARGS: {why}. Fix the arguments.")
}

impl Args {
    /// Control characters dropped (newlines kept in the summary), limits checked. `pane` is the caller.
    pub fn report(self, pane: &str, ts: u64) -> Result<Report, String> {
        let summary = clean(&self.summary, true);
        if summary.trim().is_empty() {
            return Err(bad("summary is empty"));
        }
        if summary.len() > SUMMARY_MAX_BYTES {
            return Err(bad(&format!(
                "summary is over {SUMMARY_MAX_BYTES} bytes; point to files or commits instead"
            )));
        }
        let files: Vec<String> = self
            .files
            .iter()
            .map(|f| clean(f, false))
            .filter(|f| !f.trim().is_empty())
            .collect();
        if files.iter().map(|f| f.len() + 1).sum::<usize>() > FILES_MAX_BYTES {
            return Err(bad(&format!(
                "files are over {FILES_MAX_BYTES} bytes together; list the main ones or a folder"
            )));
        }
        Ok(Report {
            pane: pane.to_owned(),
            status: match self.status {
                Status::Done => "done",
                Status::Failed => "failed",
                Status::Blocked => "blocked",
            },
            summary,
            files,
            tests: match self.tests {
                Some(Tests::Pass) => "pass",
                Some(Tests::Fail) => "fail",
                Some(Tests::None) | None => "none",
            },
            ts,
        })
    }
}

impl Report {
    /// `RESULT status=done files=a.rs,b.rs tests=pass` (`files=-` when none), then the summary.
    pub fn message(&self) -> String {
        let files = if self.files.is_empty() {
            "-".to_owned()
        } else {
            self.files.join(",")
        };
        format!(
            "RESULT status={} files={files} tests={}\n{}",
            self.status, self.tests, self.summary
        )
    }
}

/// REPORT_RATE windows per caller and the UI callback. Cheap to clone; one per MCP server.
#[derive(Clone)]
pub struct Reports {
    times: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    on_report: OnReport,
}

impl Reports {
    pub fn new(on_report: OnReport) -> Self {
        Self {
            times: Arc::default(),
            on_report,
        }
    }

    /// Takes a slot in `caller`'s window, or `REPORT_RATE` with the seconds to wait.
    fn take(&self, caller: &str, now: Instant) -> Result<(), String> {
        let mut all = self.times.lock().unwrap_or_else(|e| e.into_inner());
        let times = all.entry(caller.to_owned()).or_default();
        while times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= WINDOW)
        {
            times.pop_front();
        }
        if times.len() >= REPORTS_PER_MINUTE as usize {
            let left = times.front().map_or(Duration::ZERO, |t| {
                (*t + WINDOW).saturating_duration_since(now)
            });
            let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
            return Err(format!(
                "REPORT_RATE: more than {REPORTS_PER_MINUTE} pane_report calls in a minute. Wait {secs} seconds; \
                 report once when the task is finished."
            ));
        }
        times.push_back(now);
        Ok(())
    }

    /// A refused send gives its slot back, so only reports that reached the parent's queue count.
    fn give_back(&self, caller: &str, at: Instant) {
        let mut all = self.times.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(times) = all.get_mut(caller)
            && let Some(i) = times.iter().rposition(|t| *t == at)
        {
            times.remove(i);
        }
    }

    /// `pane_report` for `caller`: send to its parent, keep as its last report, tell the UI.
    /// Blocks up to the Enter delay like `pane_send`.
    pub fn send(
        &self,
        engine: &Engine,
        caller: &str,
        args: Args,
    ) -> Result<(String, &'static str), String> {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        let report = args.report(caller, ts)?;
        let now = Instant::now();
        self.take(caller, now)?;
        let sent = engine.send(caller, "parent", &report.message());
        if sent.is_err() {
            self.give_back(caller, now);
        }
        let sent = sent?;
        panes::lock(&engine.panes).set_report(report.clone());
        (self.on_report)(&report);
        Ok(sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(v: serde_json::Value) -> Result<Args, String> {
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    fn report(v: serde_json::Value) -> Result<Report, String> {
        args(v)?.report("p2", 7)
    }

    #[test]
    fn result_line_formats() {
        let r = report(json!({"status": "done", "summary": "ok"})).unwrap();
        assert_eq!(r.message(), "RESULT status=done files=- tests=none\nok");
        let r = report(json!({"status": "failed", "summary": "a\nb", "files": ["src/a.rs", "b c.md"], "tests": "fail"}))
            .unwrap();
        assert_eq!(
            r.message(),
            "RESULT status=failed files=src/a.rs,b c.md tests=fail\na\nb"
        );
        assert_eq!((r.pane.as_str(), r.ts), ("p2", 7));
        let r = report(json!({"status": "blocked", "summary": "s", "files": [], "tests": "pass"}))
            .unwrap();
        assert_eq!(r.message(), "RESULT status=blocked files=- tests=pass\ns");
    }

    #[test]
    fn control_characters_are_dropped() {
        let r = report(json!({
            "status": "done",
            "summary": "x\x1b[201~\ry\n\tz\x07",
            "files": ["a\nRESULT status=done.rs", "\x1b", "b\x00.rs"]
        }))
        .unwrap();
        assert_eq!(r.summary, "x[201~y\nz");
        assert_eq!(r.files, ["aRESULT status=done.rs", "b.rs"]);
        assert_eq!(r.message().lines().count(), 3);
    }

    #[test]
    fn limits_and_bad_args() {
        let at = |n: usize| report(json!({"status": "done", "summary": "é".repeat(n / 2)}));
        assert!(at(SUMMARY_MAX_BYTES).is_ok());
        let ok =
            report(json!({"status": "done", "summary": "a".repeat(SUMMARY_MAX_BYTES)})).unwrap();
        assert_eq!(ok.summary.len(), 2048);
        // Measured after control characters are dropped.
        let padded = format!("{}\x07\x07", "a".repeat(SUMMARY_MAX_BYTES));
        assert!(report(json!({"status": "done", "summary": padded})).is_ok());
        for v in [
            json!({"status": "done", "summary": "a".repeat(SUMMARY_MAX_BYTES + 1)}),
            json!({"status": "done", "summary": "\x1b\x07 \n"}),
            json!({"status": "done", "summary": "s", "files": ["a".repeat(FILES_MAX_BYTES)]}),
        ] {
            assert!(report(v).unwrap_err().starts_with("BAD_ARGS: "));
        }
        for v in [
            json!({"status": "ok", "summary": "s"}),
            json!({"summary": "s"}),
            json!({"status": "done"}),
            json!({"status": "done", "summary": "s", "tests": "skipped"}),
            json!({"status": "done", "summary": "s", "files": "a.rs"}),
            json!({"status": "done", "summary": "s", "to": "p1"}),
        ] {
            assert!(args(v).is_err());
        }
    }

    #[test]
    fn rate_is_six_per_minute_per_caller() {
        let r = Reports::new(Arc::new(|_| {}));
        let t0 = Instant::now();
        for i in 0..6 {
            r.take("p2", t0 + Duration::from_secs(i)).unwrap();
        }
        let e = r
            .take("p2", t0 + Duration::from_millis(10_500))
            .unwrap_err();
        assert!(
            e.starts_with("REPORT_RATE: ") && e.contains("Wait 50 seconds") && e.ends_with('.'),
            "{e}"
        );
        assert!(r.take("p3", t0).is_ok());
        // Given back: the slot is free again.
        r.give_back("p2", t0 + Duration::from_secs(5));
        assert!(r.take("p2", t0 + Duration::from_secs(11)).is_ok());
        // The oldest leaves the window after 60 s.
        assert!(r.take("p2", t0 + Duration::from_secs(60)).is_ok());
    }
}
