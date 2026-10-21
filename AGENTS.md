# agentdeck — instruksi untuk coding agent

Terminal desktop multi-pane yang membuat agen CLI (Claude Code, Codex, ...) bisa saling melempar prompt lewat server MCP
lokal. Sumber kebenaran: `docs/PRD.md`. Cara kerja: `docs/ARCHITECTURE.md`. Kontrak tool: `docs/MCP-TOOLS.md`.
Keputusan: `docs/DECISIONS.md`. Latar belakang: `docs/RESEARCH.md`. UI: `docs/UI-UX.md`. Urutan kerja:
`docs/IMPLEMENTATION-PLAN.md`. Setup: `docs/SETUP.md`. Spike: `docs/PHASE-0.md`.

## Stack
Tauri 2 + Rust (core: PTY, server MCP, antrean pesan), UI React + TypeScript strict + Vite + xterm.js, pnpm, macOS dulu.
Warna dan font hanya lewat token di `src/styles/tokens.css` (UI-UX §6).

## Aturan
1. Baca PRD dan DECISIONS sebelum mengubah arsitektur. Bila PRD bertentangan dengan fakta teknis atau ketentuan
   provider, hentikan bagian itu, catat di DECISIONS, minta keputusan.
2. Jangan membaca, menyimpan, mencatat, atau meneruskan kredensial agen (keychain, `~/.claude/.credentials*`, `auth.json`
   Codex, cookie). Jangan memodifikasi binary agen. Jangan memakai endpoint privat provider.
3. Jangan menulis ke config global agen (`~/.claude.json`, `~/.claude/settings.json`, `~/.codex/config.toml`). Semua
   injeksi lewat argumen/env per peluncuran (D-005).
4. Agen yang dibuka oleh agen tidak pernah mendapat flag bypass izin. Tool MCP tidak menerima argumen perintah, flag,
   atau env.
5. Server MCP hanya bind ke `127.0.0.1`. Setiap permintaan wajib token pane; cek `Host`/`Origin`.
6. Token tidak boleh muncul di log, berkas, pesan galat, atau UI.
7. Logika yang menentukan keamanan (token, batas, pengaman putaran, validasi argumen) ada di Rust dan punya tes unit.
8. Jangan menyalin teks, aset, atau binary MXterm Pro. Kode T3 Code (MIT) boleh dipakai dengan pemberitahuan lisensinya.
9. Rust: tanpa `unwrap()`/`expect()` di jalur produksi kecuali invarian yang dijelaskan; galat dipetakan ke kode di
   `docs/MCP-TOOLS.md`. TypeScript: strict, hindari `any`.
10. Jangan tambah dependency besar tanpa alasan; catat yang penting di DECISIONS.
11. Satu alur ujung ke ujung sebelum lanjut; ikuti roadmap PRD §9.

## Workflow tiap tugas
Periksa repo → rencana singkat + berkas yang diubah → perubahan sekecil mungkin → `cargo fmt --check`, `cargo clippy`,
`cargo test`, `pnpm typecheck`, `pnpm test` → lapor (tujuan, perubahan, berkas, tes + hasil,
keterbatasan, keputusan tertunda). Perilaku terminal dan agen wajib dicoba di aplikasi yang berjalan
(`pnpm tauri dev`), bukan hanya lewat tes. Jangan klaim tes lulus bila belum dijalankan.
