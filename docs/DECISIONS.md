# Decision Log

Format: keputusan, alasan, status. Tanggal ISO. Keputusan yang bergantung pada Phase 0 ditandai **sementara**.

## D-001 — Nama kerja dan lokasi (2026-10-21)
Nama kerja **agentdeck**, repo `~/dev/agentdeck`. Proyek terpisah dari NEXORA; NEXORA tetap di-deploy di Vercel tanpa
perubahan dan tidak terintegrasi di MVP. Nama final dan bundle identifier: pertanyaan terbuka PRD §11.

## D-002 — Stack: Tauri 2 + Rust, UI React + TypeScript (2026-10-21)
Dipilih pemilik (dibanding Electron + TypeScript). Alasan: binary kecil dan hemat RAM untuk banyak pane; sama dengan
MXterm Pro yang terbukti bekerja. Konsekuensi: pemilik belajar Rust; logika inti dijaga kecil.
Versi saat diputuskan (crates.io / npm, 2026-10-21): `tauri` 2.12.4, `portable-pty` 0.9.0, `rmcp` 3.6.0 (fitur
`transport-streamable-http-server`) + `axum`, `vt100` 0.18.1, `@xterm/xterm` 6.0.0, `@xterm/addon-fit` 0.11.0,
`@xterm/addon-webgl` 0.19.0. Ditolak: `alacritty_terminal` (lebih berat dari kebutuhan), `wezterm-term` (tidak ada di
crates.io). Rust 1.99.1 terpasang lewat rustup (2026-10-21).

## D-003 — Model kerja: CLI di PTY + MCP lokal + tempel (2026-10-21)
Agen dijalankan sebagai CLI interaktif; agen memanggil agentdeck lewat MCP; agentdeck mengirim prompt dengan menulis ke
PTY. Ditolak: model T3 Code (adapter protokol per provider lewat `codex app-server` dan Agent SDK), karena butuh
ribuan baris per provider dan harus mengikuti perubahan SDK. Bukti dan analisis: `docs/RESEARCH.md`.

## D-004 — Server MCP: streamable HTTP di loopback, identitas dari token per pane (2026-10-21)
Satu server di `127.0.0.1:<port acak>/mcp`. Pane diidentifikasi dari `Authorization: Bearer`, bukan `Mcp-Session-Id`,
karena protokol MCP `2026-07-28` menghapus sesi (SEP-2567). `Host`/`Origin` asing ditolak.

## D-005 — Injeksi per peluncuran, tidak menulis config global (2026-10-21, sementara sampai S2/S3)
Claude Code: `--mcp-config <json>` tanpa `--strict-mcp-config` (agar MCP pengguna tetap termuat). Codex:
`-c mcp_servers.agentdeck={...}`. Alasan: MXterm Pro menulis ke `~/.claude.json` dan `~/.codex/config.toml`; di luar
MXterm, Claude Code memuat entri itu dengan variabel kosong dan melaporkan `INVALID_CONFIG` (terlihat 2026-10-21).

## D-006 — Kredensial dan ketentuan provider (2026-10-21)
agentdeck menjalankan binary resmi yang tidak dimodifikasi, dan pengguna login sendiri. agentdeck tidak menawarkan
login, tidak membaca/menyimpan/meneruskan kredensial atau token sesi, tidak memakai endpoint privat, dan tidak
menampilkan kuota yang diambil dari kredensial.
- Anthropic (code.claude.com/docs/en/legal-and-compliance): pihak ketiga dilarang "to offer Claude.ai login" atau
  "to route requests through Free, Pro, or Max plan credentials on behalf of their users", dan "may not collect, store,
  or intermediate Claude.ai credentials or session tokens"; halaman yang sama tidak melarang "an end user from signing
  in to the unmodified Claude Code binary with their own Claude subscription". "The Claude Code binary must not be
  modified."
- OpenAI (learn.chatgpt.com/docs/auth): "Treat `~/.codex/auth.json` like a password"; "API keys are still the
  recommended default for automation". Halaman Terms of Use tidak bisa dibuka (403) saat diperiksa.
- Info login hanya lewat perintah CLI yang tidak membuka kredensial (`claude auth status`). Bila `ANTHROPIC_API_KEY`
  ada di environment, tampilkan peringatan (kunci itu didahulukan dan ditagih per token); jangan diubah diam-diam.
- **Terbuka:** sebelum rilis publik, periksa ulang ketentuan kedua provider dan tanyakan ke Anthropic apakah aplikasi
  lokal yang menjalankan CLI milik pengguna memerlukan Commercial Terms. Untuk pemakaian pribadi tidak ada larangan
  yang ditemukan.

## D-007 — Status dari sinyal agen sendiri (2026-10-21, sementara sampai S4/S5)
Urutan: escape dari agen (Claude OSC 7501, Codex OSC 9 lewat `tui.notification_method="osc9"`), lalu hook (Claude
lewat `--settings`), lalu heuristik output. Hook Codex tidak dipakai karena butuh kepercayaan lewat `/hooks` atau
`--dangerously-bypass-hook-trust`. Membaca layar tidak dipakai untuk status karena rapuh terhadap perubahan TUI.

## D-008 — Tool agentdeck tidak ditanyakan ulang oleh agen (2026-10-21)
Claude: `--allowedTools mcp__agentdeck`. Codex: `default_tools_approval_mode="approve"` + anotasi tool. Alasan:
gerbang ada di agentdeck (kartu spawn, sakelar peer, pengaman putaran); prompt ganda melelahkan. Izin lain milik agen
tidak diubah: tidak ada `--permission-mode`, `-a`, `-s`, atau flag bypass.

## D-009 — Agen dijalankan tanpa shell (2026-10-21)
`portable-pty` menjalankan binary agen langsung dengan argv; `PATH` diambil sekali dari shell login. Alasan: teks tugas
dari agen lain tidak boleh pernah diurai shell (injeksi perintah).

## D-010 — Penyimpanan JSON/JSONL (2026-10-21)
Settings di JSON, log pesan dan audit di JSONL harian (14 hari). SQLite ditunda sampai ada kebutuhan pencarian riwayat.

## D-011 — macOS dulu (2026-10-21)
Apple Silicon, macOS 13+. Kode tidak boleh mengunci platform tanpa alasan (`portable-pty` dan Tauri lintas platform),
tetapi Windows/Linux tidak diuji di MVP.

## D-012 — Batas pemakaian referensi dan penamaan (2026-10-21)
MXterm Pro produk tertutup: hanya konsep yang diambil; teks, preset, aset, dan binary tidak disalin. Nama tool memakai
snake_case (`pane_send`) dan teks instructions ditulis sendiri. Kode T3 Code (MIT) boleh dipakai dengan pemberitahuan
lisensi di berkas yang bersangkutan.
