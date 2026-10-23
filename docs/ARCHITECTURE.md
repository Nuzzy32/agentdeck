# agentdeck — Cara kerja

Dokumen ini menjelaskan bagaimana agentdeck bekerja. Apa yang dibangun dan kenapa ada di `docs/PRD.md`; kontrak tool di
`docs/MCP-TOOLS.md`. Fakta eksternal diverifikasi 2026-10-21 terhadap dokumentasi resmi dan `--help` CLI terpasang
(Claude Code 2.1.303, Codex 0.163.0) dan dibuktikan di Phase 0 (D-013–D-018). Yang belum terbukti dikumpulkan di §12
beserta task yang wajib mengujinya.

## 1. Gambaran besar

```
┌─────────────────────────── agentdeck.app (satu proses Tauri) ───────────────────────────┐
│                                                                                          │
│  WebView (React + xterm.js)                    Core (Rust, tokio)                         │
│  ┌──────────────────────────┐   invoke    ┌──────────────────────────────────────────┐   │
│  │ Sidebar / split layout   │ ──────────▶ │ PaneManager   portable-pty, 1 thread baca│   │
│  │ TerminalPane × N         │ ◀────────── │ Screen        vt100 per pane             │   │
│  │ Kartu persetujuan        │   Channel   │ StatusTracker OSC 7501 / heuristik       │   │
│  │ Log pesan, Settings      │   (bytes)   │ Delivery      antrean + pengaman putaran │   │
│  └──────────────────────────┘             │ Policy        relasi, batas, rate limit  │   │
│                                           │ McpServer     rmcp + axum, 127.0.0.1:port│   │
│                                           │ Store         JSON / JSONL di app data   │   │
│                                           └──────────────┬───────────────────────────┘   │
└──────────────────────────────────────────────────────────┼───────────────────────────────┘
                     PTY (stdin/stdout)                    │ HTTP POST /mcp  (Bearer token pane)
        ┌──────────────────────┬───────────────────────────┤
        ▼                      ▼                           │
   ┌─────────┐            ┌─────────┐                      │
   │ claude  │ ──MCP────▶ │  codex  │ ──MCP────────────────┘
   │ pane p1 │            │ pane p2 │
   └─────────┘            └─────────┘
```

Agen adalah proses CLI biasa di PTY. Agen berbicara ke agentdeck lewat MCP; agentdeck berbicara ke agen dengan
**menulis ke PTY-nya**, persis seperti orang yang mengetik. Tidak ada protokol khusus per provider.

## 2. Komponen

### Core (Rust, `src-tauri/src/`)
| Modul | Tugas |
|---|---|
| `pty.rs` | Membuka PTY (`portable-pty`), menjalankan proses, menulis input, resize, menghentikan grup proses. Satu thread baca per pane. |
| `screen.rs` | Emulator headless `vt100` per pane: isi layar untuk `pane_read`, status bracketed paste, waktu output terakhir. |
| `tokens.rs` | Registri token pane (CSPRNG, perbandingan waktu-konstan, dicabut saat pane keluar). |
| `status.rs` | Pemindai OSC 7501 di thread baca: **menjawab query `OSC 7501;?`** (tanpa jawaban Claude diam, D-016) dan memetakan laporan ke status pane (§6). |
| `catalogue.rs` | Definisi agen (TOML tertanam + berkas pengguna di P1) dan deteksi instalasi. |
| `launch.rs` | Menyusun argv dan env per agen, termasuk injeksi MCP (§4). |
| `mcp/` | Server MCP `rmcp` (fitur `transport-streamable-http-server`) dipasang di `axum` pada `/mcp`; middleware token, `Origin` apa pun → 403, `Host` asing → 403; melayani protokol `2026-07-28` (tanpa sesi, Claude) dan `2025-06-18` (dengan `Mcp-Session-Id`, Codex) (D-017); implementasi tool. |
| `delivery.rs` | Antrean pesan per pane tujuan, syarat siap, penulisan tempelan, pengaman putaran (§5). |
| `policy.rs` | Relasi pane (induk, anak, peer), batas spawn, rate limit, pengaman kuota, aturan baca/kirim. |
| `approvals.rs` | Mengirim kartu persetujuan ke UI dan menunggu jawaban dengan batas waktu. |
| `store.rs` | Settings, log pesan, dan log audit di `~/Library/Application Support/<bundle-id>/`. |

