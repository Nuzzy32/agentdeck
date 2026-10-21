# agentdeck — Cara kerja

Dokumen ini menjelaskan bagaimana agentdeck bekerja. Apa yang dibangun dan kenapa ada di `docs/PRD.md`; kontrak tool di
`docs/MCP-TOOLS.md`. Fakta eksternal diverifikasi 2026-10-21 terhadap dokumentasi resmi dan `--help` CLI terpasang
(Claude Code 2.1.303, Codex 0.163.0). Yang ditandai **[Phase 0]** belum terbukti dan wajib diuji sebelum dipakai.

## 1. Gambaran besar

```
┌─────────────────────────── agentdeck.app (satu proses Tauri) ───────────────────────────┐
│                                                                                          │
│  WebView (React + xterm.js)                    Core (Rust, tokio)                         │
│  ┌──────────────────────────┐   invoke    ┌──────────────────────────────────────────┐   │
│  │ Sidebar / split layout   │ ──────────▶ │ PaneManager   portable-pty, 1 thread baca│   │
│  │ TerminalPane × N         │ ◀────────── │ Screen        vt100 per pane             │   │
│  │ Kartu persetujuan        │   Channel   │ StatusTracker OSC / hook / heuristik     │   │
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
| `status.rs` | Menentukan status pane dari sinyal agen (§6). |
| `catalogue.rs` | Definisi agen (TOML tertanam + berkas pengguna di P1) dan deteksi instalasi. |
| `launch.rs` | Menyusun argv dan env per agen, termasuk injeksi MCP (§4). |
| `mcp/` | Server MCP `rmcp` (fitur `transport-streamable-http-server`) dipasang di `axum` pada `/mcp`; middleware token dan cek `Host`/`Origin`; implementasi tool. |
| `delivery.rs` | Antrean pesan per pane tujuan, syarat siap, penulisan tempelan, pengaman putaran (§5). |
| `policy.rs` | Relasi pane (induk, anak, peer), batas spawn, rate limit, aturan baca/kirim. |
| `approvals.rs` | Mengirim kartu persetujuan ke UI dan menunggu jawaban dengan batas waktu. |
| `store.rs` | Settings dan log di `~/Library/Application Support/<bundle-id>/`. |

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

1. **Buat.** UI atau `pane_spawn` meminta pane baru. `policy` memeriksa batas. `launch` menyusun argv/env.
2. **Jalankan.** `portable-pty` membuka PTY seukuran pane dan menjalankan program **langsung dengan argv, tanpa shell**,
   sehingga teks tugas dari agen tidak pernah diurai shell (D-009). `PATH` diambil sekali saat aplikasi mulai dari
   `$SHELL -l -c 'printf %s "$PATH"'`. Pane shell menjalankan `$SHELL -l`.
3. **Alirkan.** Thread baca meneruskan setiap potongan output ke: `Screen` (vt100), pemindai OSC (`status`), dan
   `Channel` ke WebView. Bila WebView belum terpasang, 1 MB terakhir disimpan untuk diputar ulang.
4. **Selesai.** Saat proses keluar: status `exited`, token pane dicabut, pesan yang tertahan ditandai `dropped`, UI
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
ditambah tabel di atas. Environment proses agentdeck tidak diwariskan, karena bila agentdeck dijalankan dari sesi
Claude Code (mis. `pnpm tauri dev`), variabel `CLAUDE_CODE_*` dan `ANTHROPIC_BASE_URL` ikut terbawa dan mengubah
perilaku agen (terlihat di S1).

Nama variabel sengaja bukan `ANTHROPIC_*`: Claude Code mengosongkan `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, dan
`NPM_TOKEN` saat mengekspansi `url`/`headers` MCP remote.

### Claude Code
```
claude --mcp-config '{"mcpServers":{"agentdeck":{"type":"http","url":"${AGENTDECK_MCP_URL}",
                      "headers":{"Authorization":"Bearer ${AGENTDECK_TOKEN}"}}}}' \
       --allowedTools mcp__agentdeck \
       "<tugas awal>"
```
- `claude "query"`: "Start interactive session with initial prompt" (cli-reference).
- `--mcp-config` menambah server **hanya untuk sesi ini**. `--strict-mcp-config` sengaja **tidak** dipakai agar server
  MCP milik pengguna tetap termuat.
