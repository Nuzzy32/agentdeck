//! Katalog agen (PRD AGT-01/AGT-02, ARCHITECTURE §2–§4): definisi bawaan, `PATH` dari shell login, deteksi
//! instalasi + versi, dan pembentuk argv dari templat. Tidak membaca berkas kredensial atau config agen.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fmt, thread};

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../catalogue.toml");
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Keluaran shell login lebih dari ini dianggap gagal (rc yang membanjiri stdout).
const PROBE_MAX_OUTPUT: usize = 64 * 1024;
const MCP_URL: &str = "{mcp_url}";
const TASK: &str = "{task}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    /// Agen baru melapor OSC 7501 setelah query `OSC 7501;?` dijawab (Claude, D-016).
    Osc7501Query,
    /// Agen melapor OSC 7501 tanpa query (Codex).
    Osc7501,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub install: String,
    pub status: StatusSource,
    pub mcp_args: Vec<String>,
    pub task_args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub install: String,
}

#[derive(Debug)]
pub enum CatalogueError {
    Parse(String),
    Invalid { agent: String, reason: String },
}

impl fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "katalog agen tidak bisa dibaca: {e}"),
            Self::Invalid { agent, reason } => {
                write!(f, "agen `{agent}` di katalog tidak sah: {reason}")
            }
        }
    }
}

impl std::error::Error for CatalogueError {}

impl Catalogue {
    pub fn builtin() -> Result<Self, CatalogueError> {
        Self::parse(BUILTIN)
    }

    pub fn parse(src: &str) -> Result<Self, CatalogueError> {
        let cat: Self = toml::from_str(src).map_err(|e| CatalogueError::Parse(e.to_string()))?;
        let mut ids = HashSet::new();
        for agent in &cat.agents {
            agent.validate().map_err(|reason| CatalogueError::Invalid {
                agent: agent.id.clone(),
                reason,
            })?;
            if !ids.insert(agent.id.as_str()) {
                return Err(CatalogueError::Invalid {
                    agent: agent.id.clone(),
                    reason: "id ganda".into(),
                });
            }
        }
        Ok(cat)
    }

    pub fn get(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|a| a.id == id)
    }
}

impl Agent {
    fn validate(&self) -> Result<(), String> {
        let id_ok = !self.id.is_empty()
            && self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !id_ok {
            return Err("id hanya boleh huruf kecil, angka, dan `-`".into());
        }
        if self.name.trim().is_empty() || self.install.trim().is_empty() {
            return Err("name dan install wajib diisi".into());
        }
        // Nama polos agar yang dijalankan selalu hasil pencarian di PATH, bukan jalur relatif.
        if self.binary.is_empty()
            || self.binary.contains(['/', '\0'])
            || self.binary.contains(char::is_whitespace)
        {
            return Err("binary harus nama perintah tanpa `/` atau spasi".into());
        }
        for arg in self.mcp_args.iter().chain(&self.task_args) {
            if let Some(why) = bypass_flag(arg) {
                return Err(format!("flag bypass izin dilarang: `{arg}` ({why})"));
            }
        }
        if self.mcp_args.iter().any(|a| a.contains(TASK)) {
            return Err("`{task}` hanya boleh di task_args".into());
        }
        if self.task_args.iter().any(|a| a.contains(MCP_URL)) {
            return Err("`{mcp_url}` hanya boleh di mcp_args".into());
        }
        // Tugas selalu argumen terakhir, utuh, tepat setelah `--` (D-019).
        let n = self.task_args.len();
        let tail_ok = n >= 2 && self.task_args[n - 2] == "--" && self.task_args[n - 1] == TASK;
        if !tail_ok || self.task_args.iter().filter(|a| a.contains(TASK)).count() != 1 {
            return Err(
                "task_args harus diakhiri \"--\", \"{task}\" dan memuat `{task}` sekali".into(),
            );
        }
        Ok(())
    }

    /// Argumen setelah nama program. Tanpa `mcp_url` tidak ada injeksi MCP; tugas kosong tidak menambah `--`.
    /// Tugas tidak pernah diurai shell atau templat: ia menjadi satu elemen argv apa pun isinya.
    pub fn build_args(&self, mcp_url: Option<&str>, task: Option<&str>) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(url) = mcp_url {
            args.extend(self.mcp_args.iter().map(|a| a.replace(MCP_URL, url)));
        }
        if let Some(task) = task.filter(|t| !t.is_empty()) {
            for a in &self.task_args {
                args.push(if a == TASK {
                    task.to_owned()
                } else {
                    a.clone()
                });
            }
        }
        args
    }
}

