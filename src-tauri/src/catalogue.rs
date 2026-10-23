//! Katalog agen (PRD AGT-01/AGT-02/AGT-05/AGT-06, SET-03, ARCHITECTURE §2–§4): definisi bawaan + katalog pengguna,
//! path manual, `PATH` dari shell login, deteksi instalasi + versi + metode login, nama kunci API di shell login, dan
//! pembentuk argv dari templat. Tidak membaca berkas kredensial atau config agen (D-006, D-023).

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fmt, thread};

use serde::{Deserialize, Serialize};

const BUILTIN: &str = include_str!("../catalogue.toml");
/// Katalog pengguna di direktori data aplikasi, format sama dengan `catalogue.toml` (AGT-06, D-023).
pub const USER_CATALOGUE: &str = "agents.toml";
/// Path manual per agen, `{"<id>": "/jalur/absolut"}` (SET-03, D-023).
pub const AGENT_PATHS: &str = "agent-paths.json";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
/// Keluaran shell login lebih dari ini dianggap gagal (rc yang membanjiri stdout).
const PROBE_MAX_OUTPUT: usize = 64 * 1024;
const MCP_URL: &str = "{mcp_url}";
const TASK: &str = "{task}";
const SESSION: &str = "{session}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    /// Agen baru melapor OSC 7501 setelah query `OSC 7501;?` dijawab (Claude, D-016).
    Osc7501Query,
    /// Agen melapor OSC 7501 tanpa query (Codex).
    Osc7501,
    /// Agen pengguna tanpa OSC 7501: tidak dipindai.
    None,
}

/// Perintah status login resmi milik CLI (AGT-05). argv-nya tetap di sini, bukan di katalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStatus {
    /// `claude auth status --json`
    ClaudeAuthStatus,
    /// `codex login status` (teks di stderr)
    CodexLoginStatus,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub id: String,
    pub name: String,
    pub binary: String,
    pub install: String,
    pub status: StatusSource,
    /// Kosong = agen diluncurkan tanpa injeksi MCP.
    #[serde(default)]
    pub mcp_args: Vec<String>,
    pub task_args: Vec<String>,
    /// Melanjutkan percakapan pane yang dipulihkan (D-030); `{session}` diganti id sesi berbentuk UUID. Kosong = agen
    /// selalu mulai baru.
    #[serde(default)]
    pub resume_args: Vec<String>,
    #[serde(default)]
    pub login_status: Option<LoginStatus>,
    /// Nama variabel kunci API yang dipakai agen ini; nilainya tidak pernah dibaca (AGT-05).
    #[serde(default)]
    pub api_key_env: Vec<String>,
    /// Katalog pengguna: `true` = sengaja menimpa agen bawaan ber-id sama.
    #[serde(default)]
    pub replace: bool,
    /// Dari `agent-paths.json`, bukan dari TOML.
    #[serde(skip)]
    pub path_override: Option<PathBuf>,
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

