# Riset referensi: T3 Code, MXterm Pro, dan BridgeMind

Dilakukan 2026-10-21 untuk menentukan model kerja agentdeck. Bagian **Observasi** didukung bukti (path, string, log);
**Inferensi** adalah kesimpulan kami; **Belum diketahui** tidak boleh dianggap fakta.

## 1. T3 Code (`~/dev/t3code`, pingdotgg/t3code, lisensi MIT, snapshot 2026-10-20)

Analisis kode sumber biasa.

### Observasi
- Monorepo TypeScript (pnpm). `apps/server` (Node 24 + Effect), `apps/web` (React, Vite, TanStack Router, Zustand,
  Tailwind 4), `apps/desktop` (Electron 44), `apps/mobile` (Expo 58 / React Native 0.88). Tidak ada Dart.
- Codex dijalankan sebagai `codex app-server` (JSON-RPC lewat stdio, `apps/server/src/provider/codexLaunchArgs.ts:12`).
  Claude Code lewat `@anthropic-ai/claude-agent-sdk` (`query()`), yang menjalankan CLI `claude` lokal.
- Lapisan adapter bersama `ProviderAdapterV2` (`packages/provider-core/src/server/ProviderAdapter.ts:646`) menyeragamkan
  event (pesan, tool call, approval, perubahan file). Adapter Codex ±7,5 ribu baris, Claude ±10,4 ribu baris.
- Kolaborasi: (1) ganti provider dalam satu thread dengan *context handoff* (pilihan pesan ±16k token);
  (2) server MCP lokal `http://127.0.0.1:<port>/mcp` dengan tool `delegate_task`
  (`apps/server/src/mcp/toolkits/orchestrator/tools.ts:66`) untuk delegasi lintas provider dan putaran review.
- Semua eksekusi di mesin yang menjalankan server; state di SQLite dengan event sourcing; UI lewat WebSocket.

### Inferensi
- Kuat, tetapi mahal dibangun: setiap provider butuh adapter protokol sendiri yang mengikuti perubahan SDK/CLI.
- Ide yang layak diambil: server MCP lokal sebagai jembatan antar agen, mode izin per agen, review lintas provider.

## 2. MXterm Pro (produk tertutup, v1.1.86)

Diamati sebagai pengguna: fitur yang terlihat di aplikasi dan dokumentasinya, serta perubahan yang ia buat pada berkas
konfigurasi agen di mesin sendiri. Hanya konsep yang dicatat; tidak ada teks, aset, atau kode yang disalin (§4).

### Observasi
- **Agen = CLI interaktif di pane terminal.** Katalog berisi banyak agen (Claude Code, Codex, Gemini, OpenCode, ...).
  Peluncur workspace memilih folder dan jumlah terminal per agen, dengan sakelar bypass izin dan worktree per terminal.
- **Jembatan MCP:** server MCP lokal di `127.0.0.1`. Entrinya ditulis ke config global agen (`~/.claude.json`,
  `~/.codex/config.toml`, `statusLine` di `~/.claude/settings.json`), dengan token per pane lewat environment variable.
- **Cara melempar prompt:** teks dari agen lain ditempel ke pane tujuan saat outputnya sepi dan orang tidak mengetik,
  diawali baris penanda bahwa pesan berasal dari agen.
- **Pagar:** persetujuan orang untuk setiap spawn, batas jumlah anak dan pane, anak tanpa flag bypass, spawn ditolak
  saat kuota hampir habis.
- **Pola kerja:** pembuat dan peninjau adalah agen berbeda, anak bekerja di worktree git sendiri, hasil dilaporkan dalam
  satu baris ringkas yang diverifikasi induk.
