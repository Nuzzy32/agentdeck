# Progres dan titik lanjut

Terakhir diperbarui: 2026-10-21. Baca ini dulu saat melanjutkan, lalu `docs/IMPLEMENTATION-PLAN.md`.

## Sudah selesai (di `main`, sudah di-push)
| Task | Hasil | Catatan |
|---|---|---|
| Docs awal | PRD, ARCHITECTURE, MCP-TOOLS, UI-UX, PLAN, PHASE-0, SETUP, DECISIONS, RESEARCH | — |
| T0.1 | Rust 1.99.1 + clippy + rustfmt lewat rustup | `~/.zprofile` memuat `~/.cargo/env` |
| S1 tempel + Enter | Lulus | D-013 |
| S4 status Claude | Lulus (OSC 7501, query harus dijawab) | D-016 |
| S5 status Codex | Lulus (OSC 7501 tanpa query, `blocked` saat izin) | D-017 |
| S7 layar `vt100` | Lulus | D-013, sumber `spikes/s7-screen/` |
| S8 server MCP | Lulus (token per pane, tolak Origin/Host) | D-017, sumber `spikes/s8-mcp/` |
| S2/S3 injeksi MCP | Lulus, config global tidak berubah | D-017 |
| D-014 | Env agen disusun dari nol | — |
| D-015 | Prototipe Claude Design jadi acuan UI (sidebar, tanpa sorotan pesan di terminal) | — |
| T1.1 | Kerangka Tauri 2 + React + TS, semua cek lulus, jendela terbuka | commit `db8e27d` |

## Sedang dikerjakan: S6 throughput (branch `spike/s6`, lokal, belum di-push)
Tujuan: 50 MB output PTY lewat `tauri::ipc::Channel` ke xterm.js tanpa UI beku; ketikan < 50 ms
(`docs/PHASE-0.md` S6). Branch ini dibuang setelah hasilnya dicatat; jangan di-merge.

Isi spike:
- `src-tauri/src/lib.rs`: perintah `pty_spawn` (portable-pty, thread baca 64 KB → Channel `Raw`), `pty_write`,
  `pty_ack`, `report` (mencetak `S6 ...` ke stdout).
- `src/App.tsx`: dua xterm.js (+ WebGL). Pane A menjalankan banjir (`VITE_S6_FLOOD`, bawaan
  `yes | head -c 50000000`); pane B menjalankan `/bin/cat` dan setiap 100 ms satu karakter dikirim lalu diukur waktu
  kembalinya. Hasil: MB/s, lag event loop, RTT ketikan, dicetak ke log saat selesai.
- Berkas uji kedua: `spikes/out/big50.log` (50 MB teks berwarna, tidak di-commit). Jalankan dengan
  `VITE_S6_FLOOD='cat spikes/out/big50.log' pnpm tauri dev`.

**Temuan sejauh ini (belum dicatat di DECISIONS):**
1. Tanpa flow control xterm.js membuang data: 71 077 kali "write data discarded, use flow control to avoid losing
   data". Jadi **flow control wajib** di produk (T1.3/T1.4).
2. Desain yang sedang diuji: Rust berhenti membaca PTY bila > 1 MB belum dikonfirmasi (`WINDOW`), JS memanggil
   `pty_ack` di callback `term.write`. Kernel PTY lalu menahan program penghasil output (backpressure alami).

**Belum beres, lanjutkan dari sini:**
1. React StrictMode menjalankan `useEffect` dua kali di dev → dua PTY dengan id sama, ack salah sasaran, pembacaan
   macet di ~1 MB. Perbaikan yang sudah disiapkan tapi **belum diterapkan**: flag modul `let started = false;` dan
   `if (... || started) return; started = true;` di awal efek `App.tsx`.
2. Clippy gagal `type_complexity` di `struct Ptys(...)`. Perbaikan: `type PtyEntry = (Box<dyn Write + Send>, Arc<Credit>);`
   lalu `struct Ptys(Mutex<HashMap<u32, PtyEntry>>);`.
3. Jalankan ulang dua banjir (`yes` dan `cat big50.log`), catat MB/s, lag event loop p95/max, RTT ketikan p95/max,
   dan jumlah "discarded" (harus 0).
4. Catat sebagai D-018 di `docs/DECISIONS.md` (hasil + keputusan flow control dengan ack/credit), tandai S6 di
   ARCHITECTURE §10, tambahkan flow control ke T1.3/T1.4 di IMPLEMENTATION-PLAN, perbarui baris Status.
5. Kembali ke `main` dan hapus branch: `git switch main && git branch -D spike/s6`.

Cara menjalankan spike: `pnpm tauri dev > /tmp/agentdeck-s6.log 2>&1` lalu `grep -a '^S6' /tmp/agentdeck-s6.log`.
Hentikan dengan `pkill -f 'tauri.js dev'; pkill -f 'target/debug/agentdeck$'; pkill -f vite/bin/vite.js`.

## Sesudah S6
- T0.7: tinjau ulang ARCHITECTURE/MCP-TOOLS, pastikan tidak ada asumsi tanpa bukti.
- T1.2: token desain (`src/styles/tokens.css`) + layout statis dari prototipe (D-015, UI-UX §2, §4.7, §6).

## Catatan kerja
- Jalankan spike agen dengan env bersih (`CLEAN` di PHASE-0 S1); sesi Claude Code membocorkan `CLAUDE_CODE_*`.
- Server OpenAI sering "Selected model is at capacity"; untuk spike pakai `-m gpt-5.6-luna` atau `-m gpt-6-luna`.
- Commit tanpa atribusi AI (aturan global pemilik).