/// Alasan bila `arg` mematikan izin agen (AGENTS.md aturan 4), `None` bila aman.
fn bypass_flag(arg: &str) -> Option<&'static str> {
    let lower = arg.to_ascii_lowercase();
    const SUBSTRINGS: [(&str, &str); 6] = [
        ("dangerously", "--dangerously-*"),
        ("yolo", "--yolo"),
        ("bypasspermissions", "bypassPermissions"),
        ("danger-full-access", "sandbox tanpa batas"),
        ("approval_policy", "approval_policy Codex"),
        ("sandbox_mode", "sandbox_mode Codex"),
    ];
    if let Some((_, why)) = SUBSTRINGS.iter().find(|(s, _)| lower.contains(s)) {
        return Some(why);
    }
    const FLAGS: [&str; 4] = [
        "--ask-for-approval",
        "--sandbox",
        "--permission-mode",
        "--full-auto",
    ];
    let name = lower.split('=').next().unwrap_or_default();
    if FLAGS.contains(&name) {
        return Some("approval/sandbox mengikuti setelan pengguna");
    }
    // `-a never`, `-anever`, `-s ...`: flag pendek Codex untuk approval dan sandbox.
    if !lower.starts_with("--") && (lower.starts_with("-a") || lower.starts_with("-s")) {
        return Some("-a/-s tidak dipakai");
    }
    None
}

/// `PATH` dari shell login, diambil sekali per proses (ARCHITECTURE §3 langkah 2).
pub fn login_path() -> &'static str {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let shell = env::var("SHELL")
            .ok()
            .filter(|s| s.starts_with('/'))
            .unwrap_or_else(|| "/bin/zsh".into());
        probe_login_path(&shell, PROBE_TIMEOUT).unwrap_or_else(fallback_path)
    })
}

