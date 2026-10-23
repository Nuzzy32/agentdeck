//! Argv dan environment untuk satu pane (ARCHITECTURE §3–§4). Program dijalankan langsung tanpa shell (D-009) dengan
//! environment yang disusun dari nol (D-014), plus injeksi MCP per peluncuran untuk pane agen (D-005, T2.3).

use std::ffi::OsString;
use std::fmt;

use serde::Serialize;

use crate::catalogue::{self, Catalogue, StatusSource};
use crate::tokens::Token;

/// Satu-satunya variabel yang diambil dari environment agentdeck (D-014, D-019). `PATH` datang dari shell login.
pub const INHERIT: [&str; 7] = [
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TMPDIR",
    "SSH_AUTH_SOCK",
];
/// Aplikasi yang dibuka dari Finder tidak punya `LANG`; tanpa locale UTF-8 shell dan TUI salah membaca karakter
/// non-ASCII. Terminal.app mengisi nilai yang sama.
const FALLBACK_LANG: &str = "en_US.UTF-8";

/// Yang dijalankan di PTY: `argv[0]` jalur absolut, env lengkap (anak tidak mewarisi apa pun di luar ini).
#[derive(Clone, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    /// Agen melapor status lewat OSC 7501 (semua agen bawaan); shell tidak dipindai.
    pub osc_status: bool,
}

/// Nilai env tidak dicetak: `AGENTDECK_TOKEN` ada di sana (AGENTS.md aturan 6).
impl fmt::Debug for Launch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Launch")
            .field("argv", &self.argv)
            .field("env", &self.env.iter().map(|(k, _)| k).collect::<Vec<_>>())
            .field("osc_status", &self.osc_status)
            .finish()
    }
}

/// Injeksi MCP satu pane agen (ARCHITECTURE §4). Token hanya masuk env, tidak pernah argv.
pub struct Mcp<'a> {
    /// `http://127.0.0.1:<port>/mcp`
    pub url: &'a str,
    pub token: &'a Token,
    pub pane: &'a str,
}

/// Galat untuk UI (UI-UX §5): `message` siap tampil, `install` perintah pasang yang bisa disalin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LaunchError {
    pub message: String,
    pub install: Option<String>,
}

impl LaunchError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            install: None,
        }
    }
}

/// Environment anak dari nol: [`INHERIT`] lewat `get` (mis. `std::env::var_os`), `PATH` dari shell login, lalu
/// `TERM`/`COLORTERM`. Kunci API dan variabel `CLAUDE_CODE_*` milik agentdeck tidak pernah ikut.
pub fn env(get: impl Fn(&str) -> Option<OsString>, path: &str) -> Vec<(String, OsString)> {
    let mut env: Vec<(String, OsString)> = INHERIT
        .iter()
        .filter_map(|&k| get(k).filter(|v| !v.is_empty()).map(|v| (k.into(), v)))
        .collect();
    if !env.iter().any(|(k, _)| k == "LANG") {
        env.push(("LANG".into(), FALLBACK_LANG.into()));
    }
    env.push(("PATH".into(), path.into()));
    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("COLORTERM".into(), "truecolor".into()));
    env
}

/// `$SHELL -l` (ARCHITECTURE §3); shell bukan jalur absolut → `/bin/zsh`.
pub fn shell(env: Vec<(String, OsString)>) -> Launch {
    let shell = env
        .iter()
        .find(|(k, _)| k == "SHELL")
        .and_then(|(_, v)| v.to_str())
        .filter(|s| s.starts_with('/'))
        .unwrap_or("/bin/zsh")
        .to_owned();
    Launch {
        argv: vec![shell, "-l".into()],
        env,
        osc_status: false,
    }
}