/// Isi `agents_info` (kontrak UI Settings): [`AgentInfo`] + path manual, metode login, dan nama kunci API.
#[derive(Debug, Clone, Serialize)]
pub struct AgentDetails {
    #[serde(flatten)]
    pub info: AgentInfo,
    pub path_override: Option<PathBuf>,
    pub login: Option<Login>,
    /// Nama variabel yang ada di environment shell login; agentdeck tidak meneruskannya (D-019).
    pub api_key_env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Login {
    pub method: String,
    pub detail: Option<String>,
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

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Direktori data aplikasi, diisi sekali saat aplikasi mulai. Tanpa ini hanya katalog bawaan yang dipakai.
pub fn set_data_dir(dir: PathBuf) {
    let _ = DATA_DIR.set(dir);
}

impl Catalogue {
    /// Katalog yang berlaku untuk peluncuran: bawaan + katalog pengguna + path manual (lihat [`Self::load_from`]).
    // ponytail: nama lama dipakai panes.rs; ganti ke `load_from` di sana saat berkas itu bebas disentuh.
    pub fn builtin() -> Result<Self, CatalogueError> {
        Self::load_from(DATA_DIR.get().map(PathBuf::as_path))
    }

    pub fn embedded() -> Result<Self, CatalogueError> {
        Self::parse(BUILTIN)
    }

    /// Katalog bawaan digabung `agents.toml` dan `agent-paths.json` di `dir`. Berkas pengguna yang rusak atau tidak
    /// sah diabaikan seluruhnya dengan peringatan; agen pengguna dengan id bawaan diabaikan kecuali `replace = true`.
    pub fn load_from(dir: Option<&Path>) -> Result<Self, CatalogueError> {
        let mut cat = Self::embedded()?;
        let Some(dir) = dir else { return Ok(cat) };
        let file = dir.join(USER_CATALOGUE);
        let user = match std::fs::read_to_string(&file) {
            Ok(src) => Self::parse(&src)
                .map_err(|e| eprintln!("agentdeck: {} diabaikan: {e}", file.display()))
                .ok(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("agentdeck: {} diabaikan: {e}", file.display());
                None
            }
        };
        for agent in user.map(|u| u.agents).unwrap_or_default() {
            match cat.agents.iter_mut().find(|a| a.id == agent.id) {
                Some(old) if agent.replace => {
                    eprintln!(
                        "agentdeck: agen bawaan `{}` ditimpa katalog pengguna",
                        agent.id
                    );
                    *old = agent;
                }
                Some(_) => eprintln!(
                    "agentdeck: agen `{}` di katalog pengguna diabaikan: id bawaan (pakai `replace = true`)",
                    agent.id
                ),
                None => cat.agents.push(agent),
            }
        }
        for (id, path) in read_paths(dir) {
            if let Some(agent) = cat.agents.iter_mut().find(|a| a.id == id) {
                agent.path_override = Some(path);
            }
        }
        Ok(cat)
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
        // Nama ini disisipkan ke skrip shell login (`probe_env_names`), jadi hanya karakter nama variabel.
        let env_name = |n: &String| {
            n.bytes()
                .enumerate()
                .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()))
                && !n.is_empty()
        };
        if !self.api_key_env.iter().all(env_name) {
            return Err("api_key_env hanya berisi nama variabel environment".into());
        }
        for arg in self
            .mcp_args
            .iter()
            .chain(&self.task_args)
            .chain(&self.resume_args)
        {
            if let Some(why) = bypass_flag(arg) {
                return Err(format!("flag bypass izin dilarang: `{arg}` ({why})"));
            }
        }
        if self.mcp_args.iter().any(|a| a.contains(TASK)) {
            return Err("`{task}` hanya boleh di task_args".into());
        }
        let sessions = self
            .resume_args
            .iter()
            .filter(|a| a.contains(SESSION))
            .count();
        if self
            .mcp_args
            .iter()
            .chain(&self.task_args)
            .any(|a| a.contains(SESSION))
            || (!self.resume_args.is_empty() && sessions != 1)
            || self
                .resume_args
                .iter()
                .any(|a| a.contains(TASK) || a.contains(MCP_URL))
        {
            return Err(
                "resume_args memuat `{session}` sekali; `{session}` hanya di resume_args".into(),
            );
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
    /// Tugas tidak pernah diurai shell atau templat: ia menjadi satu elemen argv apa pun isinya. `session` (sudah
    /// divalidasi UUID oleh pemanggil) mengisi `resume_args` sebelum tugas.
    pub fn build_args(
        &self,
        mcp_url: Option<&str>,
        task: Option<&str>,
        session: Option<&str>,
    ) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(url) = mcp_url {
            args.extend(self.mcp_args.iter().map(|a| a.replace(MCP_URL, url)));
        }
        if let Some(id) = session {
            args.extend(self.resume_args.iter().map(|a| a.replace(SESSION, id)));
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
        probe_login_path(&login_shell(), PROBE_TIMEOUT).unwrap_or_else(fallback_path)
    })
}

fn login_shell() -> String {
    env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// Menjalankan `$SHELL -l -i -c` (zsh non-interaktif tidak membaca `~/.zshrc`, D-019) yang mencetak `PATH` di antara
/// dua penanda. Keluaran lain dari berkas rc diabaikan. `None` bila gagal atau melewati batas waktu.
pub fn probe_login_path(shell: &str, timeout: Duration) -> Option<String> {
    // `"$PATH"` di fish 3 digabung dengan `:`, sama seperti sh/zsh.
    let path = probe_login_shell(shell, "printf '%s' \"$PATH\"", timeout)?;
    (!path.is_empty() && !path.contains(['\n', '\0'])).then_some(path)
}

/// Nama di `names` yang diekspor shell login (AGT-05). Nilainya hanya dilihat `printenv` di dalam shell dan dibuang ke
/// `/dev/null`; yang keluar dari shell hanya nama. `names` sudah divalidasi sebagai nama variabel ([`Agent::validate`]).
pub fn probe_env_names(shell: &str, names: &[&str], timeout: Duration) -> Vec<String> {
    let body: String = names
        .iter()
        .map(|n| format!("printenv {n} >/dev/null && printf '%s\\n' {n}\n"))
        .collect();
    probe_login_shell(shell, &body, timeout)
        .map(|out| {
            out.split_whitespace()
                .filter(|n| names.contains(n))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Isi `body` dijalankan di shell login; yang dicetaknya diambil dari antara dua penanda bernonce.
fn probe_login_shell(shell: &str, body: &str, timeout: Duration) -> Option<String> {
    let nonce = format!(
        "{}{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_nanos()
    );
    // Penanda dirakit oleh printf sehingga teks perintah sendiri (mis. bila rc menggemakan perintah) tidak memuatnya.
    let script = format!(
        "printf '\\n%s_%s\\n' AGENTDECK_BEGIN {nonce}\n{body}\nprintf '\\n%s_%s\\n' AGENTDECK_END {nonce}"
    );
    let mut cmd = base_command(shell, "/usr/bin:/bin:/usr/sbin:/sbin");
    cmd.args(["-l", "-i", "-c", &script]).stderr(Stdio::null());
    let out = capture(cmd, false, timeout, |out| {
        extract_marked(&String::from_utf8_lossy(out), &nonce).is_some()
    })?;
    extract_marked(&String::from_utf8_lossy(&out), &nonce).map(str::to_owned)
}

fn extract_marked<'a>(out: &'a str, nonce: &str) -> Option<&'a str> {
    let begin = format!("\nAGENTDECK_BEGIN_{nonce}\n");
    let end = format!("\nAGENTDECK_END_{nonce}\n");
    let start = out.find(&begin)? + begin.len();
    let len = out[start..].find(&end)?;
    Some(&out[start..start + len])
}

/// Menjalankan `cmd` dan membaca stdout (plus stderr bila `merge_stderr`) sampai EOF atau `done(keluaran)`. `None` bila
/// gagal, melewati batas waktu, atau keluaran lebih dari [`PROBE_MAX_OUTPUT`]. Proses dibunuh setelahnya.
fn capture(
    mut cmd: Command,
    merge_stderr: bool,
    timeout: Duration,
    done: impl Fn(&[u8]) -> bool,
) -> Option<Vec<u8>> {
    let (mut reader, writer) = std::io::pipe().ok()?;
    if merge_stderr {
        cmd.stderr(writer.try_clone().ok()?);
    }
    let mut child = cmd.stdin(Stdio::null()).stdout(writer).spawn().ok()?;
    // Ujung tulis milik kita ditutup agar EOF datang saat anak keluar.
    drop(cmd);
    let (tx, rx) = mpsc::sync_channel(4);
    // ponytail: bila cucu (daemon dari rc) menahan pipa, thread ini hidup sampai cucu itu keluar.
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n @ 1..) = reader.read(&mut buf) {
            if tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + timeout;
    let mut out = Vec::new();
    let result = loop {
        if done(&out) {
            break Some(out);
        }
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(chunk) if out.len() + chunk.len() <= PROBE_MAX_OUTPUT => out.extend(chunk),
            Err(mpsc::RecvTimeoutError::Disconnected) => break Some(out),
            _ => break None,
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn fallback_path() -> String {
    let home = env::var("HOME")
        .map(|h| format!("{h}/.local/bin:"))
        .unwrap_or_default();
    format!("{home}/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin")
}

/// Perintah dengan environment dari nol (D-014): hanya identitas pengguna, locale, dan `PATH` yang diberikan. Kunci
/// API tidak ikut, jadi status login yang dilaporkan sama dengan yang dilihat agen di pane.
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
    path.split(':')
        .filter(|dir| dir.starts_with('/'))
        .map(|dir| Path::new(dir).join(binary))
        .find(|p| check_executable(p).is_ok())
}

/// Syarat path manual (SET-03): absolut, ada, berkas biasa (setelah symlink), dapat dieksekusi.
pub fn check_executable(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    if !p.is_absolute() {
        return Err("path harus absolut".into());
    }
    let meta = p.metadata().map_err(|_| "berkas tidak ada".to_owned())?;
    if !meta.is_file() {
        return Err("bukan berkas".into());
    }
    if meta.permissions().mode() & 0o111 == 0 {
        return Err("tidak dapat dieksekusi".into());
    }
    Ok(())
}

/// Program yang dijalankan untuk `agent`: path manual bila diisi (harus sah, tidak jatuh diam-diam ke `PATH`), selain
/// itu hasil pencarian di `path`.
pub fn resolve(agent: &Agent, path: &str) -> Result<PathBuf, String> {
    match &agent.path_override {
        Some(p) => check_executable(p)
            .map(|()| p.clone())
            .map_err(|why| format!("Path manual {} tidak bisa dipakai: {why}.", p.display())),
        None => find_in_path(&agent.binary, path)
            .ok_or_else(|| format!("{} tidak ditemukan di PATH.", agent.binary)),
    }
}

/// Baris pertama `<binary> --version`, `None` bila gagal atau melewati batas waktu.
pub fn version(binary: &Path, path: &str, timeout: Duration) -> Option<String> {
    let mut cmd = base_command(binary, path);
    cmd.arg("--version").stderr(Stdio::null());
    let out = capture(cmd, false, timeout, |_| false)?;
    String::from_utf8_lossy(&out)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_owned)
}

/// Metode login lewat perintah status resmi CLI (AGT-05, D-006). Keluaran mentah tidak pernah disimpan atau dicatat.
pub fn login(kind: LoginStatus, binary: &Path, path: &str, timeout: Duration) -> Option<Login> {
    let mut cmd = base_command(binary, path);
    let out = match kind {
        LoginStatus::ClaudeAuthStatus => {
            cmd.args(["auth", "status", "--json"]).stderr(Stdio::null());
            capture(cmd, false, timeout, |_| false)?
        }
        LoginStatus::CodexLoginStatus => {
            cmd.args(["login", "status"]);
            capture(cmd, true, timeout, |_| false)?
        }
    };
    parse_login(kind, &String::from_utf8_lossy(&out))
}

/// `claude auth status --json` → `authMethod` (`"none"` bila belum login) + `subscriptionType`. Codex → hanya frasa
/// tetap; sisa baris (pada mode kunci API memuat potongan kunci yang disamarkan) dibuang.
fn parse_login(kind: LoginStatus, out: &str) -> Option<Login> {
    match kind {
        LoginStatus::ClaudeAuthStatus => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Status {
                logged_in: bool,
                auth_method: Option<String>,
                subscription_type: Option<String>,
            }
            let s: Status = serde_json::from_str(out).ok()?;
            Some(Login {
                method: match s.auth_method {
                    Some(m) if s.logged_in => m,
                    _ => "none".into(),
                },
                detail: s.subscription_type.filter(|_| s.logged_in),
            })
        }
        LoginStatus::CodexLoginStatus => {
            let method = if out.contains("Logged in using ChatGPT") {
                "chatgpt"
            } else if out.contains("Logged in using an API key") {
                "api_key"
            } else if out.contains("Not logged in") {
                "none"
            } else {
                return None;
            };
            Some(Login {
                method: method.into(),
                detail: None,
            })
        }
    }
}

pub fn detect(agent: &Agent, path: &str) -> AgentInfo {
    let found = resolve(agent, path).ok();
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

fn details(agent: &Agent, path: &str, api_keys: &[String]) -> AgentDetails {
    let info = detect(agent, path);
    let login = info
        .path
        .as_deref()
        .zip(agent.login_status)
        .and_then(|(bin, kind)| login(kind, bin, path, PROBE_TIMEOUT));
    AgentDetails {
        path_override: agent.path_override.clone(),
        login,
        api_key_env: agent
            .api_key_env
            .iter()
            .filter(|n| api_keys.contains(n))
            .cloned()
            .collect(),
        info,
    }
}

/// Isi perintah `agents_list` (T1.7): setiap agen di katalog, terpasang atau tidak, beserta versi dan perintah pasang.
pub fn agents_list() -> Result<Vec<AgentInfo>, CatalogueError> {
    let path = login_path();
    Ok(Catalogue::builtin()?
        .agents
        .iter()
        .map(|a| detect(a, path))
        .collect())
}

/// Isi perintah `agents_info` (T4.4/T4.5): [`agents_list`] + path manual, metode login, nama kunci API di shell login.
pub fn agents_info() -> Result<Vec<AgentDetails>, CatalogueError> {
    let cat = Catalogue::builtin()?;
    let path = login_path();
    let mut names: Vec<&str> = cat
        .agents
        .iter()
        .flat_map(|a| a.api_key_env.iter().map(String::as_str))
        .collect();
    names.sort_unstable();
    names.dedup();
    let api_keys = probe_env_names(&login_shell(), &names, PROBE_TIMEOUT);
    // Tiap agen menjalankan `--version` dan status login; paralel agar Settings tidak menunggu jumlahnya.
    Ok(thread::scope(|s| {
        let handles: Vec<_> = cat
            .agents
            .iter()
            .map(|a| s.spawn(|| details(a, path, &api_keys)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    }))
}

/// `agent_set_path` (SET-03): simpan (`Some`) atau hapus (`None`) path manual agen `id`, lalu kembalikan infonya.
pub fn agent_set_path(id: &str, path: Option<&str>) -> Result<AgentDetails, String> {
    let dir = DATA_DIR
        .get()
        .ok_or("Direktori data aplikasi tidak tersedia.")?;
    set_path_in(dir, id, path)?;
    agents_info()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.info.id == id)
        .ok_or_else(|| format!("Agen `{id}` tidak ada di katalog."))
}

fn read_paths(dir: &Path) -> HashMap<String, PathBuf> {
    let file = dir.join(AGENT_PATHS);
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            eprintln!("agentdeck: {} diabaikan: {e}", file.display());
            HashMap::new()
        }),
        Err(_) => HashMap::new(),
    }
}

fn set_path_in(dir: &Path, id: &str, path: Option<&str>) -> Result<(), String> {
    if Catalogue::load_from(Some(dir))
        .map_err(|e| e.to_string())?
        .get(id)
        .is_none()
    {
        return Err(format!("Agen `{id}` tidak ada di katalog."));
    }
    let mut paths = read_paths(dir);
    match path {
        Some(p) => {
            check_executable(Path::new(p)).map_err(|why| format!("Path {p} ditolak: {why}."))?;
            paths.insert(id.to_owned(), p.into());
        }
        None => {
            paths.remove(id);
        }
    }
    let json = serde_json::to_vec_pretty(&paths).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{AGENT_PATHS}.tmp"));
    std::fs::create_dir_all(dir)
        .and_then(|()| std::fs::write(&tmp, json))
        .and_then(|()| std::fs::rename(&tmp, dir.join(AGENT_PATHS)))
        .map_err(|e| format!("Path manual tidak tersimpan: {e}"))
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
        let cat = Catalogue::embedded().unwrap();
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

    /// D-030: `{session}` only in `resume_args`, once; resume args are checked for bypass flags like the rest.
    #[test]
    fn resume_args_template_is_checked() {
        let base = agent_toml("", r#"["--", "{task}"]"#);
        let with = |resume: &str| format!("{base}resume_args = {resume}\n");
        assert!(Catalogue::parse(&with(r#"["--resume", "{session}"]"#)).is_ok());
        for bad in [
            r#"["--resume"]"#,
            r#"["--resume", "{session}", "{session}"]"#,
            r#"["--resume", "{session}", "{task}"]"#,
            r#"["--resume", "{session}", "--yolo"]"#,
        ] {
            assert!(Catalogue::parse(&with(bad)).is_err(), "{bad}");
        }
        assert!(Catalogue::parse(&agent_toml("", r#"["{session}", "--", "{task}"]"#)).is_err());
    }

    #[test]
    fn extracts_text_between_markers_ignoring_noise() {
        let out = "Welcome to zsh!\nAGENTDECK_BEGIN_42 tidak di awal baris\n\
                   \nAGENTDECK_BEGIN_42\n/a/bin:/usr/bin\nAGENTDECK_END_42\nnvm: bye\n";
        assert_eq!(extract_marked(out, "42"), Some("/a/bin:/usr/bin"));
        assert_eq!(extract_marked(out, "43"), None, "nonce lain");
        assert_eq!(
            extract_marked("\nAGENTDECK_BEGIN_42\n/a/bin", "42"),
            None,
            "terpotong"
        );
        assert_eq!(
            extract_marked("\nAGENTDECK_BEGIN_42\n\nAGENTDECK_END_42\n", "42"),
            Some(""),
            "kosong"
        );
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("agentdeck-cat-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    const GEMINI: &str = r#"[[agents]]
id = "gemini"
name = "Gemini CLI"
binary = "gemini"
install = "npm install -g @google/gemini-cli"
status = "none"
task_args = ["--", "{task}"]
api_key_env = ["GEMINI_API_KEY"]
"#;

    #[test]
    fn user_catalogue_merges_with_builtin() {
        let dir = temp_dir("merge");
        let ids = |cat: &Catalogue| cat.agents.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(&Catalogue::load_from(Some(&dir)).unwrap()),
            ["claude-code", "codex"]
        );

        // Tanpa `replace`, id bawaan diabaikan; agen baru tetap masuk.
        let clash = GEMINI.replace("gemini\"\nname", "codex\"\nname");
        std::fs::write(
            dir.join(USER_CATALOGUE),
            format!(
                "{GEMINI}{}",
                clash.replace("id = \"gemini\"", "id = \"codex\"")
            ),
        )
        .unwrap();
        let cat = Catalogue::load_from(Some(&dir)).unwrap();
        assert_eq!(ids(&cat), ["claude-code", "codex", "gemini"]);
        assert_eq!(cat.get("codex").unwrap().binary, "codex");
        let gemini = cat.get("gemini").unwrap();
        assert!(gemini.mcp_args.is_empty() && gemini.login_status.is_none());
        assert_eq!(gemini.status, StatusSource::None);

        let replaced = GEMINI.replace("id = \"gemini\"", "id = \"codex\"\nreplace = true");
        std::fs::write(dir.join(USER_CATALOGUE), replaced).unwrap();
        let cat = Catalogue::load_from(Some(&dir)).unwrap();
        assert_eq!(ids(&cat), ["claude-code", "codex"]);
        assert_eq!(cat.get("codex").unwrap().binary, "gemini");
    }

    #[test]
    fn bad_user_catalogue_is_ignored() {
        let dir = temp_dir("bad");
        for src in [
            "[[agents]\nid = ".to_owned(),
            GEMINI.replace(
                "status = \"none\"",
                "status = \"none\"\nmcp_args = [\"--yolo\"]",
            ),
            GEMINI.replace(
                "[\"--\", \"{task}\"]",
                "[\"--dangerously-skip-permissions\", \"--\", \"{task}\"]",
            ),
            GEMINI.replace(
                "[\"--\", \"{task}\"]",
                "[\"-a\", \"never\", \"--\", \"{task}\"]",
            ),
            GEMINI.replace("GEMINI_API_KEY", "X; rm -rf ~"),
            format!("{GEMINI}{GEMINI}"),
            GEMINI.replace("binary = \"gemini\"", "binary = \"/usr/bin/gemini\""),
        ] {
            std::fs::write(dir.join(USER_CATALOGUE), &src).unwrap();
            let cat = Catalogue::load_from(Some(&dir)).unwrap();
            assert_eq!(cat.agents.len(), 2, "{src}");
        }
        // Agen sah di berkas yang sama ikut diabaikan: berkas ditolak utuh.
        std::fs::write(
            dir.join(USER_CATALOGUE),
            format!(
                "{GEMINI}{}",
                GEMINI
                    .replace("gemini\"\nname", "g2\"\nname")
                    .replace("[\"--\",", "[\"--yolo\", \"--\",")
            ),
        )
        .unwrap();
        assert!(
            Catalogue::load_from(Some(&dir))
                .unwrap()
                .get("gemini")
                .is_none()
        );
    }

    #[test]
    fn manual_path_is_validated_and_applied() {
        let dir = temp_dir("paths");
        let exe = script(&dir, "my-claude", "exit 0");
        let plain = dir.join("plain");
        std::fs::write(&plain, "x").unwrap();
        let exe_s = exe.to_str().unwrap();
        for (id, p) in [
            ("claude-code", "relative/claude"),
            ("claude-code", "/nonexistent/claude"),
            ("claude-code", plain.to_str().unwrap()),
            ("claude-code", dir.to_str().unwrap()),
            ("nope", exe_s),
        ] {
            assert!(set_path_in(&dir, id, Some(p)).is_err(), "{id} {p}");
        }
        assert!(!dir.join(AGENT_PATHS).exists());

        set_path_in(&dir, "claude-code", Some(exe_s)).unwrap();
        let cat = Catalogue::load_from(Some(&dir)).unwrap();
        let claude = cat.get("claude-code").unwrap();
        assert_eq!(claude.path_override.as_deref(), Some(exe.as_path()));
        assert_eq!(resolve(claude, "/nonexistent").unwrap(), exe);
        assert!(cat.get("codex").unwrap().path_override.is_none());

        // Berkas hilang setelah disimpan: tidak jatuh diam-diam ke PATH.
        std::fs::remove_file(&exe).unwrap();
        assert!(resolve(claude, "/bin").unwrap_err().contains("Path manual"));

        set_path_in(&dir, "claude-code", None).unwrap();
        assert!(
            Catalogue::load_from(Some(&dir))
                .unwrap()
                .get("claude-code")
                .unwrap()
                .path_override
                .is_none()
        );

        std::fs::write(dir.join(AGENT_PATHS), "{rusak").unwrap();
        assert!(Catalogue::load_from(Some(&dir)).is_ok());
    }

    #[test]
    fn parses_login_status_output() {
        let claude = r#"{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "email": "u@example.com",
  "orgId": "org-1",
  "subscriptionType": "team"
}"#;
        assert_eq!(
            parse_login(LoginStatus::ClaudeAuthStatus, claude),
            Some(Login {
                method: "claude.ai".into(),
                detail: Some("team".into())
            })
        );
        let out = r#"{"loggedIn": false, "authMethod": "none", "apiProvider": "firstParty"}"#;
        assert_eq!(
            parse_login(LoginStatus::ClaudeAuthStatus, out),
            Some(Login {
                method: "none".into(),
                detail: None
            })
        );
        assert_eq!(parse_login(LoginStatus::ClaudeAuthStatus, "Error: x"), None);

        let codex = |out| parse_login(LoginStatus::CodexLoginStatus, out).map(|l| l.method);
        assert_eq!(
            codex("Logged in using ChatGPT\n").as_deref(),
            Some("chatgpt")
        );
        let api = "Logged in using an API key - sk-proj-***ABCD\n";
        let login = parse_login(LoginStatus::CodexLoginStatus, api).unwrap();
        assert_eq!(
            login,
            Login {
                method: "api_key".into(),
                detail: None
            }
        );
        assert!(!format!("{login:?}").contains("ABCD"));
        assert_eq!(codex("Not logged in\n").as_deref(), Some("none"));
        assert_eq!(codex("error: unknown subcommand").as_deref(), None);
    }

    #[test]
    fn login_runs_cli_status_command_with_clean_env() {
        let dir = temp_dir("login");
        // Pura-pura CLI: Codex melapor di stderr, Claude JSON di stdout.
        let bin = script(
            &dir,
            "fake",
            r#"[ "$1 $2" = "login status" ] && echo "Logged in using ChatGPT" >&2
[ "$1 $2 $3" = "auth status --json" ] && echo '{"loggedIn":true,"authMethod":"claude.ai"}'"#,
        );
        let login = |k| login(k, &bin, "/usr/bin:/bin", PROBE_TIMEOUT).map(|l| l.method);
        assert_eq!(
            login(LoginStatus::CodexLoginStatus).as_deref(),
            Some("chatgpt")
        );
        assert_eq!(
            login(LoginStatus::ClaudeAuthStatus).as_deref(),
            Some("claude.ai")
        );
    }

    #[test]
    fn detects_api_key_names_without_values() {
        let dir = temp_dir("keys");
        // "Shell login" yang mengekspor kunci lalu menjalankan skrip probe (`-l -i -c <skrip>`).
        let shell = script(
            &dir,
            "shell",
            "export ANTHROPIC_API_KEY=sk-ant-SECRET-VALUE OPENAI_API_KEY=\nexec /bin/sh -c \"$4\"",
        );
        let names = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"];
        let found = probe_env_names(shell.to_str().unwrap(), &names, PROBE_TIMEOUT);
        // Diekspor walau kosong tetap terdeteksi: CLI melihatnya juga.
        assert_eq!(found, ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]);
        assert!(!format!("{found:?}").contains("SECRET"));
        assert!(probe_env_names(shell.to_str().unwrap(), &[], PROBE_TIMEOUT).is_empty());

        let mut cat = Catalogue::embedded().unwrap();
        let claude = cat.agents.remove(0);
        let d = details(&claude, "/nonexistent", &found);
        assert_eq!(d.api_key_env, ["ANTHROPIC_API_KEY"]);
        assert!(d.login.is_none() && !d.info.installed);
        let json = serde_json::to_string(&d).unwrap();
        assert!(!json.contains("SECRET") && !format!("{d:?}").contains("SECRET"));
        assert_eq!(
            json,
            r#"{"id":"claude-code","name":"Claude Code","installed":false,"path":null,"version":null,"install":"curl -fsSL https://claude.ai/install.sh | bash","path_override":null,"login":null,"api_key_env":["ANTHROPIC_API_KEY"]}"#
        );
    }

    #[test]
    fn builds_argv_with_task_after_double_dash() {
        let cat = Catalogue::embedded().unwrap();
        let codex = cat.get("codex").unwrap();
        let args = codex.build_args(
            Some("http://127.0.0.1:4711/mcp"),
            Some("--yolo; rm -rf ~ {mcp_url}"),
            None,
        );
        assert!(
            args.contains(&r#"mcp_servers.agentdeck.url="http://127.0.0.1:4711/mcp""#.to_owned())
        );
        assert_eq!(
            &args[args.len() - 2..],
            ["--", "--yolo; rm -rf ~ {mcp_url}"]
        );

        let claude = cat.get("claude-code").unwrap();
        let args = claude.build_args(Some("http://127.0.0.1:4711/mcp"), Some("-p halo"), None);
        let id = "3fe10b28-c0b7-47aa-8cb4-dfe4947debd1";
        let resumed = claude.build_args(None, None, Some(id));
        assert_eq!(resumed, ["--resume", id]);
        assert!(
            codex.build_args(None, None, Some(id)).is_empty(),
            "codex has no resume_args"
        );
        assert_eq!(args[0], "--mcp-config");
        assert!(args[1].contains("${AGENTDECK_MCP_URL}") && args[1].contains("${AGENTDECK_TOKEN}"));
        assert_eq!(&args[args.len() - 2..], ["--", "-p halo"]);

        assert!(claude.build_args(None, None, None).is_empty());
        assert!(claude.build_args(None, Some(""), None).is_empty());
        assert_eq!(claude.build_args(None, Some("x"), None), ["--", "x"]);
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
        for d in agents_info().unwrap() {
            eprintln!("{}", serde_json::to_string(&d).unwrap());
        }
    }
}
