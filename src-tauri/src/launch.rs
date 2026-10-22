//! Argv dan environment untuk satu pane (ARCHITECTURE §3–§4). Program dijalankan langsung tanpa shell (D-009) dengan
//! environment yang disusun dari nol (D-014). Injeksi MCP menyusul di T2.3; di sini `mcp_url` selalu `None`.

use std::ffi::OsString;

use serde::Serialize;

use crate::catalogue::{self, Catalogue};

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
    /// Agen melapor status lewat OSC 7501 (semua agen bawaan); shell tidak dipindai.
    pub osc_status: bool,
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

/// Agen dari katalog: biner dicari di `PATH` env itu sendiri, tugas setelah `--` sebagai satu argumen.
pub fn agent(
    cat: &Catalogue,
    id: &str,
    task: Option<&str>,
    env: Vec<(String, OsString)>,
) -> Result<Launch, LaunchError> {
    let agent = cat
        .get(id)
        .ok_or_else(|| LaunchError::new(format!("Agen `{id}` tidak ada di katalog.")))?;
    let path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .and_then(|(_, v)| v.to_str())
        .unwrap_or_default();
    let found =
        catalogue::find_in_path(&agent.binary, path).and_then(|p| p.to_str().map(str::to_owned));
    let Some(program) = found else {
        return Err(LaunchError {
            message: format!("{} tidak ditemukan di PATH.", agent.binary),
            install: Some(agent.install.clone()),
        });
    };
    let mut argv = vec![program];
    argv.extend(agent.build_args(None, task));
    Ok(Launch {
        argv,
        env,
        osc_status: true,
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
        let cat = Catalogue::builtin().unwrap();
        for (id, bin) in [("claude-code", "claude"), ("codex", "codex")] {
            let task = "--dangerously-skip-permissions; rm -rf ~ $(x)";
            let l = agent(&cat, id, Some(task), env(polluted, &path)).unwrap();
            // No shell, no MCP injection yet (T2.3), the task one untouched element after `--`.
            assert_eq!(
                l.argv,
                [dir.join(bin).to_str().unwrap(), "--", task],
                "{id}"
            );
            assert!(l.osc_status);
            let bare = agent(&cat, id, None, env(polluted, &path)).unwrap();
            assert_eq!(bare.argv.len(), 1);
            let empty = agent(&cat, id, Some(""), env(polluted, &path)).unwrap();
            assert_eq!(empty.argv, bare.argv);
        }
    }

    #[test]
    fn missing_binary_reports_install_command() {
        let cat = Catalogue::builtin().unwrap();
        let err = agent(&cat, "codex", None, env(polluted, "/nonexistent")).unwrap_err();
        assert_eq!(err.message, "codex tidak ditemukan di PATH.");
        assert_eq!(err.install.as_deref(), Some("npm install -g @openai/codex"));
        assert!(agent(&cat, "nope", None, vec![]).is_err());
    }

    #[test]
    fn shell_runs_login_shell_from_env() {
        let l = shell(env(polluted, "/bin"));
        assert_eq!(l.argv, ["/bin/zsh", "-l"]);
        assert!(!l.osc_status);
        assert_eq!(shell(vec![]).argv, ["/bin/zsh", "-l"]);
    }
}
