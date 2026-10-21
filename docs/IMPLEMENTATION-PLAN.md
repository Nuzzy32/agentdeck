# Rencana implementasi

Pecahan roadmap PRD §9 menjadi task kecil. Satu task = satu perubahan yang bisa direview dan diuji sendiri (satu commit
atau PR). Kolom **Butuh** menyebut task yang harus selesai lebih dulu; task tanpa ketergantungan satu sama lain boleh
dikerjakan paralel (ditandai ∥). Setiap task selesai bila kriteria **Selesai bila** terpenuhi dan workflow di
`AGENTS.md` dijalankan.

Status per 2026-10-21: T0.1, T0.2 (S1) selesai; T0.3 S4 selesai, S5 sebagian (blocked Codex belum terlihat); task lain `todo`. Perbarui baris ini saat task selesai.

## Phase 0 — Spike kelayakan
Panduan lengkap: `docs/PHASE-0.md`. Tidak ada kode produk.

| ID | Task | Butuh | Selesai bila |
|---|---|---|---|
| T0.1 | Pasang Rust (SETUP §1) | — | `cargo --version` jalan |
| T0.2 | S1 tempel + Enter | — | Hasil + jeda Enter tercatat di DECISIONS |
| T0.3 | S4 status Claude, S5 status Codex ∥ | — | Sinyal per keadaan tercatat |
| T0.4 | S7 layar `vt100` | T0.1, T0.2 | Hasil tercatat |
| T0.5 | S8 server `rmcp`, lalu S2 dan S3 | T0.1 | Injeksi kedua agen terbukti atau desain disesuaikan |
| T0.6 | S6 throughput (setelah T1.1) | T1.1 | Angka tercatat |
| T0.7 | Perbarui ARCHITECTURE/MCP-TOOLS: hapus tanda **[Phase 0]** yang terjawab | T0.2–T0.6 | Tidak ada asumsi tanpa bukti |

## Phase 1 — Fondasi terminal
Tujuan akhir: alur A (PRD §7) tanpa MCP.

| ID | Task | Berkas utama | Butuh | Selesai bila |
|---|---|---|---|---|
| T1.1 | Kerangka Tauri + konfigurasi jendela dan CSP (SETUP §2, §5), skrip `typecheck`/`test` | `package.json`, `src-tauri/tauri.conf.json` | T0.1 | `pnpm tauri dev` membuka jendela "agentdeck"; clippy, tes, typecheck lulus |
| T1.2 ∥ | Token desain + kerangka layout statis: title bar, sidebar, status bar, keadaan awal | `src/styles/tokens.css`, `src/App.tsx`, `src/layout/` | T1.1 | Cocok dengan UI-UX §2, §4.7, §6 di tema gelap dan terang |
| T1.3 ∥ | `PaneManager`: buat pane shell, tulis, resize, hentikan grup proses; thread baca → `Channel`; buffer putar ulang 1 MB | `src-tauri/src/pty.rs`, `lib.rs` | T1.1 | Tes integrasi: `echo` muncul di stream; `pane_close` menghentikan anak dan cucu |
| T1.4 | `TerminalPane`: xterm.js + fit + webgl (cadangan bila konteks WebGL hilang), Channel → `write`, `onData` → `pane_write`, resize | `src/pane/TerminalPane.tsx` | T1.3 | Shell interaktif berjalan; `vim` dan `htop` tampil benar; resize mengikuti |
| T1.5 | Pohon split + workspace (sidebar), fokus, pintasan UI-UX §7 (tanpa `⌘J`), dialog tutup | `src/layout/` | T1.4 | Semua pintasan bekerja; split ditolak di bawah 40×8 |
| T1.6 ∥ | `PATH` dari shell login, `catalogue.toml` (claude-code, codex), deteksi + versi, perintah `agents_list` | `catalogue.rs`, `catalogue.toml` | T1.3 | Tes unit parsing katalog; deteksi benar di mesin pemilik |
| T1.7 | Peluncur (UI-UX §4.1), luncurkan agen manual tanpa MCP, header pane dengan status heuristik, pita "selesai" | `src/launcher/`, `launch.rs`, `status.rs` | T1.5, T1.6 | Alur A berjalan; agen tidak terpasang tampil sesuai UI-UX §5 |
| T1.8 | Bersih-bersih saat aplikasi keluar | `pty.rs`, `lib.rs` | T1.3 | Setelah keluar, `pgrep -f "claude\|codex"` tidak menemukan proses dari agentdeck |

## Phase 2 — Jembatan MCP dan pesan
Tujuan akhir: alur C dan D dengan CLI asli.

