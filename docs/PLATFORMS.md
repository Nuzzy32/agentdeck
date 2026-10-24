# Dukungan platform

Status per 2026-10-24, berdasarkan kode, tes, dan build yang benar-benar dijalankan (D-038, D-039). "Belum diverifikasi"
berarti belum pernah dibangun atau dijalankan di platform itu; jangan dibaca sebagai "jalan".

| Platform | Status | Bukti |
|---|---|---|
| macOS 13+ Apple Silicon | **Didukung** | `cargo test` (234), `pnpm test`, `pnpm tauri build` → `.app` + `.dmg`, uji di aplikasi berjalan |
| macOS Intel | Belum diverifikasi | Tidak ada mesin uji; kode tidak memakai API khusus arm64 |
| Windows 10/11 | **Belum didukung** — terkompilasi, tes sebagian | CI `windows-latest` 2026-10-24: typecheck, tes UI, fmt, clippy lulus; `cargo test` lib 155/165 sebelum D-040, hasil setelahnya lihat run CI terbaru. Belum pernah dijalankan sebagai aplikasi |
| Linux | Belum diverifikasi | Audit kode; tidak pernah dibangun |

## Fitur per platform

| Fitur | macOS | Windows | Linux |
|---|---|---|---|
| Terminal PTY (shell, agen) | Jalan | Sinyal Unix sudah di balik `cfg(unix)`; belum dikompilasi atau dijalankan | Kemungkinan jalan (portable-pty Unix); belum dibangun |
| Menutup pane + semua turunannya | Jalan (killpg + pemindaian turunan) | Sebagian, belum diverifikasi: `ChildKiller` hanya menghentikan anak langsung; cucu tertinggal sampai ada Job Object `KILL_ON_JOB_CLOSE` | Sebagian: pemindaian turunan hanya macOS, proses di sesi sendiri (Bash tool Claude) bisa tertinggal |
| Server MCP lokal, token pane | Jalan | Kode portabel; belum diverifikasi | Kode portabel; belum diverifikasi |
| Status OSC 7501, blok OSC 133 | Jalan (zsh) | Belum diverifikasi: ConPTY dibuat tanpa passthrough | OSC 133 hanya zsh |
| Mencari agen di `PATH` | Jalan | Sebagian: pemisah `;` sudah benar, `PATHEXT` (`.cmd`/`.exe`) belum | Jalan bila `$SHELL` login tersedia |
| Lingkungan anak dari nol | Jalan | Blocker runtime: `SystemRoot`, `USERPROFILE`, `APPDATA`, `PATHEXT`, `ComSpec` tidak ikut | Jalan |
| Pintasan keyboard | Jalan (`⌘`) | Tidak ada: semua pintasan butuh `metaKey` | Tidak ada: sama |
| Menu aplikasi | Jalan | Menu khusus macOS akan muncul sebagai bilah menu | Sama |
| Perintah pasang agen (katalog) | `npm`, `curl … \| bash` | `npm` jalan, installer `curl \| bash` Claude tidak | Jalan |
| Installer agentdeck (`install.sh`) | Jalan (build dari sumber) | Ditolak dengan pesan | Ditolak dengan pesan |

## Blocker Windows, diurutkan

1. ~~`src-tauri/src/pty.rs`: sinyal Unix tanpa `cfg`~~ — di-`cfg` (D-039); jalur Windows memakai `ChildKiller`
   portable-pty. Sisa: Job Object `KILL_ON_JOB_CLOSE` supaya seluruh pohon proses ikut berhenti.
2. Lingkungan anak (`launch.rs`, `catalogue.rs`): tambahkan variabel wajib Windows ke env yang dibangun dari nol.
3. Biner npm di Windows adalah `.cmd`; menjalankannya lewat `cmd.exe` mengurai ulang tugas dan JSON `--mcp-config`
   (masalah "BatBadBut"). Harus memanggil `.exe` asli atau `node <entry>`, bukan `.cmd`.
4. Shell bawaan: `/bin/zsh -l` → `%ComSpec%` atau PowerShell tanpa `-l`.
5. Pintasan: `Ctrl+Shift` sebagai modifier di luar macOS; `Ctrl+C` tetap SIGINT di terminal.
6. ~~Tes Rust yang memakai `/bin/sh`, `ps`, `perl`, `libc`~~ — `#[cfg(unix)]` per modul (D-039). Sisa: modul itu
   juga berisi tes logika murni yang kini tidak jalan di Windows; pisahkan bila perlu cakupan Windows. Tes lain
   (`mcp/tools.rs`, `panes.rs`, `layout.rs`, …) memakai path gaya Unix (`/w/src`), yang di Windows bukan path absolut;
   CI pertama yang akan menunjukkan mana yang gagal.
7. ~~CI `windows-latest`~~ — `.github/workflows/check.yml` (macOS + Windows), belum pernah berjalan: belum di-push.

## Sudah dikerjakan (aman untuk macOS, tanpa perubahan perilaku)

- `find_in_path` memakai `std::env::split_paths` dan `Path::is_absolute` (pemisah `PATH` sesuai platform).
- `$SHELL` diperiksa dengan `Path::is_absolute`, bukan awalan `/`.
- Pemeriksaan bit eksekusi (`check_executable`) dan integrasi zsh (`zsh_integration`) di balik `#[cfg(unix)]`, dengan
  jalur non-Unix yang aman (berkas biasa cukup; shell polos).
- `pty.rs` (D-039): `killpg`/`kill`/sinyal/`pid_t`/`descendants` di balik `#[cfg(unix)]`; jalur non-Unix menyimpan
  `ChildKiller` dan `terminate` memanggil `kill()`. `libc` jadi dependency khusus Unix.
- Tes yang butuh Unix (`pty.rs`, `spawn.rs`, `catalogue.rs`, `launch.rs`, `mcp/claim.rs`, `tests/fake_agent.rs`,
  `tests/real_cli.rs`) di balik `#[cfg(unix)]`.