### WebView (React + TypeScript, `src/`)
| Bagian | Tugas |
|---|---|
| `layout/` | Workspace (sidebar) dan pohon split. |
| `pane/TerminalPane.tsx` | `@xterm/xterm` + `addon-fit` + `addon-webgl`; menerima byte lewat Tauri `Channel`, mengirim ketikan lewat `invoke`. |
| `cards/` | Kartu persetujuan spawn dan jeda pengaman putaran. |
| `messages/` | Log pesan antar agen per workspace. |
| `settings/` | Sakelar dan angka batas. |

Byte terminal mengalir lewat `tauri::ipc::Channel` (dokumentasi Tauri: event "not designed for low latency or high
throughput", channel "designed to be fast and deliver ordered data"). Input, resize, dan perintah lain lewat `invoke`.

## 3. Siklus hidup pane

1. **Buat.** UI atau `pane_spawn` meminta pane baru. `policy` memeriksa batas dan, untuk `pane_spawn`, kuota 5 jam
   Claude pemanggil (D-033). `launch` menyusun argv/env.
2. **Jalankan.** `portable-pty` membuka PTY seukuran pane dan menjalankan program **langsung dengan argv, tanpa shell**,
   sehingga teks tugas dari agen tidak pernah diurai shell (D-009). `PATH` diambil sekali saat aplikasi mulai dari
   `$SHELL -l -i -c` yang mencetak `PATH` di antara dua penanda (agar keluaran `.zshrc` tidak ikut terbaca); `-i` perlu
   karena zsh non-interaktif tidak membaca `~/.zshrc`, tempat nvm/volta biasa menambah `PATH`. Pane shell menjalankan
   `$SHELL -l`.
3. **Alirkan.** Thread baca meneruskan setiap potongan output ke: `Screen` (vt100), pemindai OSC (`status`), dan
   `Channel` ke WebView. Bila WebView belum terpasang untuk pane itu, core sendiri yang mengonfirmasi byte yang masuk
   ke buffer putar ulang (1 MB terakhir, yang tertua dibuang), sehingga agen tidak pernah tertahan karena pane tidak
   tampil; saat WebView terpasang, buffer diputar ulang lalu flow control di bawah berlaku.
   **Flow control (D-018):** thread baca berhenti membaca bila byte yang terkirim tetapi belum dikonfirmasi xterm.js
   melebihi jendela kredit (128 KB, D-020); buffer kernel PTY lalu menahan program penghasil output. WebView
   mengirim `pane_ack {total}` berisi total kumulatif dari callback `term.write`, digabung paling banyak satu per
   putaran event loop lewat microtask (bukan `setTimeout`, yang ditahan WebKit saat jendela tersembunyi). Total
   kumulatif membuat ack yang hilang tertutup oleh ack berikutnya. Perintah `pane_write`/`pane_ack` `async` agar
   tidak berjalan di main thread. Tanpa ini xterm.js membuang data ("write data discarded").
4. **Selesai.** Saat proses keluar: status `exited`, token pane dicabut, klaim berkasnya dilepas (§5.6), pesan yang
   tertahan ditandai `dropped`, UI
   menawarkan "Jalankan ulang" atau "Buka shell di sini".
5. **Tutup.** Hanya orang. `SIGTERM` ke grup proses, `SIGKILL` setelah 3 detik. Saat aplikasi keluar, semua pane
   diperlakukan sama. Bila aplikasi crash, master PTY tertutup dan kernel mengirim `SIGHUP` ke sesi.

## 4. Meluncurkan agen dan menyuntik MCP

Setiap pane agen mendapat environment:

| Variabel | Isi |
|---|---|
| `AGENTDECK_MCP_URL` | `http://127.0.0.1:<port>/mcp` |
| `AGENTDECK_TOKEN` | Token acak 32 byte (hex) milik pane ini |
| `AGENTDECK_PANE_ID` | Mis. `p3` (informasi saja; identitas tetap dari token) |
| `TERM`, `COLORTERM` | `xterm-256color`, `truecolor` |

Environment anak disusun dari nol (D-014): hanya `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `PATH` dari shell login,
`TMPDIR` dan `SSH_AUTH_SOCK` dari environment launchd milik agentdeck (git lewat ssh, berkas sementara), ditambah tabel
di atas. Kunci API (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) **tidak** diteruskan; meneruskannya lewat daftar izin adalah
keputusan tertunda (D-019). Environment proses agentdeck tidak diwariskan, karena bila agentdeck dijalankan dari sesi
Claude Code (mis. `pnpm tauri dev`), variabel `CLAUDE_CODE_*` dan `ANTHROPIC_BASE_URL` ikut terbawa dan mengubah
perilaku agen (terlihat di S1).

Nama variabel sengaja bukan `ANTHROPIC_*`: Claude Code mengosongkan `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, dan
`NPM_TOKEN` saat mengekspansi `url`/`headers` MCP remote.

### Claude Code
```
claude --mcp-config '{"mcpServers":{"agentdeck":{"type":"http","url":"${AGENTDECK_MCP_URL}",
                      "headers":{"Authorization":"Bearer ${AGENTDECK_TOKEN}"},"timeout":330000}}}' \
       --allowedTools mcp__agentdeck \
       -- "<tugas awal>"
```
- `claude "query"`: "Start interactive session with initial prompt" (cli-reference).
- **`--` wajib sebelum tugas.** `--mcp-config <configs...>` dan `--allowedTools <tools...>` menerima banyak nilai
  (`claude --help` 2.1.303), jadi tanpa `--` tugas terbaca sebagai nama tool. Dengan `--`, tugas yang diawali `-` pun
  masuk utuh sebagai prompt (terbukti 2026-10-22, D-019).
- `--mcp-config` menambah server **hanya untuk sesi ini**. `--strict-mcp-config` sengaja **tidak** dipakai agar server
  MCP milik pengguna tetap termuat.
- `"timeout":330000`: batas waktu per panggilan tool untuk server ini. Tanpa itu Claude Code 2.1.308 memutus
  `pane_spawn` setelah 60 detik ("The operation timed out.") sementara kartu masih menunggu. Dengan itu kartu yang
  dijawab setelah 95 detik selesai normal (T3.5, D-024).
- `--allowedTools mcp__agentdeck` mengizinkan semua tool agentdeck tanpa prompt, karena gerbangnya ada di agentdeck
  (D-008). Izin lain (edit berkas, Bash) tetap ditanyakan Claude seperti biasa; tidak ada `--permission-mode`.
- Terbukti di S2 (D-017): bentuk di atas (pembungkus `mcpServers`, `${VAR}` dari env anak) tersambung, `whoami`
  dipanggil tanpa prompt, dan bagian `mcpServers` di `~/.claude.json` tidak berubah. Token tidak pernah ditulis ke berkas.

### Codex
```
codex -c 'mcp_servers.agentdeck.url="http://127.0.0.1:<port>/mcp"' \
      -c 'mcp_servers.agentdeck.bearer_token_env_var="AGENTDECK_TOKEN"' \
      -c 'mcp_servers.agentdeck.default_tools_approval_mode="approve"' \
      -c 'mcp_servers.agentdeck.tool_timeout_sec=330' \
      -- "<tugas awal>"
```
- `codex [PROMPT]` dan `-c key=value` ("Values parse as TOML if possible") terkonfirmasi di `codex --help` 0.163.0.
- `[mcp_servers.<id>]` mendukung `url` (streamable HTTP) dan `bearer_token_env_var` (config-reference).
- `default_tools_approval_mode = "approve"` berarti tool agentdeck tidak ditanyakan (D-008, terbukti S3). Anotasi tool
  tetap dipasang, tetapi perilaku mode `auto` tanpa `approve` tidak diuji dan tidak diandalkan.
- `tool_timeout_sec` bawaan Codex 60 detik (config-reference), lebih pendek dari kartu persetujuan `pane_spawn`
  (sampai 300 detik), jadi dinaikkan ke 330. Belum terbukti dengan CLI asli: server OpenAI "at capacity" selama
  uji T3.5 (D-024).
- Status Codex datang dari OSC 7501 tanpa konfigurasi (S5, D-016/D-017); `tui.notification*` tidak dipakai.
- `CODEX_HOME` tidak diubah (berisi `auth.json`, login pengguna). Flag `-a`/`-s` tidak dipakai: sandbox dan approval
  mengikuti setelan pengguna.
- Terbukti di S3 (D-017): kunci per baris tersambung, `whoami` dipanggil tanpa prompt, `~/.codex/config.toml` tidak
  berubah. Thread pembuat judul Codex memuat ulang config dan mencatat `invalid transport in mcp_servers.agentdeck`
  (DEBUG, untuk inline table maupun kunci per baris); sesi utama tidak terpengaruh. Pantau di versi Codex berikutnya.

### Yang tidak pernah dilakukan
Menulis `~/.claude.json`, `~/.claude/settings.json`, atau `~/.codex/config.toml`; memakai `--dangerously-*`,
`--yolo`, `bypassPermissions`; membaca keychain atau berkas kredensial; menjalankan agen lewat shell.

## 5. Pengiriman pesan antar agen

Ini inti produk: bagaimana `pane_send` dari agen A menjadi prompt di agen B.

### 5.1 Alur
```
A: pane_send {to:"p2", text}
  → McpServer: token → pane A; policy: boleh A→p2? ukuran ≤ 8 KB?
  → Delivery: masukkan ke antrean p2, catat `queued` di log, kembalikan {message:"m7", state:"queued"}
  → (menunggu p2 siap)
  → tulis tempelan ke PTY p2, tunggu sebentar, tulis Enter
  → catat `delivered`; status p2 → working
```

### 5.2 Kapan pane tujuan "siap"
Semua syarat harus benar:
1. Proses hidup dan antrean tidak dijeda pengaman putaran.
2. Status `idle`, **atau** status `unknown` dan output sepi ≥ `quiet_ms` (bawaan 1500).
3. Tidak ada ketikan orang di pane itu selama ≥ `typing_ms` (bawaan 2000). Core tahu waktunya karena setiap ketikan
   dari xterm.js lewat `invoke("pane_write")`.

4. Kotak input agen kosong. Teks yang diketik orang lebih dari `typing_ms` lalu (atau saran samar Claude, D-017) akan
   ikut terkirim bersama tempelan. **[T2.7]** buktikan cara mendeteksinya dari `vt100`; bila tidak bisa, pesan
   menunggu dan pane diberi chip "pesan menunggu: kosongkan input". **Dibuat (D-028):** setiap alasan tunggu tampil di chip
   (`1 pesan · kosongkan input`, `· kamu sedang mengetik`, `· agen sedang bekerja`, ...).

"Ketikan orang" hanya ketikan dan tempelan sungguhan: `TerminalPane` menandainya dari event keyboard/paste DOM.
Balasan otomatis xterm.js (DA/DSR, warna `OSC 10/11`, laporan fokus/mouse) juga lewat `onData`, tetapi dikirim dengan
penanda `reply` dan tidak mengubah `typing_ms` maupun pengaman putaran.

Angka `quiet_ms` dan `typing_ms` belum pernah diukur; disetel di T2.7 terhadap CLI asli. Pesan diproses satu per satu
(FIFO); setelah satu terkirim, pesan berikutnya menunggu pane `idle` lagi.

### 5.3 Bentuk yang ditulis ke PTY
```
ESC[200~[agentdeck] message from p2 (codex, "review: auth"). Reply with pane_send to "p2".
<isi pesan>ESC[201~      ← hanya bila vt100 melaporkan bracketed paste aktif
(jeda 150 ms)
\r
```
- Baris pertama selalu penanda pengirim, dibuat agentdeck (agen tidak bisa memalsukannya).
- Karakter kontrol dibuang kecuali `\n`; `ESC[201~` di dalam isi dibuang agar tidak menutup tempelan lebih awal.
- Satuan: 1 KB = 1024 byte. Isi ≤ 8192 byte UTF-8 setelah sanitasi, ditambah baris penanda ≤ 256 byte. S1 hanya
  menguji 8004 byte; **[T2.7]** uji tempelan maksimum termasuk penanda.
- Bila bracketed paste tidak aktif, pesan tidak dikirim dan ditandai `dropped` dengan alasan; tidak ada pengiriman
  karakter demi karakter.
- Claude Code meringkas tempelan > 800 karakter atau 3 baris menjadi `[Pasted text #N ...]`, tetapi isi lengkap tetap
  terkirim saat Enter (terminal-config).
- Jeda 150 ms lalu `\r` terbukti mengirim tempelan 3 baris dan 8 KB sebagai satu prompt di kedua TUI (S1, D-013).
- Claude Code menandai isi tempelan sebagai teks dari luar dan hanya mengikuti instruksi di dalamnya bila bagian yang
  diketik memintanya (terminal-config, "How Claude treats pasted text"). Karena pesan agentdeck seluruhnya tempelan,
  **[T2.7]** uji apakah Claude tetap mengerjakan permintaan dari pesan agen (mis. "perbaiki temuan ini"), bukan
  hanya menjawab pertanyaan. Bila tidak, baris penanda ditulis di luar tempelan sebagai teks ketikan.

### 5.4 Siapa boleh mengirim ke siapa
| Relasi pengirim → tujuan | `pane_send` | `pane_read` |
|---|---|---|
| induk → anak sendiri | ya | ya |
| anak → induk (`"parent"`) | ya | ya |
| peer (pane agen lain di workspace yang sama) | hanya bila "Agen boleh saling kirim" aktif | sama |
| pane shell | tidak (MVP) | tidak (MVP) |
| workspace lain, diri sendiri | tidak | tidak |

### 5.5 Pengaman putaran
- Penghitung per workspace: bertambah setiap pesan agen terkirim, kembali ke 0 saat orang menekan Enter di pane mana pun
  dalam workspace itu atau memilih "Lanjutkan" di kartu.
- Saat mencapai batas (bawaan 12), semua antrean di workspace dijeda dan kartu "Lanjutkan / Hentikan" muncul.
  "Hentikan" membuang antrean (`dropped`).
- `pane_send` tetap mengembalikan `state:"paused"` agar agen tahu pesannya tertahan.

### 5.6 Klaim berkas (D-033)
- `pane_claim` menyimpan ≤ 10 path absolut per pane di registri (`panes.rs`). Path diberikan relatif terhadap `cwd`
  pane; `mcp/claim.rs` menolak path absolut, `..`, dan symlink yang keluar dari `cwd` (leluhur terdalam yang ada
  di-`canonicalize`, sisanya ditempel apa adanya; symlink gantung ditolak).
- Tumpang tindih = path sama atau satu di dalam yang lain, per komponen. Dicek terhadap klaim pane hidup lain di
  workspace yang sama, di bawah kunci registri yang sama dengan penyimpanannya, jadi dua klaim tidak berpacu.
- Dilepas saat proses keluar dan saat pane ditutup. `pane_list` menampilkan klaim tiap pane.
- Hanya penanda kerja sama: agentdeck tidak mencegah agen menulis berkas yang diklaim pane lain. Anak dengan
  `worktree: true` punya path absolut lain, jadi klaimnya tidak pernah bertabrakan dengan pane di pohon induk.

## 6. Deteksi status

Status: `starting`, `working`, `idle`, `needs_you`, `exited`, `unknown`. Sumber, dari yang paling dipercaya:

| Sumber | Claude Code | Codex |
|---|---|---|
| Escape dari agen sendiri | OSC 7501 (Program Status Protocol rev 0.4, superlogical.com/rex/docs/build/program-status). Claude mengirim query `OSC 7501;?` dan **hanya melapor bila terminal menjawab**; agentdeck menjawab `ESC]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth ESC\` (S4, D-016). Terlihat: `idle`, `working` (+`msg`), `done`, `blocked:kind=permission` (+`msg` base64 mis. "approve Write: …"). | OSC 7501 tanpa query: `state=idle\|working:app=codex` (S1, S5). `blocked` saat minta izin, tanpa `kind` dan `msg` (S5, D-017). Setelah galat server Codex melapor `idle`, bukan `error`. |
| Hook, OSC 9 | Tidak dipakai (D-016). Ditinjau ulang hanya bila OSC 7501 berubah. | Tidak dipakai (D-017). |
| Heuristik | output dalam 1 detik terakhir → `working`; selain itu `unknown` | sama |

Pemetaan OSC 7501 → status agentdeck: `idle`/`done` → `idle`, `working` → `working`, `blocked` → `needs_you` (label
menurut `kind`: `permission` "butuh izin", `question` "butuh jawaban", `auth` "perlu login", tanpa `kind` "butuh kamu"),
`error` → `idle` dengan tanda galat di header. Claude melapor `blocked:kind=auth` dengan `msg` "Login expired · Please
run /login" saat login kedaluwarsa (terlihat 2026-10-22). Codex tidak memancarkan `error`: setelah galat server ia
melapor `idle` (D-017), jadi galat Codex tidak terdeteksi dari status. Rekaman `working`/`blocked` dibuang saat proses keluar (rev 0.4).
`msg` didekode dan ditampilkan sebagai tooltip header; panjang dibatasi, karakter kontrol ditolak (spesifikasi).

Aturan: output baru tidak pernah mengubah `needs_you` atau `idle` dari sinyal resmi menjadi `working` sebelum ada
sinyal berikutnya atau input baru. `unknown` tidak pernah dianggap selesai.

## 7. Keamanan

| Ancaman | Penangkal |
|---|---|
| Halaman web memanggil `127.0.0.1:<port>` (DNS rebinding, CSRF) | Bind loopback, `allowed_hosts` loopback (bawaan rmcp), tolak `Origin` apa pun, token wajib. |
| Proses lokal lain menebak port | Token 256-bit dari CSPRNG, dibandingkan waktu-konstan; token → pane, dicabut saat pane keluar. |
| Agen memalsukan identitas | Identitas hanya dari token; tidak ada argumen "from". Penanda pengirim ditulis agentdeck. |
| Prompt injection berantai | Anak tanpa flag bypass; izin berbahaya tetap ditanyakan agen ke orang; peer mati bawaan; `instructions` meminta pesan agen lain diperlakukan sebagai data. |
| Agen menjalankan perintah lewat agentdeck | Tidak ada tool untuk shell; `pane_spawn` tidak menerima perintah, flag, atau env; agen dijalankan tanpa shell. |
| Kebocoran token | Token hanya di memori dan env anak; tidak di log, berkas, UI, pesan galat. |
| Pengguna yang sama membaca env proses (`ps -E`) | Di luar model ancaman: batas kepercayaan adalah akun pengguna. |

Identitas pemanggil diambil dari header `Authorization` per permintaan (middleware axum, lalu `Parts` di handler tool),
bukan dari `Mcp-Session-Id`. Protokol `2026-07-28` menghapus sesi (SEP-2567), tetapi Codex 0.163.0 masih bernegosiasi
`2025-06-18` dengan sesi, jadi server melayani keduanya (`legacy_session_mode: true`, bawaan rmcp) dan identitas tetap
dari token di kedua mode (D-017). `allowed_origins` rmcp kosong secara bawaan berarti `Origin` tidak divalidasi; penolakan
`Origin` dilakukan middleware kita.

## 8. Penyimpanan
`~/Library/Application Support/<bundle-id>/`:
- `settings.json`: sakelar dan angka batas.
- `layout.json` (T4.3, D-025): versi, workspace aktif, lalu per workspace id, nama, folder, pohon split, pane terfokus,
  dan per pane id, agen, nama agen, judul, `cwd`, induk, worktree (diisi core dari registri). Tanpa token, status,
  proses, pesan, laporan, atau zoom. Dibaca sekali di `setup`, divalidasi di `layout.rs`.
- `logs/messages-YYYY-MM-DD.jsonl`: `ts`, `workspace`, `from`, `to`, `message_id`, `state`, `reason`, `text`.
  Disimpan 14 hari.
- `logs/audit-YYYY-MM-DD.jsonl`: `spawn_requested`, `spawn_approved`, `spawn_denied` (`denied`/`timeout`/`launch
  failed`), `spawn_refused` (kode, sebelum kartu), `loop_paused`, `pane_closed` (D-024).

Tidak ada token di mana pun. SQLite ditunda sampai ada kebutuhan pencarian (D-010).

## 9. Struktur repo
```
agentdeck/
├── AGENTS.md, CLAUDE.md, README.md
├── docs/                  PRD, ARCHITECTURE, MCP-TOOLS, UI-UX, IMPLEMENTATION-PLAN, PHASE-0, SETUP, DECISIONS, RESEARCH
├── src/                   WebView: React + TS
├── src-tauri/
│   ├── src/               modul §2
│   ├── catalogue.toml     agen bawaan
│   └── tauri.conf.json
├── spikes/                alat Phase 0 (pty_probe.py); hasil di spikes/out/ tidak di-commit
└── tests/fake-agent/      program kecil yang meniru TUI agen untuk tes integrasi
```

## 10. Phase 0: spike kelayakan
Langkah dan perintah lengkap: `docs/PHASE-0.md`. Setiap spike punya kriteria lulus. Hasilnya dicatat di DECISIONS dengan versi CLI yang diuji.

| # | Pertanyaan | Lulus bila |
|---|---|---|
| S1 | Tempel + Enter | Di Claude Code dan Codex, tempelan 3 baris dan tempelan 8 KB lewat bracketed paste + `\r` terkirim sebagai satu prompt utuh. **Lulus (D-013).** |
| S2 | Injeksi MCP Claude | `/mcp` menampilkan `agentdeck` tersambung; tool bisa dipanggil tanpa prompt; bagian `mcpServers` di `~/.claude.json` sama sebelum/sesudah (berkas itu sendiri berubah tiap sesi). **Lulus (D-017).** |
| S3 | Injeksi MCP Codex | Tool agentdeck terdaftar dan bisa dipanggil tanpa prompt; tidak ada entri `agentdeck` di `~/.codex/config.toml` sesudahnya. **Lulus (D-017).** |
| S4 | Status Claude | OSC 7501 terlihat untuk working/idle/needs_you setelah query dijawab. **Lulus (D-016).** |
| S5 | Status Codex | OSC 7501 `blocked` saat minta izin dan `idle` saat turn selesai. **Lulus (D-017).** |
| S6 | Throughput | 50 MB output (`cat` berkas besar) lewat Channel tanpa UI beku; ketikan tetap < 50 ms. **Lulus dengan flow control (D-018).** |
| S7 | Layar headless | `vt100` membaca layar Claude Code dan Codex dengan benar dan `bracketed_paste()` bernilai benar setelah TUI siap. **Lulus (D-013).** |
| S8 | Server MCP | `rmcp` 3.x + axum: middleware token, token → pane di handler, `instructions` terlihat oleh kedua agen, negosiasi versi protokol dengan kedua klien. **Lulus (D-017).** |

## 11. Pengujian
- **Unit (Rust):** policy relasi, batas dan rate limit, pengaman putaran, sanitasi pesan, pembentukan argv/env,
  middleware token dan `Host`/`Origin`.
- **Integrasi (Rust):** `tests/fake-agent` yang menyalakan bracketed paste, memanggil server MCP sungguhan, dan meniru
  dua perilaku status: mode `claude` (kirim query `OSC 7501;?`, melapor hanya setelah dijawab, `blocked:kind=permission`)
  dan mode `codex` (melapor tanpa query, `blocked` tanpa `kind`), untuk menguji antrean ujung ke ujung tanpa kuota.
- **UI (Vitest):** komponen kartu dan log pesan.
- **Manual dengan CLI asli** sebelum setiap rilis: alur A–D di PRD §7, dicatat dengan versi CLI.

## 12. Belum terbukti
Klaim yang dipakai desain tetapi belum diuji. Task yang disebut wajib membuktikannya (atau mengubah desain) sebelum
dianggap selesai.

| Klaim | Task |
|---|---|
| ~~Tugas awal sampai 16 KB lewat argv diterima utuh oleh kedua CLI.~~ **Terbukti T2.3 (2026-10-22):** tugas tepat 16384 byte yang diawali `-`, diluncurkan lewat `panes::start` (argv + injeksi MCP sungguhan), dikerjakan Claude Code 2.1.308 dan Codex 0.163.0: keduanya memanggil `whoami` agentdeck tanpa prompt izin (hasil `p2`) dan mengutip baris terakhir tugas. | T2.3 |
| Kotak input kosong bisa dideteksi dari `vt100`; teks lama atau saran samar Claude tidak ikut terkirim (§5.2). **T2.7, terbukti sebagian (2026-10-22):** pada Claude Code 2.1.308 dan Codex 0.163.0 asli (lewat `panes::start`), `input_state` = kosong saat idle, sehingga pesan langsung `delivered`; saran samar Claude ("Try …") tidak dianggap teks. Teks yang diketik orang lalu dibiarkan di kotak input belum diuji di CLI asli (hanya fake-agent: pesan menunggu sampai input kosong dan `typing_ms` lewat). | T2.7 |
| Claude mengerjakan permintaan dari pesan agen yang seluruhnya tempelan (§5.3). **T2.7, belum terbukti:** uji Codex → Claude (`src-tauri/tests/real_cli.rs`) gagal di sisi Codex: "Selected model is at capacity" (model bawaan dan `-m gpt-5.6-luna`), jadi permintaan tidak pernah dikirim. Tempelan 8 KB langsung dari core ke Claude memulai giliran kerja (status `working`), tetapi jawabannya tidak ditunggu. Ulangi dengan `cargo test --test real_cli -- --ignored --nocapture` saat server Codex lega. **Uji manual pemilik 2026-10-22 (Claude Code 2.1.308, Codex 0.163.0, di aplikasi):** alur Claude → Codex → Claude berhasil: tempelan dari p1 diterima Codex sebagai satu prompt, Codex **mengerjakan permintaan di dalamnya** (memanggil `pane_send` membalas "halo" ke p1), balasan diterima Claude yang mengenalinya sebagai pesan agen dan memilih tidak membalas lagi (menghindari putaran). Terbukti untuk Codex; untuk Claude terbukti pesan agen dibaca dan ditindaklanjuti sesuai `instructions`, tetapi permintaan kerja (mis. "perbaiki temuan ini") dari Codex ke Claude belum diuji. | T2.7 |
| Tempelan maksimum (8192 + penanda) terkirim utuh. **T2.7, terbukti sebagian:** Codex 0.163.0 menampilkan satu tempelan `[Pasted Content 8286 chars]` (8192 + penanda 93 + `\n`); Claude 2.1.308 menampilkan `[Pasted text #1 +215 lines]` dan baris terakhir pesan terlihat di layarnya, lalu mulai bekerja. Jawaban yang mengutip baris terakhir belum ditunggu (lihat baris di atas). | T2.7 |
| `tool_timeout_sec=330` membuat Codex menunggu kartu persetujuan; batas waktu tool MCP di Claude cukup panjang. **T3.5 (2026-10-23):** klaim Claude **salah**: Claude Code 2.1.308 memutus panggilan di 60 s. Diperbaiki dengan `"timeout":330000` di `--mcp-config`; kartu yang disetujui setelah ~95 s kini selesai (panggilan 107 s) dan Claude melanjutkan. Codex belum terbukti: model bawaan dan `gpt-5.6-luna` "at capacity" (D-024). | T3.2, T3.5 |
| Worktree anak yang masih terbuka saat aplikasi keluar dibersihkan. **Tidak:** hanya penutupan pane oleh orang yang menghapus worktree tanpa perubahan; yang tersisa setelah aplikasi keluar (atau crash) tetap ada di `.worktrees/` bersama branch-nya (T4.1, D-024). **T4.3:** anak yang dipulihkan dari `layout.json` membawa worktree-nya lagi (bila masih utuh, `worktree::still_ours`) dan membersihkannya saat ditutup; worktree yang tertinggal setelah crash tanpa pane tetap tidak dibersihkan. | T4.3 |
| Pane di workspace tidak aktif, jendela tersembunyi, atau diminimalkan tetap mengalir dan statusnya tetap terbaca (D-018 hanya mengukur jendela terlihat). | T1.4 |
| ~~`pane_read` pada alternate screen: `vt100` tidak punya scrollback di sana; `lines` dibatasi tinggi layar bila alternate screen aktif.~~ **Terbukti T2.4:** `Screen::read` mengembalikan paling banyak setinggi layar di alternate screen (tes `screen.rs`), `pane_read` memakainya; deskripsi tool menyebut batas ini. | T2.4 |
| Saran samar Claude bisa dibedakan dari teks biasa lewat atribut `vt100`; bila tidak, dicatat sebagai batasan `pane_read`. **T2.4:** dibedakan untuk deteksi input (atribut dim, terbukti di layar Claude 2.1.308 asli). `pane_read` membuang sel samar di baris input (baris kursor diawali `❯`/`›` tidak samar), jadi saran ("Try …") tidak terbaca seperti teks diketik; teks samar di baris lain tetap ada (tes unit `screen.rs`, belum dicoba di CLI asli). | T2.4 |
| `SIGTERM`/`SIGKILL` ke grup proses dan `SIGHUP` saat crash menghentikan anak agen (Bash tool, server MCP stdio) yang mungkin membuat sesi sendiri. **T1.8, terbukti sebagian:** tutup pane/aplikasi menjangkau seluruh keturunan (grup lain, sesi sendiri, abaikan HUP/TERM) lewat potret pohon proses (tes `pty.rs`); proses yang sudah lepas ke launchd sebelum tutup (fork ganda) tidak terjangkau. Saat crash, shell dan job biasanya berhenti, tetapi proses di sesi sendiri (seperti Bash tool Claude Code) dan job yang mengabaikan `SIGHUP` **tetap hidup** kecuali agen induknya membersihkannya; tanpa daemon pengawas ini batasan. Perilaku agen asli saat crash belum diuji. | T1.3, T1.8 |
| Env dari nol cukup untuk kerja nyata: `git push` lewat ssh, `npm install`, proxy/CA perusahaan, `LANG` saat dibuka dari Finder. **T1.7, terbukti sebagian:** di pane dari aplikasi, `npm view` dan `npm install --dry-run` berhasil; tanpa `LANG` di agentdeck, anak mendapat `en_US.UTF-8` (D-021); `CLAUDE_CODE_*` milik agentdeck tidak ada di env proses `claude`/`codex` (`ps eww`). `SSH_AUTH_SOCK` diteruskan, tetapi `git` lewat ssh belum terbukti: mesin pemilik tidak punya kunci ssh GitHub (`ssh -T git@github.com` ditolak juga dari terminal biasa). Proxy/CA perusahaan tidak diuji (tidak ada). | T1.7 |
| `$SHELL -l -i -c` dengan penanda membaca `PATH` dengan benar (zsh, bash, fish) tanpa menggantung. | T1.6 |
| 6 pane agen aktif di MacBook Air M1 8 GB tanpa UI tersendat (S6 hanya 2 pane, ±110 MB RSS). **Belum diuji di T2.7** (butuh aplikasi + UI T2.8). | T2.7 |