| ID | Task | Berkas utama | Butuh | Selesai bila |
|---|---|---|---|---|
| T2.1 ∥ | Registri token + `policy` (relasi, aturan kirim/baca ARCHITECTURE §5.4, batas MCP-TOOLS) | `policy.rs`, `tokens.rs` | T1.3 | Tes unit untuk setiap baris tabel §5.4 dan setiap batas |
| T2.2 | `McpServer`: axum + rmcp di `127.0.0.1:0`, middleware token (perbandingan waktu-konstan), tolak `Origin`, `instructions`; indikator status bar | `mcp/server.rs` | T2.1, T0.5 | Tes: tanpa token 401, `Origin` asing ditolak, token sah → pane benar |
| T2.3 | Injeksi saat peluncuran: env + argv Claude/Codex sesuai hasil S2/S3 | `launch.rs` | T2.2, T1.7 | Tes unit argv; manual: `/mcp` di kedua agen menunjukkan `agentdeck`; config global tidak berubah (cara cek PHASE-0 S2/S3) |
| T2.4 ∥ | `Screen` (`vt100`) per pane; tool `pane_list`, `pane_read` + `READ_RATE` | `screen.rs`, `mcp/tools.rs` | T2.2 | Agen bisa membaca layar pane lain sesuai policy |
| T2.5 ∥ | `tests/fake-agent`: binary kecil yang menyalakan bracketed paste, mencetak OSC status saat menerima input, dan bisa memanggil MCP | `tests/fake-agent/` | T2.2 | Dipakai oleh tes T2.6–T2.7 |
| T2.6 | `StatusTracker`: pemindai OSC + pemetaan per agen dari S4/S5; event status ke UI | `status.rs` | T2.5, T0.3 | Tes dengan fake-agent; header menampilkan status benar dengan CLI asli |
| T2.7 | `Delivery`: antrean FIFO per tujuan, syarat siap (status, sepi, waktu ketikan terakhir dari `pane_write`), sanitasi, tempelan + Enter, `QUEUE_FULL`, `dropped` saat keluar; tool `pane_send` | `delivery.rs`, `mcp/tools.rs` | T2.6 | Tes dengan jam palsu untuk syarat siap; tes integrasi fake-agent; tidak pernah menempel saat orang mengetik |
| T2.8 | Pengaman putaran + pita (UI-UX §4.3) + sakelar "Saling kirim" + item status bar | `delivery.rs`, `src/workspace/` | T2.7 | Alur D lulus; Enter oleh orang mengatur ulang hitungan |
| T2.9 | Log pesan JSONL + panel Pesan (UI-UX §4.4) | `store.rs`, `src/messages/` | T2.7 | Setiap pesan tampil dengan status yang benar; log tanpa token |

## Phase 3 — Agen membuka agen
Tujuan akhir: alur B (inti MVP) dengan Claude Code dan Codex asli.

| ID | Task | Berkas utama | Butuh | Selesai bila |
|---|---|---|---|---|
| T3.1 | `approvals` + kartu (UI-UX §4.2), `⌘J`, batas waktu, notifikasi macOS (`tauri-plugin-notification`, dicatat di DECISIONS) | `approvals.rs`, `src/cards/` | T2.2 | Kartu tidak mengambil fokus; habis waktu = `APPROVAL_TIMEOUT` |
| T3.2 | Tool `pane_spawn`: validasi argumen, `cwd` dikanonikalisasi dan harus di dalam folder pemanggil, batas, rate limit, `NESTED`; pane anak dibuka di samping pemanggil | `mcp/tools.rs`, `policy.rs` | T3.1, T2.3 | Tes unit untuk setiap kode galat; spawn nyata membuka pane anak |
| T3.3 ∥ | Jendela Settings (UI-UX §4.6) + `settings.json` | `src/settings/`, `store.rs` | T2.8 | Nilai di luar rentang ditolak; perubahan langsung berlaku |
| T3.4 ∥ | Log audit (spawn diminta/disetujui/ditolak, jeda, tutup) | `audit.rs` | T3.1 | Baris audit untuk setiap kejadian, tanpa token |
| T3.5 | Uji alur B ujung ke ujung dengan CLI asli; catat versi dan hasil di DECISIONS | — | T3.2 | Alur B dan semua AC-nya lulus |

**MVP selesai di akhir Phase 3.**

## Phase 4 — Kerja paralel (P1)
| ID | Task | Persyaratan |
|---|---|---|
| T4.1 | `worktree: true` pada `pane_spawn`, hapus worktree tanpa perubahan saat pane ditutup | SPN-05, MCP-06 |
| T4.2 | Tool `pane_report` + hasil di header anak | MCP-06 |
| T4.3 | Pulihkan tata letak | TERM-06 |
| T4.4 | Katalog agen pengguna + path manual | AGT-06, SET-03 |
| T4.5 | Info login (`claude auth status`) + peringatan `ANTHROPIC_API_KEY` | AGT-05 |
| T4.6 | Pesan tertahan > 10 menit: kirim paksa / buang | MSG-07 |
| T4.7 | Tema terminal dan font | TERM-07 |

## Phase 5 — Distribusi
| ID | Task |
|---|---|
| T5.1 | Ikon dan nama final; bundle identifier |
| T5.2 | Penandatanganan dan notarisasi (SETUP §6) |
| T5.3 | Updater + halaman rilis |
| T5.4 | Periksa ulang ketentuan provider sebelum rilis publik (DECISIONS D-006) |

## Mendelegasikan task ke agen
Setiap task di atas bisa menjadi brief untuk Claude Code atau Codex: sebutkan ID task, berkas utama, kriteria selesai,
dan minta agen mengikuti `AGENTS.md`. Task bertanda ∥ dalam satu phase tidak menyentuh berkas yang sama, sehingga bisa
dikerjakan dua agen di worktree terpisah.
