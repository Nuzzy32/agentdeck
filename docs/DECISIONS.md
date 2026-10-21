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

## D-013 — Phase 0 S1: tempel + Enter (2026-10-21)
Versi: Claude Code 2.1.303, Codex 0.163.0. Perintah: `docs/PHASE-0.md` S1 (environment bersih, jeda Enter 150 ms).
Hasil: **lulus.** Tempelan 3 baris (88 B) dan 8 004 B lewat bracketed paste + `\r` masuk sebagai satu prompt di kedua
agen; layar akhir (renderer S7) menunjukkan satu prompt dan satu jawaban. Claude menjawab benar untuk keduanya. Codex
menjawab benar untuk 3 baris; dua percobaan 8 KB tampil utuh sebagai satu prompt, tetapi dijawab server "Selected model
is at capacity" (gangguan server, bukan tempelan). Kedua TUI menyalakan bracketed paste sejak awal dan tetap menyala.
Bukti: `spikes/out/s1-*.bin` (tidak di-commit). Dampak ke desain: jeda 150 ms dipertahankan; tanda [Phase 0] di
ARCHITECTURE §5.3 dihapus.
Temuan sampingan, dicatat untuk spike terkait:
- **S5:** Codex 0.163.0 memancarkan `OSC 7501;state=idle:app=codex` dan `state=working:app=codex` tanpa konfigurasi
  apa pun (tanpa `tui.notification_method`). Codex juga bertanya warna terminal (`OSC 10;?`, `OSC 11;?`).
- **S4:** dengan environment bersih Claude Code mengirim query `OSC 7501;?` dan tidak memancarkan status karena probe
  tidak menjawab. Format jawaban tidak ada di terminal-config; harus dicari sebelum S4.
- **S7:** `vt100` 0.18.1 menampilkan layar akhir kedua TUI dengan benar; `bracketed_paste()` benar. API 0.18 memakai
  `NonZeroU16` untuk ukuran (cuplikan di PHASE-0 diperbaiki).
- Claude Code menandai isi tempelan sebagai teks dari luar (terminal-config). Risiko untuk pesan antar agen, diuji di
  Phase 2 (ARCHITECTURE §5.3).
- Probe memperbaiki positif palsu: teks `--expect` yang ada di dalam tempelan selalu "ditemukan" karena TUI
  menampilkan ulang tempelan; sekarang ditolak.

## D-014 — Environment agen disusun dari nol (2026-10-21)
`launch.rs` tidak mewariskan environment proses agentdeck. Anak hanya mendapat `HOME`, `USER`, `LOGNAME`, `SHELL`,
`LANG`, `PATH` (dari shell login), variabel `AGENTDECK_*`, `TERM`, `COLORTERM`. Alasan: saat S1 dijalankan dari sesi
Claude Code, anak mewarisi ±35 variabel `CLAUDE_CODE_*`/`ANTHROPIC_BASE_URL`; Claude yang diluncurkan menolak menyimpan
transcript ("inherited CLAUDE_CODE_CHILD_SESSION marker") dan `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` mematikan status
OSC 7501. Hal yang sama terjadi bila `pnpm tauri dev` dijalankan dari terminal Claude Code. Variabel lain milik
pengguna (mis. proxy) ditambah lewat daftar izin di Settings bila nanti dibutuhkan, bukan diwariskan diam-diam.

## D-015 — Acuan visual: prototipe Claude Design (2026-10-21)
Prototipe "agentdeck Prototype" di Claude Design
(https://claude.ai/design/p/54f25ab8-5160-4612-9f8f-13420aa64ade) menjadi acuan visual T1.2 dan seterusnya. Tokennya
sama dengan UI-UX §6. Yang diambil dan dicatat di UI-UX:
- **Sidebar kiri** (daftar workspace + pohon pane induk–anak, 200 px, `⌘B` untuk menciutkan) menggantikan tab bar.
  Alasan: relasi induk–anak terlihat tanpa membaca header. "Tab" di dokumen lain diganti "workspace".
- **Settings sebagai modal** di jendela utama, bukan jendela terpisah. Alasan: lebih sederhana di Tauri.
Yang **tidak** diambil:
- Sorotan blok "pesan dari p2" di dalam isi terminal. Claude Code dan Codex memakai alternate screen (S1), jadi
  dekorasi baris xterm.js bergeser saat TUI menggambar ulang. Penanda pesan: baris `[agentdeck] message from …`,
  chip header, kilatan tepi, panel Pesan.
- Gemini CLI di peluncur MVP (katalog pengguna adalah P1, AGT-06).
Perbaikan wajib saat implementasi: pemendekan header (nama agen tidak pernah dipotong) dan `aria-label` untuk semua
tombol ikon, termasuk sidebar dan status bar (UI-UX §3, §8).