/// Agen dari katalog: path manual bila diisi, selain itu biner dicari di `PATH` env itu sendiri; tugas setelah `--`
/// sebagai satu argumen. Dengan `mcp`, argv templat katalog (`{mcp_url}`) dan env `AGENTDECK_*` ditambahkan; tanpa itu
/// (server MCP mati) atau tanpa templat (agen pengguna) agen jalan polos dan tidak menerima token.
pub fn agent(
    cat: &Catalogue,
    id: &str,
    task: Option<&str>,
    mut env: Vec<(String, OsString)>,
    mcp: Option<Mcp>,
) -> Result<Launch, LaunchError> {
    let agent = cat
        .get(id)
        .ok_or_else(|| LaunchError::new(format!("Agen `{id}` tidak ada di katalog.")))?;
    let path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .and_then(|(_, v)| v.to_str())
        .unwrap_or_default();
    let program = catalogue::resolve(agent, path).map_err(|message| LaunchError {
        message,
        // Path manual yang salah diperbaiki di Settings, bukan dengan memasang ulang.
        install: agent.path_override.is_none().then(|| agent.install.clone()),
    })?;
    let program = program
        .to_str()
        .ok_or_else(|| LaunchError::new("Path agen bukan UTF-8."))?;
    let mcp = mcp.filter(|_| !agent.mcp_args.is_empty());
    let mut argv = vec![program.to_owned()];
    argv.extend(agent.build_args(mcp.as_ref().map(|m| m.url), task));
    if let Some(m) = mcp {
        env.push(("AGENTDECK_MCP_URL".into(), m.url.into()));
        env.push(("AGENTDECK_TOKEN".into(), m.token.expose().into()));
        env.push(("AGENTDECK_PANE_ID".into(), m.pane.into()));
    }
    Ok(Launch {
        argv,
        env,
        osc_status: agent.status != StatusSource::None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Environment agentdeck seperti saat `pnpm tauri dev` dijalankan dari sesi Claude Code (D-014).
    fn polluted(key: &str) -> Option<OsString> {
        let v = match key {
            "HOME" => "/Users/u",
            "USER" | "LOGNAME" => "u",
            "SHELL" => "/bin/zsh",
            "LANG" => "id_ID.UTF-8",
            "TMPDIR" => "/var/folders/x/T/",
            "SSH_AUTH_SOCK" => "/private/tmp/ssh/agent",
            "PATH" => "/agentdeck/own/path",
            "CLAUDE_CODE_CHILD_SESSION" | "CLAUDE_CODE_DISABLE_TERMINAL_TITLE" => "1",
            "ANTHROPIC_API_KEY" | "OPENAI_API_KEY" => "sk-secret",
            "ANTHROPIC_BASE_URL" => "http://proxy",
            _ => return None,
        };
        Some(v.into())
    }

    fn keys(env: &[(String, OsString)]) -> Vec<&str> {
        env.iter().map(|(k, _)| k.as_str()).collect()
    }

    fn value<'a>(env: &'a [(String, OsString)], key: &str) -> Option<&'a str> {
        env.iter()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.to_str())
    }

    #[test]
    fn env_is_built_from_scratch() {
        let env = env(polluted, "/login/bin:/usr/bin");
        assert_eq!(
            keys(&env),
            [
                "HOME",
                "USER",
                "LOGNAME",
                "SHELL",
                "LANG",
                "TMPDIR",
                "SSH_AUTH_SOCK",
                "PATH",
                "TERM",
                "COLORTERM"
            ]
        );
        // PATH from the login shell, never agentdeck's own.
        assert_eq!(value(&env, "PATH"), Some("/login/bin:/usr/bin"));
        assert_eq!(value(&env, "TERM"), Some("xterm-256color"));
        for (_, v) in &env {
            assert_ne!(v, "sk-secret");
        }
    }

    #[test]
    fn missing_lang_falls_back_to_utf8() {
        let env = env(|k| (k == "HOME").then(|| "/h".into()), "/bin");
        assert_eq!(value(&env, "LANG"), Some(FALLBACK_LANG));
        assert_eq!(keys(&env), ["HOME", "LANG", "PATH", "TERM", "COLORTERM"]);
    }

    fn bin_dir(names: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("agentdeck-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in names {
            let p = dir.join(n);
            std::fs::write(&p, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        dir
    }

    #[test]
    fn agent_argv_runs_binary_directly_with_task_after_double_dash() {
        let dir = bin_dir(&["claude", "codex"]);
        let path = format!("/nonexistent:{}", dir.display());
        let cat = Catalogue::embedded().unwrap();
        for (id, bin) in [("claude-code", "claude"), ("codex", "codex")] {
            let task = "--dangerously-skip-permissions; rm -rf ~ $(x)";
            let l = agent(&cat, id, Some(task), env(polluted, &path), None).unwrap();
            // No shell, no MCP without a server, the task one untouched element after `--`.
            assert_eq!(
                l.argv,
                [dir.join(bin).to_str().unwrap(), "--", task],
                "{id}"
            );
            assert!(l.osc_status);
            let bare = agent(&cat, id, None, env(polluted, &path), None).unwrap();
            assert_eq!(bare.argv.len(), 1);
            let empty = agent(&cat, id, Some(""), env(polluted, &path), None).unwrap();
            assert_eq!(empty.argv, bare.argv);
        }
    }

    /// ARCHITECTURE §4 exactly: Claude `--mcp-config` with `${VAR}` and `--allowedTools`, Codex four `-c` keys
    /// including `tool_timeout_sec=330`, then `--` and the task. The token is in env only.
    #[test]
    fn mcp_injection_puts_token_in_env_only() {
        let dir = bin_dir(&["claude", "codex"]);
        let path = dir.display().to_string();
        let cat = Catalogue::embedded().unwrap();
        let token = Token::generate().unwrap();
        let url = "http://127.0.0.1:4321/mcp";
        let mcp = || {
            Some(Mcp {
                url,
                token: &token,
                pane: "p7",
            })
        };
        let big = format!("-{}", "x".repeat(16 * 1024 - 1));
        let claude = agent(&cat, "claude-code", Some(&big), env(polluted, &path), mcp()).unwrap();
        assert_eq!(
            claude.argv[1..],
            [
                "--mcp-config",
                r#"{"mcpServers":{"agentdeck":{"type":"http","url":"${AGENTDECK_MCP_URL}","headers":{"Authorization":"Bearer ${AGENTDECK_TOKEN}"},"timeout":330000}}}"#,
                "--allowedTools",
                "mcp__agentdeck",
                "--",
                big.as_str(),
            ]
        );
        let codex = agent(&cat, "codex", Some("-a never"), env(polluted, &path), mcp()).unwrap();
        assert_eq!(
            codex.argv[1..],
            [
                "-c",
                r#"mcp_servers.agentdeck.url="http://127.0.0.1:4321/mcp""#,
                "-c",
                r#"mcp_servers.agentdeck.bearer_token_env_var="AGENTDECK_TOKEN""#,
                "-c",
                r#"mcp_servers.agentdeck.default_tools_approval_mode="approve""#,
                "-c",
                "mcp_servers.agentdeck.tool_timeout_sec=330",
                "--",
                "-a never",
            ]
        );
        for l in [&claude, &codex] {
            assert_eq!(value(&l.env, "AGENTDECK_MCP_URL"), Some(url));
            assert_eq!(value(&l.env, "AGENTDECK_TOKEN"), Some(token.expose()));
            assert_eq!(value(&l.env, "AGENTDECK_PANE_ID"), Some("p7"));
            assert!(!l.argv.iter().any(|a| a.contains(token.expose())));
            assert!(!format!("{l:?}").contains(token.expose()));
            // Still from scratch: nothing of agentdeck's own besides the allow-list and AGENTDECK_*.
            assert!(!l.env.iter().any(|(_, v)| v == "sk-secret"));
            assert_eq!(l.env.len(), 13);
        }
    }

    #[test]
    fn missing_binary_reports_install_command() {
        let cat = Catalogue::embedded().unwrap();
        let err = agent(&cat, "codex", None, env(polluted, "/nonexistent"), None).unwrap_err();
        assert_eq!(err.message, "codex tidak ditemukan di PATH.");
        assert_eq!(err.install.as_deref(), Some("npm install -g @openai/codex"));
        assert!(agent(&cat, "nope", None, vec![], None).is_err());
    }

    #[test]
    fn manual_path_wins_over_path_and_must_stay_valid() {
        let dir = bin_dir(&["my-claude", "claude"]);
        let mut cat = Catalogue::embedded().unwrap();
        let manual = dir.join("my-claude");
        cat.agents[0].path_override = Some(manual.clone());
        let path = dir.display().to_string();
        let l = agent(&cat, "claude-code", Some("hi"), env(polluted, &path), None).unwrap();
        assert_eq!(l.argv, [manual.to_str().unwrap(), "--", "hi"]);

        // A stale manual path is an error to fix in Settings, never a silent fall back to `claude` on PATH.
        cat.agents[0].path_override = Some(dir.join("gone"));
        let err = agent(&cat, "claude-code", None, env(polluted, &path), None).unwrap_err();
        assert!(err.message.contains("Path manual"), "{}", err.message);
        assert_eq!(err.install, None);
    }

    /// A user agent with no MCP template runs plain: no MCP argv, no `AGENTDECK_*` env, so no token leaves agentdeck;
    /// `status = "none"` turns OSC scanning off.
    #[test]
    fn user_agent_without_mcp_template_gets_no_token() {
        let dir = bin_dir(&["gemini"]);
        let mut cat = Catalogue::embedded().unwrap();
        let mut gemini = cat.agents[0].clone();
        gemini.id = "gemini".into();
        gemini.binary = "gemini".into();
        gemini.mcp_args.clear();
        gemini.status = StatusSource::None;
        cat.agents.push(gemini);
        let token = Token::generate().unwrap();
        let mcp = Mcp {
            url: "http://127.0.0.1:1/mcp",
            token: &token,
            pane: "p1",
        };
        let path = dir.display().to_string();
        let l = agent(&cat, "gemini", Some("x"), env(polluted, &path), Some(mcp)).unwrap();
        assert_eq!(l.argv[1..], ["--", "x"]);
        assert!(!l.env.iter().any(|(k, _)| k.starts_with("AGENTDECK_")));
        assert!(!l.osc_status);
    }

    #[test]
    fn shell_runs_login_shell_from_env() {
        let l = shell(env(polluted, "/bin"));
        assert_eq!(l.argv, ["/bin/zsh", "-l"]);
        assert!(!l.osc_status);
        assert_eq!(shell(vec![]).argv, ["/bin/zsh", "-l"]);
    }
}
