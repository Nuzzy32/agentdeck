# Dukungan platform

Status per 2026-10-24, berdasarkan kode, tes, dan build yang benar-benar dijalankan (D-038). "Belum diverifikasi"
berarti belum pernah dibangun atau dijalankan di platform itu; jangan dibaca sebagai "jalan".

| Platform | Status | Bukti |
|---|---|---|
| macOS 13+ Apple Silicon | **Didukung** | `cargo test` (234), `pnpm test`, `pnpm tauri build` → `.app` + `.dmg`, uji di aplikasi berjalan |
| macOS Intel | Belum diverifikasi | Tidak ada mesin uji; kode tidak memakai API khusus arm64 |
| Windows 10/11 | **Belum didukung** — tidak bisa dikompilasi | Audit kode 2026-10-24; `cargo check --target x86_64-pc-windows-msvc` di Mac terhenti di tauri-build (butuh `llvm-rc`), jadi belum ada bukti kompilasi |
| Linux | Belum diverifikasi | Audit kode; tidak pernah dibangun |

## Fitur per platform

| Fitur | macOS | Windows | Linux |
|---|---|---|---|
| Terminal PTY (shell, agen) | Jalan | Blocker: sinyal Unix di `pty.rs` | Kemungkinan jalan (portable-pty Unix); belum dibangun |
| Menutup pane + semua turunannya | Jalan (killpg + pemindaian turunan) | Blocker: perlu Job Object `KILL_ON_JOB_CLOSE` | Sebagian: pemindaian turunan hanya macOS, proses di sesi sendiri (Bash tool Claude) bisa tertinggal |
| Server MCP lokal, token pane | Jalan | Kode portabel; belum diverifikasi | Kode portabel; belum diverifikasi |
| Status OSC 7501, blok OSC 133 | Jalan (zsh) | Belum diverifikasi: ConPTY dibuat tanpa passthrough | OSC 133 hanya zsh |
| Mencari agen di `PATH` | Jalan | Sebagian: pemisah `;` sudah benar, `PATHEXT` (`.cmd`/`.exe`) belum | Jalan bila `$SHELL` login tersedia |
| Lingkungan anak dari nol | Jalan | Blocker runtime: `SystemRoot`, `USERPROFILE`, `APPDATA`, `PATHEXT`, `ComSpec` tidak ikut | Jalan |
| Pintasan keyboard | Jalan (`⌘`) | Tidak ada: semua pintasan butuh `metaKey` | Tidak ada: sama |
| Menu aplikasi | Jalan | Menu khusus macOS akan muncul sebagai bilah menu | Sama |
| Perintah pasang agen (katalog) | `npm`, `curl … \| bash` | `npm` jalan, installer `curl \| bash` Claude tidak | Jalan |
| Installer agentdeck (`install.sh`) | Jalan (build dari sumber) | Ditolak dengan pesan | Ditolak dengan pesan |

## Blocker Windows, diurutkan

1. `src-tauri/src/pty.rs`: `libc::killpg`/`kill`/`SIGHUP`/`SIGKILL` dan `pid_t` tanpa `cfg`. Perlu jalur Windows:
   `ChildKiller` portable-pty untuk mulai, Job Object untuk menutup seluruh pohon proses.
2. Lingkungan anak (`launch.rs`, `catalogue.rs`): tambahkan variabel wajib Windows ke env yang dibangun dari nol.
3. Biner npm di Windows adalah `.cmd`; menjalankannya lewat `cmd.exe` mengurai ulang tugas dan JSON `--mcp-config`
   (masalah "BatBadBut"). Harus memanggil `.exe` asli atau `node <entry>`, bukan `.cmd`.
4. Shell bawaan: `/bin/zsh -l` → `%ComSpec%` atau PowerShell tanpa `-l`.
5. Pintasan: `Ctrl+Shift` sebagai modifier di luar macOS; `Ctrl+C` tetap SIGINT di terminal.
6. Tes Rust yang memakai `/bin/sh`, `ps`, `perl`, `libc` perlu `#[cfg(unix)]`.
7. CI `windows-latest` untuk bukti kompilasi dan tes, karena Mac tanpa `llvm-rc` tidak bisa memeriksanya.

## Sudah dikerjakan (aman untuk macOS, tanpa perubahan perilaku)

- `find_in_path` memakai `std::env::split_paths` dan `Path::is_absolute` (pemisah `PATH` sesuai platform).
- `$SHELL` diperiksa dengan `Path::is_absolute`, bukan awalan `/`.
- Pemeriksaan bit eksekusi (`check_executable`) dan integrasi zsh (`zsh_integration`) di balik `#[cfg(unix)]`, dengan
  jalur non-Unix yang aman (berkas biasa cukup; shell polos).