- `--allowedTools mcp__agentdeck` mengizinkan semua tool agentdeck tanpa prompt, karena gerbangnya ada di agentdeck
  (D-008). Izin lain (edit berkas, Bash) tetap ditanyakan Claude seperti biasa; tidak ada `--permission-mode`.
- **[Phase 0]** Ekspansi `${VAR}` terdokumentasi untuk `.mcp.json`, belum untuk `--mcp-config`; bentuk pembungkus
  `mcpServers` juga belum dipastikan. Cadangan: tulis nilai literal di argumen JSON (terlihat oleh `ps` milik pengguna
  yang sama, sama dengan env). Token tidak pernah ditulis ke berkas.

### Codex
```
codex -c 'mcp_servers.agentdeck={url="http://127.0.0.1:<port>/mcp",bearer_token_env_var="AGENTDECK_TOKEN",default_tools_approval_mode="approve"}' \
      -c 'tui.notifications=["agent-turn-complete","approval-requested"]' \
      -c 'tui.notification_method="osc9"' \
      "<tugas awal>"
```
- `codex [PROMPT]` dan `-c key=value` ("Values parse as TOML if possible") terkonfirmasi di `codex --help` 0.163.0.
- `[mcp_servers.<id>]` mendukung `url` (streamable HTTP) dan `bearer_token_env_var` (config-reference).
- `default_tools_approval_mode = "approve"` berarti tool agentdeck tidak ditanyakan (D-008). Tool juga diberi anotasi
  `destructiveHint:false, openWorldHint:false` sehingga mode bawaan `auto` pun tidak bertanya.
- `tui.notification_method="osc9"` membuat Codex mengirim notifikasi sebagai escape OSC 9 ke terminal, yang kita baca
  untuk status (§6). agentdeck meneruskannya sebagai notifikasi macOS bila jendela tidak fokus.
- `CODEX_HOME` tidak diubah (berisi `auth.json`, login pengguna). Flag `-a`/`-s` tidak dipakai: sandbox dan approval
  mengikuti setelan pengguna.
- **[Phase 0]** Bentuk inline table untuk `url` belum diuji (contoh resmi memakai `command`/`args`).

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

Angka bawaan diatur ulang setelah Phase 0. Pesan diproses satu per satu (FIFO); setelah satu terkirim, pesan
berikutnya menunggu pane `idle` lagi.

### 5.3 Bentuk yang ditulis ke PTY
```
ESC[200~[agentdeck] message from p2 (codex, "review: auth"). Reply with pane_send to "p2".
<isi pesan>ESC[201~      ← hanya bila vt100 melaporkan bracketed paste aktif
(jeda 150 ms)
\r
```
- Baris pertama selalu penanda pengirim, dibuat agentdeck (agen tidak bisa memalsukannya).
- Karakter kontrol dibuang kecuali `\n`; `ESC[201~` di dalam isi dibuang agar tidak menutup tempelan lebih awal.
- Bila bracketed paste tidak aktif, pesan tidak dikirim dan ditandai `dropped` dengan alasan; tidak ada pengiriman
  karakter demi karakter.
- Claude Code meringkas tempelan > 800 karakter atau 3 baris menjadi `[Pasted text #N ...]`, tetapi isi lengkap tetap
  terkirim saat Enter (terminal-config).
- Jeda 150 ms lalu `\r` terbukti mengirim tempelan 3 baris dan 8 KB sebagai satu prompt di kedua TUI (S1, D-013).
- Claude Code menandai isi tempelan sebagai teks dari luar dan hanya mengikuti instruksi di dalamnya bila bagian yang
  diketik memintanya (terminal-config, "How Claude treats pasted text"). Karena pesan agentdeck seluruhnya tempelan,
  **[Phase 2]** uji apakah Claude tetap mengerjakan permintaan dari pesan agen (mis. "perbaiki temuan ini"), bukan
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