/// Menjalankan `$SHELL -l -i -c` (zsh non-interaktif tidak membaca `~/.zshrc`, D-019) yang mencetak `PATH` di antara
/// dua penanda. Keluaran lain dari berkas rc diabaikan. `None` bila gagal atau melewati batas waktu.
pub fn probe_login_path(shell: &str, timeout: Duration) -> Option<String> {
    let nonce = format!(
        "{}{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
    );
    // Penanda dirakit oleh printf sehingga teks perintah sendiri (mis. bila rc menggemakan perintah) tidak memuatnya.
    // `"$PATH"` di fish 3 digabung dengan `:`, sama seperti sh/zsh.
    let script = format!(
        "printf '\\n%s_%s\\n%s\\n%s_%s\\n' AGENTDECK_PATH_BEGIN {nonce} \"$PATH\" AGENTDECK_PATH_END {nonce}"
    );
    let mut child = base_command(shell, "/usr/bin:/bin:/usr/sbin:/sbin")
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::sync_channel(4);
    // ponytail: bila cucu shell (daemon dari rc) menahan pipa, thread ini hidup sampai cucu itu keluar.
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = stdout.read(&mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    let path = loop {
        if let Some(p) = extract_marked_path(&String::from_utf8_lossy(&out), &nonce) {
            break Some(p);
        }
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(chunk) if out.len() + chunk.len() <= PROBE_MAX_OUTPUT => out.extend(chunk),
            _ => break None,
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    path
}

fn extract_marked_path(out: &str, nonce: &str) -> Option<String> {
    let begin = format!("\nAGENTDECK_PATH_BEGIN_{nonce}\n");
    let end = format!("\nAGENTDECK_PATH_END_{nonce}\n");
    let start = out.find(&begin)? + begin.len();
    let len = out[start..].find(&end)?;
    let path = &out[start..start + len];
    (!path.is_empty() && !path.contains(['\n', '\0'])).then(|| path.to_owned())
}

fn fallback_path() -> String {
    let home = env::var("HOME")
        .map(|h| format!("{h}/.local/bin:"))
        .unwrap_or_default();
    format!("{home}/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
}

/// Perintah dengan environment dari nol (D-014): hanya identitas pengguna, locale, dan `PATH` yang diberikan.
fn base_command(program: impl AsRef<std::ffi::OsStr>, path: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env_clear();
    for key in ["HOME", "USER", "LOGNAME", "SHELL", "LANG", "TMPDIR"] {
        if let Some(v) = env::var_os(key) {
            cmd.env(key, v);
        }
    }
    cmd.env("PATH", path).env("TERM", "dumb");
    cmd
}

/// Jalur absolut `binary` pertama yang bisa dieksekusi di `path`. Entri relatif diabaikan.
pub fn find_in_path(binary: &str, path: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    path.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| Path::new(dir).join(binary))
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// Baris pertama `<binary> --version`, `None` bila gagal atau melewati batas waktu.
pub fn version(binary: &Path, path: &str, timeout: Duration) -> Option<String> {
    let mut child = base_command(binary, path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // ponytail: keluaran --version kecil, jadi aman dibaca setelah proses keluar tanpa takut pipa penuh.
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

pub fn detect(agent: &Agent, path: &str) -> AgentInfo {
    let found = find_in_path(&agent.binary, path);
    AgentInfo {
        id: agent.id.clone(),
        name: agent.name.clone(),
        installed: found.is_some(),
        version: found
            .as_deref()
            .and_then(|p| version(p, path, PROBE_TIMEOUT)),
        path: found,
        install: agent.install.clone(),
    }
}

/// Isi perintah `agents_list` (T1.7): setiap agen bawaan, terpasang atau tidak, beserta versi dan perintah pasang.
pub fn agents_list() -> Result<Vec<AgentInfo>, CatalogueError> {
    let path = login_path();
    Ok(Catalogue::builtin()?
        .agents
        .iter()
        .map(|a| detect(a, path))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_toml(extra_mcp: &str, task: &str) -> String {
        format!(
            r#"[[agents]]
id = "x"
name = "X"
binary = "x"
install = "npm i -g x"
status = "osc7501"
mcp_args = ["--mcp", "{{mcp_url}}"{extra_mcp}]
task_args = {task}
"#
        )
    }

    #[test]
    fn builtin_catalogue_parses() {
        let cat = Catalogue::builtin().unwrap();
        let claude = cat.get("claude-code").unwrap();
        assert_eq!(claude.binary, "claude");
        assert_eq!(claude.status, StatusSource::Osc7501Query);
        let codex = cat.get("codex").unwrap();
        assert_eq!(codex.binary, "codex");
        assert_eq!(codex.status, StatusSource::Osc7501);
        assert!(
            codex
                .mcp_args
                .iter()
                .any(|a| a == "mcp_servers.agentdeck.tool_timeout_sec=330")
        );
    }

    #[test]
    fn rejects_bypass_flags() {
        for flag in [
            "--dangerously-skip-permissions",
            "--dangerously-bypass-approvals-and-sandbox",
            "--allow-dangerously-skip-permissions",
            "--yolo",
            "--permission-mode=bypassPermissions",
            "bypassPermissions",
            "--permission-mode",
            "-a",
            "-anever",
            "--ask-for-approval=never",
            "-s",
            "--sandbox",
            "--full-auto",
            "approval_policy=\"never\"",
            "sandbox_mode=\"danger-full-access\"",
        ] {
            let src = agent_toml(
                &format!(", {}", toml::Value::from(flag)),
                r#"["--", "{task}"]"#,
            );
            match Catalogue::parse(&src) {
                Err(CatalogueError::Invalid { reason, .. }) => {
                    assert!(reason.contains("bypass"), "{flag}: {reason}")
                }
                other => panic!("{flag} lolos: {other:?}"),
            }
        }
        // Mode approve khusus tool agentdeck (D-008) bukan bypass.
        let ok = agent_toml(
            r#", "-c", 'x.default_tools_approval_mode="approve"'"#,
            r#"["--", "{task}"]"#,
        );
        assert!(Catalogue::parse(&ok).is_ok());
    }

    #[test]
    fn rejects_bad_task_template_and_fields() {
        for task in [
            r#"["{task}"]"#,
            r#"["{task}", "--"]"#,
            r#"["--", "{task}", "x"]"#,
            r#"["-p", "{task}", "--", "{task}"]"#,
            "[]",
        ] {
            assert!(Catalogue::parse(&agent_toml("", task)).is_err(), "{task}");
        }
        let src = agent_toml("", r#"["--", "{task}"]"#);
        assert!(Catalogue::parse(&src.replace("binary = \"x\"", "binary = \"./x\"")).is_err());
        assert!(
            Catalogue::parse(&src.replace("status = \"osc7501\"", "status = \"hooks\"")).is_err()
        );
        assert!(Catalogue::parse(&format!("{src}\nunknown = 1")).is_err());
        assert!(
            Catalogue::parse(&format!("{src}{src}")).is_err(),
            "id ganda"
        );
    }

    #[test]
    fn extracts_path_between_markers_ignoring_noise() {
        let out = "Welcome to zsh!\nAGENTDECK_PATH_BEGIN_42 tidak di awal baris\n\
                   \nAGENTDECK_PATH_BEGIN_42\n/a/bin:/usr/bin\nAGENTDECK_PATH_END_42\nnvm: bye\n";
        assert_eq!(
            extract_marked_path(out, "42").as_deref(),
            Some("/a/bin:/usr/bin")
        );
        assert_eq!(extract_marked_path(out, "43"), None, "nonce lain");
        assert_eq!(
            extract_marked_path("\nAGENTDECK_PATH_BEGIN_42\n/a/bin", "42"),
            None,
            "terpotong"
        );
        assert_eq!(
            extract_marked_path("\nAGENTDECK_PATH_BEGIN_42\n\nAGENTDECK_PATH_END_42\n", "42"),
            None,
            "kosong"
        );
    }

    #[test]
    fn builds_argv_with_task_after_double_dash() {
        let cat = Catalogue::builtin().unwrap();
        let codex = cat.get("codex").unwrap();
        let args = codex.build_args(
            Some("http://127.0.0.1:4711/mcp"),
            Some("--yolo; rm -rf ~ {mcp_url}"),
        );
        assert!(
            args.contains(&r#"mcp_servers.agentdeck.url="http://127.0.0.1:4711/mcp""#.to_owned())
        );
        assert_eq!(
            &args[args.len() - 2..],
            ["--", "--yolo; rm -rf ~ {mcp_url}"]
        );

        let claude = cat.get("claude-code").unwrap();
        let args = claude.build_args(Some("http://127.0.0.1:4711/mcp"), Some("-p halo"));
        assert_eq!(args[0], "--mcp-config");
        assert!(args[1].contains("${AGENTDECK_MCP_URL}") && args[1].contains("${AGENTDECK_TOKEN}"));
        assert_eq!(&args[args.len() - 2..], ["--", "-p halo"]);

        assert!(claude.build_args(None, None).is_empty());
        assert!(claude.build_args(None, Some("")).is_empty());
        assert_eq!(claude.build_args(None, Some("x")), ["--", "x"]);
    }

    #[test]
    fn probe_gives_up_on_endless_noise() {
        // `yes` berperan sebagai shell yang terus mencetak sampah tanpa penanda dan tidak pernah keluar.
        let started = Instant::now();
        assert_eq!(
            probe_login_path("/usr/bin/yes", Duration::from_millis(300)),
            None
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(probe_login_path("/nonexistent/shell", PROBE_TIMEOUT), None);
    }

    #[test]
    fn probe_reads_path_from_real_sh() {
        let path = probe_login_path("/bin/bash", PROBE_TIMEOUT).expect("PATH dari bash -l -i -c");
        assert!(path.split(':').any(|d| d == "/usr/bin"), "{path}");
    }

    #[test]
    fn finds_executable_in_absolute_path_entries_only() {
        assert_eq!(
            find_in_path("sh", "relative:/nonexistent:/bin"),
            Some(PathBuf::from("/bin/sh"))
        );
        assert_eq!(find_in_path("sh", "bin"), None);
        assert_eq!(find_in_path("hosts", "/etc"), None, "bukan executable");
    }

    #[test]
    #[ignore = "menjalankan shell login pengguna; jalankan dengan --ignored"]
    fn real_login_shell_and_agents() {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let started = Instant::now();
        let path = probe_login_path(&shell, PROBE_TIMEOUT).expect("PATH dari shell login");
        eprintln!(
            "{shell}: {} entri PATH dalam {:?}",
            path.split(':').count(),
            started.elapsed()
        );
        assert!(path.split(':').any(|d| d == "/usr/bin"));
        for info in agents_list().unwrap() {
            eprintln!(
                "{} terpasang={} path={:?} versi={:?}",
                info.id, info.installed, info.path, info.version
            );
        }
    }
}