- Fitur lain yang terlihat: melanjutkan sesi agen, preset tim, akses dari perangkat lain, notifikasi, tampilan kuota.
- Deteksi status agen dari layar; pemberitahuan pihak ketiga MXterm mengkredit [herdr](https://github.com/herdrdev/herdr)
  (Apache-2.0) untuk bagian ini.

### Inferensi
- Intinya hanya tiga bagian: **PTY + server MCP lokal + tempel teks ke PTY saat sepi**. Tidak perlu adapter protokol
  per provider; agen apa pun yang berjalan di terminal dan mendukung MCP bisa ikut.
- Menulis config global punya efek samping: di luar MXterm, Claude Code tetap memuat entri `mxterm` dengan variabel
  kosong dan melaporkan `INVALID_CONFIG` (terlihat di sesi Claude Code di luar MXterm, 2026-10-21). agentdeck
  menyuntikkan semuanya per peluncuran (D-005, D-029).
- Kuota dibaca dari statusline resmi Claude Code, bukan dari kredensial (D-006, D-029).

## 3. BridgeMind (bridgemind.ai, produk tertutup, sumber publik saja, 2026-10-23)

Tidak ada binary yang dianalisis. Situs menolak fetch biasa (403); dibaca lewat snapshot Wayback Feb–Jul 2026 dan
docs/changelog saat ini. Klaim pemasaran ditandai.

### Observasi (docs resmi)
- Sejak ±Sep 2026 produk digabung menjadi **BridgeMind One** (mode Agent, Code, Chat). BridgeSpace lama: 1–16 pane
  terminal dari templat grid, blok perintah ala Warp lewat OSC 133, ⌘F, editor CodeMirror 6 + file tree, Kanban,
  pustaka Prompts, tema. Stack di docs Feb 2026: Tauri 2 + Rust (`portable-pty`), React, Zustand, xterm.js.
- **BridgeMCP** adalah server MCP **hosted** (`mcp.bridgecode.dev/mcp`, kunci API `bm_live_…`) berisi CRUD
  proyek/task/agen (`create_task`, `update_task(status)`, `create_agent(systemPrompt)`, …). Tidak ada tool untuk
  mengirim prompt ke agen lain: koordinasi lewat papan task di cloud.
- **Kanban → agen:** memilih task membuka terminal, menunggu prompt shell, lalu *mengetik* perintah ke PTY. Changelog
  v0.3.3–0.3.4: kartu di-drag ke pane menjadi prompt berikutnya dan pindah ke In Progress; batal → kembali To Do;
  "Dispatch" ke agen tersimpan yang bekerja di branch sendiri sementara sesi kedua meninjau.
- **Pemulihan sesi (v0.3.4):** pane Claude Code/Codex membuka kembali percakapan yang sama (akun dan folder sama), satu
  prompt "Resume agents?" saat start. Beta: proses *terminal host* terpisah menjaga PTY tetap hidup setelah app keluar.
- Preset sesi Solo/Pair/Workbench/Swarm, notifikasi selesai/butuh kamu, beberapa akun per workspace, Routines terjadwal,
  remote dari ponsel lewat QR. Langganan $20–100/bulan berbasis kredit; dikte suara lewat cloud (lokal diklaim di
  halaman produk, bertentangan dengan docs).

### Inferensi
- Pendekatan "tempel ke PTY saat siap" sama dengan MXterm dan agentdeck; bedanya BridgeMind memusatkan koordinasi di
  papan task, bukan pesan antar agen.
- Server MCP hosted dengan kunci API bertentangan dengan prinsip lokal agentdeck (aturan 5); tidak ditiru.

## 4. Batas pemakaian referensi
- **BridgeMind:** produk tertutup. Sama seperti MXterm: hanya konsep.
- **herdr** (github.com/herdrdev/herdr): Apache-2.0. Aturan deteksi status boleh dipakai dengan menyertakan
  pemberitahuan hak cipta, lisensi, dan catatan perubahan; ambil langsung dari repo herdr.
- **MXterm:** produk tertutup. Hanya konsep yang diambil. Jangan menyalin teks (preset, skill, deskripsi tool), aset,
  atau binary-nya. Nama tool kita sengaja berbeda (`pane_send`, bukan `pane.send`).
- **T3 Code:** MIT. Potongan kode boleh dipakai dengan menyertakan pemberitahuan hak cipta dan lisensinya.