## 6. Deteksi status

Status: `starting`, `working`, `idle`, `needs_you`, `exited`, `unknown`. Sumber, dari yang paling dipercaya:

| Sumber | Claude Code | Codex |
|---|---|---|
| Escape dari agen sendiri | OSC 7501, protokol status pane resmi (butuh v2.1.295+, tidak di dalam tmux) **[Phase 0: format]** | OSC 9 dari `tui.notifications`: `agent-turn-complete` → `idle`, `approval-requested` → `needs_you` **[Phase 0: format teks]** |
| Hook | `--settings` dengan `Stop` → `idle`, `Notification` `permission_prompt` → `needs_you`, `idle_prompt` → `idle`, `UserPromptSubmit` → `working` **[Phase 0: apakah `--settings` menimpa hook milik pengguna]** | Tidak dipakai: hook Codex harus dipercaya lewat `/hooks` atau `--dangerously-bypass-hook-trust` |
| Heuristik | output dalam 1 detik terakhir → `working`; selain itu `unknown` | sama |

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
bukan dari `Mcp-Session-Id`: protokol MCP `2026-07-28` menghapus sesi (SEP-2567).

## 8. Penyimpanan
`~/Library/Application Support/<bundle-id>/`:
- `settings.json`: sakelar dan angka batas.
- `layout.json` (P1): workspace dan split.
- `logs/messages-YYYY-MM-DD.jsonl`: `ts`, `workspace`, `from`, `to`, `message_id`, `state`, `reason`, `text`.
  Disimpan 14 hari.
- `logs/audit-YYYY-MM-DD.jsonl`: spawn diminta/disetujui/ditolak, jeda pengaman putaran, pane ditutup.

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
| S1 | Tempel + Enter | Di Claude Code dan Codex, tempelan 3 baris dan tempelan 8 KB lewat bracketed paste + `\r` terkirim sebagai satu prompt utuh. |
| S2 | Injeksi MCP Claude | `/mcp` menampilkan `agentdeck` tersambung; tool bisa dipanggil tanpa prompt; bagian `mcpServers` di `~/.claude.json` sama sebelum/sesudah (berkas itu sendiri berubah tiap sesi). |
| S3 | Injeksi MCP Codex | Tool agentdeck terdaftar dan bisa dipanggil tanpa prompt; tidak ada entri `agentdeck` di `~/.codex/config.toml` sesudahnya. |
| S4 | Status Claude | Format OSC 7501 terdokumentasi dan terlihat di stream PTY untuk working/idle/needs_you; bila tidak, hook lewat `--settings` bekerja tanpa mematikan hook pengguna. |
| S5 | Status Codex | OSC 9 muncul di stream untuk turn selesai dan permintaan approval, teksnya bisa dibedakan. |
| S6 | Throughput | 50 MB output (`cat` berkas besar) lewat Channel tanpa UI beku; ketikan tetap < 50 ms. |
| S7 | Layar headless | `vt100` membaca layar Claude Code dan Codex dengan benar dan `bracketed_paste()` bernilai benar setelah TUI siap. |
| S8 | Server MCP | `rmcp` 3.x + axum: middleware token, token → pane di handler, `instructions` terlihat oleh kedua agen, negosiasi versi protokol dengan kedua klien. |

## 11. Pengujian
- **Unit (Rust):** policy relasi, batas dan rate limit, pengaman putaran, sanitasi pesan, pembentukan argv/env,
  middleware token dan `Host`/`Origin`.
- **Integrasi (Rust):** `tests/fake-agent` yang menyalakan bracketed paste, mencetak OSC status, dan memanggil server MCP
  sungguhan, untuk menguji antrean ujung ke ujung tanpa kuota.
- **UI (Vitest):** komponen kartu dan log pesan.
- **Manual dengan CLI asli** sebelum setiap rilis: alur A–D di PRD §7, dicatat dengan versi CLI.
