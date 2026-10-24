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

## D-005 — Injeksi per peluncuran, tidak menulis config global (2026-10-21; terbukti di S2/S3, D-017)
Claude Code: `--mcp-config <json>` tanpa `--strict-mcp-config` (agar MCP pengguna tetap termuat). Codex: kunci per
baris `-c mcp_servers.agentdeck.url=…`, `.bearer_token_env_var=…`, `.default_tools_approval_mode="approve"` (D-017). Alasan: MXterm Pro menulis ke `~/.claude.json` dan `~/.codex/config.toml`; di luar
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

## D-007 — Status dari sinyal agen sendiri (2026-10-21; terbukti di S4/S5, D-016, D-017)
Urutan: OSC 7501 dari agen (Claude setelah query dijawab, Codex tanpa query), lalu heuristik output. Hook Claude lewat
`--settings` dan OSC 9 Codex tidak dipakai; ditinjau ulang hanya bila OSC 7501 berubah. Hook Codex juga butuh kepercayaan
lewat `/hooks` atau `--dangerously-bypass-hook-trust`. Membaca layar tidak dipakai untuk status karena rapuh terhadap perubahan TUI.

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

## D-016 — Phase 0 S4: status Claude Code lewat OSC 7501 (2026-10-21)
Versi: Claude Code 2.1.303, Codex 0.163.0. Perintah: `docs/PHASE-0.md` S4 (`pty_probe.py --osc7501`, environment bersih).
Spesifikasi: Program Status Protocol (OSC 7501) rev 0.4, 2026-10-10,
https://www.superlogical.com/rex/docs/build/program-status (ringkasan: control-codes.page/proposals/osc-7501).
Hasil S4: **lulus.** Claude mengirim `OSC 7501;?` saat mulai dan hanya melapor bila terminal menjawab
`OSC 7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth ST`. Setelah dijawab:
`state=idle:app=claude-code` → `state=working` (dengan `msg`=base64 "Writing spikes/out/s4.txt") → `state=done`, dan
`state=blocked:kind=permission:msg=<base64 "approve Write: /…/s4.txt">` saat meminta izin (diuji dengan
`--permission-mode default` khusus spike). Ketiga keadaan PRD terbedakan tanpa hook, jadi hook `--settings` tidak
dipakai (pertanyaan "apakah `--settings` menimpa hook pengguna" tidak perlu dijawab).
Hasil S5 (sebagian): Codex memancarkan `state=idle|working:app=codex` tanpa query dan tanpa konfigurasi. Status
`blocked` belum terlihat: dua kali server membalas "Selected model is at capacity" sebelum Codex sampai ke permintaan
izin; setelah galat itu Codex melapor `idle`, bukan `error`. Ulangi S5 saat kapasitas normal.
Dampak ke desain: `status.rs` membaca OSC 7501 untuk kedua agen dan `screen.rs`/pemindai OSC wajib menjawab query
`OSC 7501;?` (tanpa jawaban, Claude diam). Pemetaan ke status agentdeck di ARCHITECTURE §6. `msg` adalah teks dari
agen: didekode dengan batas spesifikasi dan hanya ditampilkan, tidak pernah dieksekusi. Konfigurasi
`tui.notification*` untuk Codex ditahan sebagai cadangan sampai S5 tuntas.
Bukti: `spikes/out/s4-*.bin`, `spikes/out/s5-*.bin` (tidak di-commit).

## D-017 — Phase 0 S8, S2, S3, S5: server MCP, injeksi per peluncuran, status Codex (2026-10-21)
Versi: Claude Code 2.1.303, Codex 0.163.0, `rmcp` 3.6.0, `axum` 0.8.9. Sumber: `spikes/s8-mcp/`.
- **S8 lulus.** Server `rmcp` (`StreamableHttpService` + `LocalSessionManager`) di `127.0.0.1:0`, middleware axum:
  `Origin` ada → 403, token tidak ada/salah → 401, token sah → `PaneId` dimasukkan ke extensions request dan dibaca tool
  lewat `Extension<http::request::Parts>`. `Host` asing → 403 dari `allowed_hosts` bawaan rmcp. `instructions` terbaca
  kedua agen. Catatan rmcp: `allowed_origins` kosong berarti Origin **tidak** divalidasi (bawaan), jadi penolakan
  Origin dilakukan middleware kita (atau `enforce_origin_validation()`).
- **Versi protokol:** Claude bernegosiasi `2026-07-28` (stateless, SEP-2567), Codex `2025-06-18` (sesi lama,
  `Mcp-Session-Id`). Identitas dari token bekerja di keduanya; server wajib melayani dua mode (bawaan rmcp
  `legacy_session_mode: true` melakukannya).
- **S2 lulus.** `claude --mcp-config '{"mcpServers":{"agentdeck":{"type":"http","url":"${AGENTDECK_MCP_URL}",
  "headers":{"Authorization":"Bearer ${AGENTDECK_TOKEN}"}}}}' --allowedTools mcp__agentdeck`: `${VAR}` diekspansi dari
  env anak, `whoami` → p1 tanpa prompt, bagian `mcpServers` di `~/.claude.json` sama sebelum/sesudah. Cadangan nilai
  literal tidak diperlukan.
- **S3 lulus** dengan kunci per baris (`-c mcp_servers.agentdeck.url=…`, `.bearer_token_env_var=…`,
  `.default_tools_approval_mode="approve"`): `whoami` → p2 tanpa prompt, `~/.codex/config.toml` tidak berubah
  (shasum sama). Inline table juga tersambung ke server, tetapi turn-nya tidak pernah selesai karena galat server;
  kunci per baris dipakai karena itulah yang terbukti ujung ke ujung. Thread pembuat judul Codex mencatat
  `invalid transport in mcp_servers.agentdeck` (DEBUG) untuk kedua bentuk; sesi utama tidak terpengaruh.
- **S5 lulus.** Dengan `-s read-only -a on-request` (khusus spike) Codex memancarkan `state=blocked:app=codex` saat
  meminta izin (tanpa `kind`/`msg`); berkas tidak dibuat. `tui.notification*` tidak diperlukan.
- **Gangguan server OpenAI:** banyak percobaan Codex gagal "Selected model is at capacity" (log Codex:
  `run_turn: Turn error`), juga untuk tugas latar Codex yang tidak memakai MCP. Percobaan sah memakai `-m gpt-6-luna`
  (S5) dan `-m gpt-5.6-luna` (S3). Dampak ke desain: header pane perlu menampilkan bahwa agen kembali `idle` setelah
  galat; `pane_send` ke pane yang baru gagal tetap dikirim (agen sendiri yang melapor).
- **Temuan untuk `pane_read`:** Claude menampilkan saran prompt berikutnya sebagai teks samar di kotak input (mis.
  "catat hasil spike ini di PHASE-0.md"); `vt100` membacanya sebagai teks biasa. `pane_read` tidak boleh
  menganggapnya ketikan orang.
Bukti: `spikes/out/s2-*`, `s3-*`, `s5-*` (tidak di-commit), log server di `/tmp/agentdeck-s8/err.log`.

## D-018 — Phase 0 S6: throughput PTY → Channel → xterm.js butuh flow control (2026-10-22)
Versi: Tauri 2.12.4, `portable-pty` 0.9.0, `@xterm/xterm` 6.0.0 + `@xterm/addon-webgl` 0.19.0, WKWebView macOS.
Sumber: `spikes/s6-throughput/` (dua pane xterm.js; pane A dibanjiri, pane B menjalankan `cat` dan setiap 100 ms satu
karakter dikirim lalu diukur waktu kembalinya; jeda antar-frame lewat `requestAnimationFrame`).
- **Tanpa flow control: gagal.** xterm.js membuang data saat buffer tulisnya penuh (71 077 kali "write data discarded,
  use flow control to avoid losing data" untuk 50 MB `yes`).
- **Keputusan:** jendela kredit di Rust. Thread baca berhenti bila `sent - acked > WINDOW`; WebView mengirim total
  kumulatif dari callback `term.write`, digabung lewat microtask; perintah ack/tulis `async`. Ack per-chunk (selisih)
  ditolak: satu ack yang gagal di bawah beban (terlihat, galat `null`) mengecilkan jendela permanen sampai macet.
  `setTimeout` ditolak untuk penjadwalan ack: WebKit menahan timer ke ±1 Hz saat jendela tersembunyi, sehingga PTY
  ikut melambat.
- **Hasil dengan jendela 1 MB** (jendela terlihat): 50 MB `yes` (75 MB byte terminal karena `\n`→`\r\n`) tuntas
  17,6 s, 0 dibuang, jeda frame maks 42 ms, RTT ketikan p95 2 ms / maks 37 ms. Berkas berwarna 50 MB: 6,0 s
  (8,4 MB/s), 0 dibuang, jeda frame maks 65 ms, lag event loop maks 39 ms, tetapi RTT ketikan p50 110 / p95 255 ms
  (gema pane B antre di belakang hingga 1 MB data pane A).
- **Hasil dengan jendela 256 KB** (bawaan yang dipilih): berkas berwarna 6,2 s (8,1 MB/s, throughput sama), RTT
  ketikan p50 17 / p95 49 / maks 72 ms, 0 dibuang; `yes` 18 s, RTT p95 8 ms. Catatan jujur: kedua run 256 KB berakhir
  dengan jendela `hidden` (pemilik sedang memakai Mac), jadi lag event loop maks ±985 ms di run itu adalah efek
  penahanan timer, bukan beku. Ukur ulang di aplikasi nyata (T1.4) dengan jendela terlihat.
- Memori proses ±110 MB RSS setelah 50 MB.
- Kriteria S6 (UI tidak beku > 1 s, ketikan < 50 ms p95): **lulus dengan flow control dan jendela 256 KB**.
Dampak ke desain: ARCHITECTURE §3 (flow control), T1.3/T1.4. Jendela kredit dijadikan konstanta yang bisa disetel
(bukan pengaturan pengguna). Spike menjalankan jendela `always_on_top` hanya untuk pengukuran; produk tidak.
Temuan sampingan: React StrictMode menjalankan `useEffect` dua kali di dev; pembuatan PTY di `TerminalPane` harus
idempoten (id pane dari core, bukan dari efek).

## D-019 — T0.7: tinjauan docs setelah Phase 0 (2026-10-22)
Tinjauan semua docs terhadap D-013–D-018 (44 temuan). Keputusan yang diambil:
- **Peluncuran Claude memakai `--` sebelum tugas.** `--mcp-config <configs...>` dan `--allowedTools <tools...>` menerima
  banyak nilai (`claude --help` 2.1.303), jadi tanpa `--` tugas terbaca sebagai nama tool. Diuji: dengan `--`, tugas yang
  diawali `--` masuk utuh sebagai prompt. Codex juga memakai `--` agar tugas yang diawali `-` aman.
- **Codex `tool_timeout_sec=330`** (bawaan 60 detik menurut config-reference) agar `pane_spawn` bisa menunggu kartu
  persetujuan sampai 300 detik. Belum diuji (T3.2).
- **Env dari nol** ditambah `TMPDIR` dan `SSH_AUTH_SOCK` dari environment launchd agentdeck. Kunci API
  (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) tidak diteruskan. **Tertunda (keputusan pemilik):** apakah kunci API boleh
  diteruskan lewat daftar izin di Settings; sampai diputuskan, pengguna yang login lewat kunci API di env tidak didukung.
- **`PATH`** diambil dengan `$SHELL -l -i -c` di antara dua penanda, karena zsh non-interaktif tidak membaca `~/.zshrc`.
- **Ketikan orang** ditandai dari event keyboard/paste DOM; balasan otomatis xterm.js (`reply`) tidak dihitung.
- **Syarat siap** ditambah: kotak input agen kosong.
- **Pane tanpa WebView** dikonfirmasi oleh core ke buffer putar ulang, agar agen tidak tertahan.
- **Pengaman putaran** diatur ulang oleh Enter dari orang (bukan sembarang ketikan), sesuai ARCHITECTURE §5.5.
- **Asal permintaan:** `Origin` apa pun → 403, `Host` selain loopback → 403.
- **Satuan pesan:** 8 KB = 8192 byte isi + penanda ≤ 256 byte.
- Galat tambahan `UNKNOWN_PANE` dan `BAD_ARGS` untuk `pane_send`/`pane_read`; batas `pane_report` 6 per menit.
- OSC 7501 `blocked:kind=auth` dari Claude ("Login expired · Please run /login", terlihat saat uji di atas karena login
  CLI pemilik kedaluwarsa) ditampilkan sebagai "perlu login".
Sisa klaim yang belum terbukti beserta task pengujinya: ARCHITECTURE §12.

## D-020 — T1.4: S6 diukur ulang di aplikasi nyata (2026-10-22)
Versi sama dengan D-018. Sumber: `TerminalPane` + `pty.rs` sungguhan di `pnpm tauri dev` (harness ukur sementara, tidak
di-commit). RTT ketikan kini diukur sampai callback `term.write` (setelah xterm.js mengurai), lebih ketat daripada S6
(saat byte tiba di channel). Jendela dibuat terlihat di semua Space; run yang jendelanya sempat tersembunyi tidak dihitung.
- **Jendela kredit 256 KB, jendela terlihat:** `yes` 50 MB (75 MB byte terminal) 17,6 s (4,3 MB/s), jeda frame maks
  40 ms, RTT ketikan p95 3 / maks 7 ms. Berkas berwarna 50 MB 6,0–6,3 s (8,0–8,4 MB/s), jeda frame maks 30–49 ms, lag
  event loop maks ≤ 63 ms, **RTT ketikan p95 53–62 ms, maks 72–82 ms**: melewati target 50 ms. "write data discarded" 0
  di semua run.
- **Jendela kredit 128 KB (uji sementara, tidak di-commit):** berkas berwarna 6,3 s (8,0 MB/s, throughput praktis
  sama), RTT ketikan p95 19–24 / maks 30–37 ms, 0 dibuang.
- **Pane tanpa WebView tetap mengalir:** pane yang di-unmount (`pane_detach`) menuntaskan 50 MB `yes` dalam 17,8 s
  (sama dengan saat terpasang; yang membatasi PTY kernel, bukan xterm.js) dan isinya muncul lagi dari buffer putar ulang
  saat dipasang kembali. Pindah workspace di aplikasi = detach/attach, isi kembali utuh.
- **Jendela diminimalkan atau disembunyikan, pane terpasang:** tetap mengalir tanpa macet dan tanpa data dibuang
  (20 MB `yes` 8,1–8,4 s, ±3,7 MB/s). Tetapi bila jendela lama tersembunyi, berkas berwarna melambat sampai 1,1–1,4 MB/s:
  xterm.js memecah penguraian dengan `setTimeout`, yang ditahan WebKit ±1 Hz. Agen tidak tertahan (output agen jauh
  di bawah angka itu), tetapi banjir output di jendela tersembunyi menahan programnya.
- `vim` (alternate screen, jumlah `~` = baris − 2) dan `top` menggambar sesuai ukuran pane; `stty size` = ukuran xterm
  setelah resize; `vim`/`top` menggambar ulang ke ukuran baru. Diperiksa dari isi buffer xterm.js, bukan tangkapan layar.
**Diputuskan pemilik (2026-10-22):** `WINDOW` di `pty.rs` diturunkan ke 128 KB agar ketikan p95 < 50 ms saat pane lain
dibanjiri output berwarna (throughput tidak berubah). Tes kredit di `pty.rs` kini menghitung jumlah potongan dari
`WINDOW`, jadi tetap benar bila angkanya disetel lagi. Opsi untuk jendela tersembunyi bila nanti terasa: detach saat
`visibilitychange` → hidden dan pasang ulang saat terlihat (kehilangan scrollback di luar 1 MB putar ulang).

## D-021 — T1.7: peluncur, dialog folder native, status di header (2026-10-22)
- **`tauri-plugin-dialog` (2.x)** untuk tombol "Pilih…" di peluncur (UI-UX §4.1 meminta dialog folder native).
  WebView tidak punya pemilih folder yang mengembalikan jalur absolut; plugin resmi Tauri ini satu-satunya cara tanpa
  kode Objective-C sendiri. Menarik `rfd` dan `tauri-plugin-fs` ke pohon dependency. Izin yang diberikan hanya
  `dialog:allow-open` (capability `default`); tidak ada izin fs.
- **Tugas awal hanya lewat argv** (`<biner> -- <tugas>`, D-009/D-019), biner dicari dengan `find_in_path` di `PATH`
  shell login lalu dijalankan dengan jalur absolut. Tidak terpasang → galat `"<biner> tidak ditemukan di PATH."` +
  perintah pasang; peluncur tetap terbuka dan tidak ada pane rusak (PRD alur A).
- **Env (D-014) ditambah `LANG=en_US.UTF-8` bila agentdeck tidak punya `LANG`** (aplikasi yang dibuka dari Finder),
  sama seperti Terminal.app. Pane shell kini juga memakai env dari nol + `PATH` shell login (sebelumnya mewarisi
  sebagian env proses agentdeck).
- **Status:** pemindai OSC 7501 hanya berjalan di pane agen (shell tidak dipindai, jadi `claude` yang dijalankan manual
  di pane shell tidak menjawab query dan tidak melapor). Event `status`/`exited` lewat `Channel` per pane (bukan event
  global). Heuristik output (≤ 1 s → `working`, selain itu `unknown`) dihitung di WebView dari aliran byte pane yang
  terpasang; pane di workspace yang tidak aktif tidak memakai heuristik (status resmi tetap jalan). Agen tetap
  `starting` sampai laporan pertama, paling lama 5 s, lalu heuristik berlaku.
- **"Jalankan ulang"** menjalankan agen yang sama di folder yang sama **tanpa** tugas awal (mengulang tugas bisa
  mengulang pekerjaan). "Buka shell di sini" mengganti pane dengan shell di folder itu. Pane yang sudah keluar atau gagal
  ditutup tanpa dialog.
- Peluncur memakai `<dialog>` modal di tengah jendela, bukan di tengah pane tujuan: pane terkecil (40×8) lebih sempit
  daripada peluncur. 5 folder terakhir disimpan di `localStorage` WebView.
Diuji 2026-10-22 dengan Claude Code 2.1.306 dan Codex 0.163.0 di `pnpm tauri dev`: Claude `idle` setelah query
dijawab; dengan tugas awal `idle → working → blocked:kind=auth` ("perlu login", login pemilik kedaluwarsa); Codex
`idle`. Bagian `mcpServers` di `~/.claude.json` dan shasum `~/.codex/config.toml` sama sebelum/sesudah.

## D-022 — T3.1: kartu persetujuan spawn, notifikasi macOS (2026-10-22)
- **`tauri-plugin-notification` (2.7.0)** untuk notifikasi "<Agen> (pN) ingin membuka <agen>" saat jendela tidak fokus
  (UI-UX §4.2). Plugin resmi Tauri; alternatifnya memanggil `UNUserNotificationCenter` sendiri lewat Objective-C.
  Dikirim dari Rust saja (`approval_notify` di `lib.rs`), jadi capability `default` tidak diberi izin
  `notification:*` dan paket JS-nya tidak dipasang. Di macOS yang dikompilasi hanya `notify-rust` +
  `mac-notification-sys`; crate zbus/winrt di `Cargo.lock` hanya untuk Linux/Windows. Klik notifikasi mengaktifkan
  aplikasi (perilaku bawaan macOS), yang membawa jendela dan kartunya ke depan; plugin tidak memberi callback klik di
  desktop. Di `pnpm tauri dev` (tanpa bundle) macOS bisa menampilkannya atas nama aplikasi lain atau tidak sama sekali;
  diperiksa ulang di build bundel.
- **`approvals.rs`:** `Approvals::request(req, timeout, alive)` → `Approved | Denied | Timeout`. Batas waktu dari
  pemanggil (T3.2: Settings, 30–300 s). `alive` dicek tiap 250 ms; untuk `pane_spawn` cukup `resolve(token).is_some()`,
  karena token dicabut saat pane keluar atau ditutup (panes.rs tidak perlu kait baru). Future yang dibuang (panggilan
  MCP dibatalkan) menutup kartunya sebagai `denied`. Jawaban untuk kartu yang sudah tutup diabaikan
  (`approval_answer` → `false`).
- **Kontrak UI:** event `agentdeck://approval` `{ id, pane, requester_agent, agent, title, cwd, task, created_at,
  expires_at }` (ms epoch; `created_at` ditambahkan supaya bar hitung mundur benar setelah WebView dimuat ulang),
  `agentdeck://approval-closed` `{ id, outcome: "approved"|"denied"|"timeout" }`, perintah
  `approval_answer { id, allow } -> bool` dan `approvals_pending -> Approval[]`. `requester_agent`/`agent` adalah nama
  tampilan. Perintah `approval_demo { pane, secs? }` hanya ada di build debug.
- **⌘J** berputar: kartu tertunda tertua → kartu berikutnya → pita pengaman putaran → kembali. Kartu di workspace lain
  atau di bawah pane yang diperbesar: workspace-nya diaktifkan dan zoom dilepas dulu. Kartu sendiri tidak pernah
  memanggil `focus()`.
Diuji 2026-10-22 di `pnpm tauri dev` dengan harness sementara (33/33): kartu tidak mencuri fokus dari terminal,
hitung mundur, tugas 12 baris bergulir (maks 8), ⌘J, ⌘Enter, Esc, klik Izinkan, Kedaluwarsa 3 s, pane peminta
ditutup/keluar → `denied`.

## D-023 — T4.4/T4.5: katalog pengguna, path manual, info login, nama kunci API (2026-10-22)
- **Katalog pengguna** `agents.toml` di direktori data aplikasi, format sama dengan `catalogue.toml` dan validasi yang
  sama (flag bypass, `task_args`, id unik). Berkas yang tidak terbaca atau tidak sah diabaikan **utuh** dengan peringatan
  di stderr; aplikasi tetap jalan dengan katalog bawaan. Agen ber-id bawaan diabaikan kecuali `replace = true` (dicatat
  di stderr saat dimuat). Tambahan skema: `mcp_args` opsional (tanpa itu agen jalan tanpa injeksi dan tidak menerima
  `AGENTDECK_*`/token), `status = "none"` (tanpa pemindaian OSC 7501), `login_status`, `api_key_env`.
- **Path manual** di `agent-paths.json` (`{"<id>": "/jalur"}`), ditulis atomik (tmp + rename). Disimpan hanya bila
  absolut, berkas biasa, dan dapat dieksekusi; diperiksa lagi saat peluncuran. Path manual yang tidak sah lagi menjadi
  galat peluncuran, tidak jatuh diam-diam ke `PATH`.
- **Info login** hanya lewat perintah status resmi CLI dengan env dari nol (sama dengan env pane, jadi kunci API milik
  agentdeck tidak memengaruhi hasil) dan batas waktu 5 detik: `claude auth status --json` → `authMethod` +
  `subscriptionType` (email/org tidak diambil); `codex login status` → hanya frasa tetap `chatgpt`/`api_key`/`none`.
  Pada mode kunci API Codex mencetak potongan kunci yang disamarkan; baris itu dibuang, tidak disimpan atau dicatat.
  agentdeck sendiri tidak membuka berkas kredensial atau keychain. **Untuk ditinjau pemilik:** bila `codex login status`
  dianggap terlalu dekat dengan kredensial, hapus `login_status` dari entri Codex di `catalogue.toml` (hasilnya `null`).
- **Nama kunci API** (`api_key_env` di katalog: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) dideteksi dengan
  `printenv NAMA >/dev/null` di dalam `$SHELL -l -i -c`; nilai tidak pernah keluar dari shell. Nama divalidasi sebagai
  nama variabel saat katalog dimuat karena disisipkan ke skrip shell. Diperiksa ulang setiap `agents_info`.
- `Catalogue::builtin()` kini mengembalikan katalog yang berlaku (bawaan + pengguna + path manual) karena dipakai
  `panes.rs`; katalog murni lewat `Catalogue::embedded()`.

## D-024 — T3.2/T3.4/T4.1/T3.5: `pane_spawn`, audit, worktree, alur B dengan CLI asli (2026-10-23)
- **Urutan pemeriksaan** (`spawn.rs`): argumen (`BAD_ARGS`: field asing, `title` 1–60 karakter tanpa karakter kontrol,
  `task` 1–16384 byte tanpa NUL) → sakelar/`NESTED`/batas (`policy::check_spawn`) → agen di katalog (`UNKNOWN_AGENT`
  menyebut id yang ada) dan terpasang di `PATH` env anak (`AGENT_NOT_INSTALLED`) → `cwd` absolut tanpa `..`,
  dikanonikalisasi (symlink diikuti) dan harus di dalam folder pemanggil yang juga dikanonikalisasi (`CWD_OUTSIDE`;
  tidak ada/bukan folder = `BAD_ARGS`) → `worktree: true` butuh repo git (`BAD_ARGS`) → kartu. Agen tidak pernah
  ditanya orang untuk sesuatu yang pasti gagal.
- **Batas:** rate 5 per 10 menit dihitung **per aplikasi** dan baru terpakai saat kartu ditampilkan (ditolak dan habis
  waktu tetap terhitung, mencegah banjir kartu). Kartu yang masih menunggu dan anak yang sedang diluncurkan dihitung
  sebagai anak hidup untuk `CAP_CHILDREN`/`CAP_GLOBAL`, jadi kartu bertumpuk tidak bisa melampaui batas. Pemeriksaan
  diulang saat slot diambil.
- **Kode galat baru `SPAWN_FAILED`** (MCP-TOOLS): kartu disetujui tetapi peluncuran atau pembuatan worktree gagal.
  Worktree yang terlanjur dibuat dihapus lagi; audit `spawn_denied` alasan `launch failed`.
- **Peluncuran anak** lewat `panes::start` yang sama dengan pane buatan orang (tanpa shell, env dari nol, injeksi MCP,
  tugas setelah `--`), `parent` = pemanggil, workspace pemanggil, PTY 24×80 sampai UI memasangnya. Fungsi kembali
  setelah proses berjalan: `{pane, cwd, status: "starting"}` (+ `branch` bila worktree).
- **UI:** event baru `agentdeck://pane-spawned` (`Spawned`: pane, pty, workspace, parent, agent, agent_name, title,
  cwd, branch), `agentdeck://pane-event` `{pane, event}` (status/keluar anak; selalu sesudah `pane-spawned`, event
  proses lama yang sudah diganti tidak diteruskan), `agentdeck://spawn-refused` `{pane, code, limit}` untuk toast
  (SPAWN_OFF, CAP_*, RATE). Anak ditaruh di samping induk: split kanan bila muat 40×8, selain itu bawah. `pane_close`
  kini mengembalikan `KeptWorktree | null` (sebelumnya `null`), jadi kontrak lama tetap berlaku.
- **Worktree (T4.1):** `<akar repo utama>/.worktrees/<id>` di branch `agent/<id>` dari `HEAD` pemanggil; `<id>` = id
  pane anak, diberi akhiran `-2`, `-3` bila sudah ada (id pane mulai lagi dari p1 setiap kali aplikasi dibuka, worktree
  yang dibiarkan bertahan). `/.worktrees/` ditambahkan ke `.git/info/exclude` (lokal, tidak di-commit) agar repo induk
  tidak melihatnya. `git worktree add` jalan dengan `core.hooksPath=/dev/null` (permintaan agen tidak menjalankan hook
  repo) dan tanpa variabel `GIT_*` agentdeck. Saat orang menutup anak: berubah = ada berkas berubah/baru di
  `git status --porcelain` **atau** commit baru di branch sejak dibuat → dibiarkan + toast; selain itu
  `git worktree remove` (tanpa `--force`) dan `git branch -D`. Worktree anak yang masih terbuka saat aplikasi keluar
  tidak dibersihkan (batasan).
- **Audit (T3.4):** `spawn_requested` (kartu tampil, termasuk `worktree`), `spawn_approved` (`pane`), `spawn_denied`
  (`denied` | `timeout` | `launch failed`), `spawn_refused` (`code`, sebelum kartu), `loop_paused`, `pane_closed`
  (setiap pane yang ditutup orang). Tanpa token (`Store` juga menyaring deret 64-hex).
- **Batas waktu tool Claude Code:** uji asli pertama gagal: Claude Code 2.1.308 memutus `pane_spawn` tepat 60 s
  ("The operation timed out."), kartu tetap terbuka, dan persetujuan sesudahnya membuka Codex yang tidak diketahui
  Claude. Perbaikan: `"timeout":330000` di entri server `--mcp-config` (field per server Claude Code, per peluncuran,
  tidak menyentuh config global). Uji ulang: kartu disetujui setelah ~95 s, panggilan selesai di 107 s dan Claude
  melanjutkan. Codex `tool_timeout_sec=330` tidak bisa dibuktikan karena server OpenAI "at capacity".
- **Alur B dengan CLI asli (2026-10-23, Claude Code 2.1.308, Codex 0.163.0, `pnpm tauri dev`):** Claude di repo uji
  sementara menulis `clamp` + tes lalu memanggil `pane_spawn` codex; kartu tampil tanpa mencuri fokus; ⌘J ⌘Enter
  setelah 90 s → pane Codex terbuka di samping dengan lencana "anak p1" dan baris anak di sidebar. Codex menjawab
  "Selected model is at capacity" (model bawaan dan `-m gpt-5.6-luna` lewat path manual khusus uji), jadi Codex tidak
  meninjau. Sebagai gantinya Claude membuka Claude Code sebagai anak (p3): p3 meninjau, memanggil `pane_report`, p1
  menerima `RESULT status=done files=calc.py,test_calc.py tests=pass` sebagai satu tempelan, memeriksa sendiri, dan
  panel Pesan menampilkan `p3 → p1 terkirim`. Juga lulus dengan CLI asli: Esc di kartu → `DENIED` dan Claude meninjau
  sendiri; kartu tidak dijawab → `APPROVAL_TIMEOUT`; sakelar mati → `SPAWN_OFF` + toast; kartu ke-6 dalam 10 menit →
  `RATE`; dua anak `worktree: true` → `.worktrees/p4|p5`, tutup yang bersih menghapus worktree + branch, yang berubah
  dibiarkan + toast. Ulangi bagian Codex saat server OpenAI lega.

## D-025 — T4.3: pemulihan tata letak (2026-10-23)
- **Tampilan pane yang dipulihkan (keputusan pemilik):** tidak ada proses yang jalan sendiri, termasuk shell. Setiap pane
  kembali sebagai `exited` dengan label "belum dijalankan" dan tombol **Jalankan** / **Buka shell di sini**. Hubungan
  anak–induk ikut pulih (anak tanpa induk yang pulih menjadi pane biasa); zoom tidak dipulihkan; berkas rusak hanya
  dicatat di log dan aplikasi mulai kosong (tanpa toast).
- **Siapa menulis:** core Rust (`layout.rs`), bukan `localStorage`, karena isi berkas menentukan apa yang diluncurkan
  saat Jalankan. WebView mengirim potret setiap perubahan (jeda 400 ms) lewat `layout_save`; core memvalidasinya dan
  mengisi worktree dari registri, tidak pernah dari WebView. Tulis atomik seperti `settings.json`.
- **Id:** id pane `pN` dan nomor workspace lama dipakai lagi; penghitung berlanjut di atas angka tertinggi, jadi id
  tetap tidak dipakai dua kali dalam satu run. Berkas dibaca di `setup` dan pane didaftarkan sebelum WebView meminta,
  jadi tidak ada balapan dengan pane baru. Id dan nomor dibatasi ≤ 1.000.000 (penghitung tidak meluap).
- **Validasi (`Layout::validated`, tes unit):** versi 1, ≤ 16 workspace, ≤ 64 pane, teks ≤ 200 karakter tanpa karakter
  kontrol, path absolut, rasio split di (0, 1), kedalaman pohon ≤ 32, setiap pane tepat satu daun di workspace-nya.
  `focused`, `parent`, dan `active` yang menunjuk ke yang tidak ada diperbaiki, bukan ditolak.
- **Worktree:** dipasang lagi hanya bila masih persis buatan `worktree::create` (`still_ours`): path dan repo kanonik
  sama dengan yang tersimpan, `<repo>/.worktrees/<nama>` adalah puncak checkout-nya sendiri, dan branch-nya
  `agent/<nama>` yang sedang di-checkout. Ini mencegah berkas yang diubah tangan membuat penutupan pane menghapus
  branch lain. Batasan: `base` tetap dipercaya dari berkas.
- **Batasan:** setelah WebView dimuat ulang (atau `layout_restore` gagal), run itu tidak menyimpan tata letak, supaya
  berkas tidak tertimpa kosong. Perubahan dalam 400 ms terakhir sebelum keluar hilang.
- **Uji di aplikasi (2026-10-23, `pnpm tauri dev`, pane shell):** 2 shell split kanan → keluar → dibuka lagi: workspace,
  split, dan fokus pulih, kedua pane "belum dijalankan"; Jalankan menghidupkan shell di foldernya; pane baru dapat p3;
  menutup pane pulihan yang belum dijalankan memperbarui berkas. Dengan Claude Code 2.1.309: anak `pane_spawn` dengan
  worktree pulih sebagai "anak p1" dan "belum dijalankan"; menutupnya menghapus worktree `agent/p2` yang bersih.

## D-026 — T5.4: ketentuan provider diperiksa ulang (2026-10-23)
Dibaca 2026-10-23: code.claude.com/docs/en/legal-and-compliance, anthropic.com/legal/consumer-terms, artikel bantuan
Claude tentang login dan Agent SDK, learn.chatgpt.com/docs/auth, repo `openai/codex` (Apache-2.0). Terms of Use dan
halaman merek OpenAI menjawab 403 (hanya terbaca lewat cuplikan pencarian, belum terverifikasi).
- **Pemakaian pribadi: boleh.** Binary tidak dimodifikasi, login milik pengguna, kredensial tidak disentuh (D-006
  tetap berlaku).
- **Baru sejak D-006 (Anthropic):** menjalankan Claude Code "in your products or services" (contohnya sandbox ter-host
  atau infrastruktur agen) memerlukan Commercial Terms. Belum jelas apakah aplikasi lokal yang meluncurkan CLI milik
  pengguna termasuk. Batas Pro/Max disebut untuk pemakaian biasa dan perorangan, dan Consumer Terms melarang akses
  otomatis lewat bot atau skrip kecuali dengan kunci API, jadi orkestrasi agen-ke-agen di langganan adalah area abu-abu.
  Anthropic sudah menindak harness pihak ketiga (yang memakai OAuth langsung, tidak seperti agentdeck).
- **Merek:** nama dan logo Claude/Claude Code/Anthropic tidak boleh dipakai di nama atau logo produk; teks biasa
  "menjalankan Claude Code" boleh. Cuplikan pedoman merek OpenAI melarang nama model dan merek GPT di nama aplikasi.
- **OpenAI:** tidak ditemukan larangan eksplisit untuk pembungkus lokal CLI yang tidak dimodifikasi; Terms of Use
  (cuplikan) melarang mengekstrak output "automatically or programmatically". `pane_read` bisa ditafsirkan begitu,
  jadi status ini belum jelas.
- **Usulan (belum diputuskan):** rilis publik ditunda sampai Anthropic menjawab soal Commercial Terms. Nama tetap tanpa
  Claude/Codex/GPT/ChatGPT. README dan layar pertama menyebut bahwa agentdeck tidak berafiliasi dengan Anthropic atau
  OpenAI, bahwa pengguna memasang dan login sendiri, dan bahwa pemakaian paralel menghabiskan kuota paket pengguna.
  Tidak membundel binary agen, tidak ada mode ter-host, tidak ada jalan tanpa orang (pengaman putaran dan kartu
  persetujuan tetap). Terms of Use OpenAI dibuka manual di browser sebelum rilis.
- **Keputusan pemilik (2026-10-23):** distribusi pribadi dulu; rilis publik ditunda sesuai usulan di atas. Nama tetap
  `agentdeck` untuk sekarang. Akibatnya untuk Phase 5: tanpa notarisasi dan tanpa updater (build lokal
  `pnpm tauri build`, pembaruan lewat `git pull` + build ulang); bundle id `local.agentdeck.desktop` tetap, sehingga
  `settings.json`, `layout.json`, dan log yang sudah ada tetap terpakai. Ikon sendiri tanpa logo atau warna khas
  provider. T5.2 (notarisasi) dan T5.3 (updater + halaman rilis) dibuka lagi bila rilis publik diputuskan.

## D-027 — TERM-03: cari teks di pane (2026-10-23)
- Celah P0 yang terlewat sejak T1.4: `⌘F` tercantum di UI-UX §7 tetapi tidak pernah dibuat. Ditemukan saat audit PRD.
- `@xterm/addon-search` 0.16.0 (addon resmi xterm.js, pasangan xterm 6.0; tanpa dependency lain). Sorotan semua hasil
  dan penghitung `N/M` memakai dekorasi addon, yang di xterm 6 masih *proposed API*, sehingga Terminal dibuat dengan
  `allowProposedApi: true`. Opsi ini hanya membuka API xterm untuk kode kita sendiri; tidak ada dampak keamanan.
- Kotak cari di pojok kanan atas pane, di luar elemen host terminal, supaya ketikan di sana tidak dihitung sebagai
  orang mengetik oleh antrean pesan (§5.2). Enter/⇧Enter berikutnya/sebelumnya, Esc menutup dan menghapus sorotan.
  Warna dari token (`--term-accent` untuk hasil, `--warn` untuk hasil aktif). Pane tanpa terminal (dipulihkan, gagal)
  tidak membuka kotak cari.
- Uji di aplikasi (`pnpm tauri dev`, pane shell): `hello` → 4 hasil bergaris, "1/4"; Enter → "2/4"; Esc menutup dan
  fokus kembali ke terminal.

## D-028 — Alasan pesan menunggu di chip (2026-10-23)
- Ide dari riset MXterm Pro (RESEARCH §2, penahanan kiriman dengan alasan). Menuntaskan ARCHITECTURE §5.2 butir 4
  (chip "kosongkan input") yang belum pernah dibuat.
- `WaitReason` yang sudah dihitung `delivery::ready` tidak lagi dibuang: `Registry` menyimpan alasan terakhir per pane
  tujuan, `MessageRow.wait` membawa kodenya (`busy`, `not_quiet`, `typing`, `input_not_empty`, `input_unknown`,
  `paused`) untuk baris `queued`. Event `messages-changed` dikirim hanya saat alasan berubah, bukan setiap tick.
  Alasan dihapus saat pesan terkirim, dibuang, atau antrean kosong.
- UI: chip header `1 pesan · kosongkan input`, baris panel Pesan menambah alasan yang sama. Kode yang tidak dikenal UI
  (core lebih baru) tidak ditampilkan mentah.
- Uji di aplikasi (`cargo run` + vite, dua pane Claude Code 2.1.309, saling kirim aktif): draf di input p2, p1 memanggil
  `pane_send` → chip p2 "1 pesan · kosongkan input"; draf dihapus → pesan terkirim, chip hilang.

## D-029 — Kuota dan konteks Claude dari statusline resmi (2026-10-23)
- Ide dari riset MXterm Pro (RESEARCH §2): statusline Claude Code memberi `rate_limits.five_hour/seven_day`
  (`used_percentage`, `resets_at`), `context_window.used_percentage`, dan `session_id` lewat stdin perintah
  `statusLine`. Fitur resmi; tidak ada kredensial yang dibaca (D-006 tetap). Terlihat di Claude Code 2.1.309 setelah
  giliran pertama; sebelum itu `rate_limits` tidak ada dan persen konteks `null`.
- Injeksi per peluncuran (D-005): `mcp_args` Claude menambah `--settings
  {"statusLine":{"type":"command","command":"cat > \"$AGENTDECK_STATUSLINE\""}}`; env `AGENTDECK_STATUSLINE` berisi
  `<data>/statusline/<pane>.json`. MXterm memasangnya di `~/.claude/settings.json`; kita tidak (aturan 3).
  Konsekuensi: di pane agentdeck statusline milik pengguna diganti (bukan dihapus dari berkasnya), baris statusline
  Claude kosong.
- `usage.rs`: berkas ditulis proses agen, jadi tidak dipercaya. Dibaca ≤ 64 KB, hanya tiga kolom, persen dijepit
  0–100, `session_id` hanya bila berbentuk UUID; `pane_usage` tidak mengirimnya (dipakai D-030).
  Direktori dikosongkan saat start, berkas pane dihapus saat pane dijalankan (ulang).
- UI memanggil `pane_usage` tiap 10 s untuk pane Claude yang berjalan di workspace yang tampil; badge header
  `5j 31% · ktx 5%`, tooltip kuota 7 hari + waktu reset, amber mulai 80%. Codex tidak: kuotanya hanya dari kredensial.
- Belum: menolak `pane_spawn` di atas ambang kuota (MXterm 85%).
- Uji: `cargo test` (`usage::tests` dengan JSON 2.1.309 nyata), argumen dari `catalogue.toml` dijalankan ke Claude
  2.1.309 asli dengan path berspasi → berkas terisi kuota, konteks, `session_id`. Di aplikasi (2026-10-23): setelah
  giliran pertama header p1 menampilkan `5j 41% · ktx 6%`; baris statusline Claude sendiri kosong.

## D-030 — Pane Claude yang dipulihkan melanjutkan percakapannya (2026-10-23)
- Ide dari BridgeMind v0.3.4 (RESEARCH §3). Melengkapi D-025: tata letak sudah kembali, kini percakapan juga.
- Id sesi diambil dari statusline (D-029), bukan dari berkas sesi Claude. `layout.json` menyimpan `session` per pane,
  diisi dari registri saat simpan (seperti `worktree`): bacaan statusline run ini, atau sesi pulihan yang belum
  dipakai. Saat dimuat, nilai yang bukan UUID dibuang (nilainya menjadi satu elemen argv).
- Katalog: `resume_args = ["--resume", "{session}"]` untuk Claude; `{session}` hanya boleh di `resume_args`, sekali,
  dan `resume_args` ikut cek flag bypass. Codex tidak punya `resume_args`: id sesinya hanya terlihat di berkas
  sesinya sendiri, yang tidak dibaca.
- Dipakai saat pane yang sama dijalankan ulang dengan agen yang sama dan tanpa tugas baru, termasuk pane yang keluar
  di run ini. Sekali pakai: bila resume gagal (sesi hilang), run berikutnya mulai baru. Tugas baru atau agen lain
  membuang sesi lama.
- Uji: unit (katalog, registri, layout). Claude 2.1.309 asli dengan argumen katalog + `--resume <id>` di folder yang
  sama: riwayat lama tampil dan statusline melaporkan id yang sama, jadi simpan berikutnya tetap menyimpannya. Belum
  diklik di aplikasi, dan uji di aplikasi menemukan bug: WebView menyimpan `layout.json` hanya saat tata letak
  berubah, padahal id sesi baru ada setelah giliran pertama, jadi sesi tidak pernah tersimpan. Perbaikan: core
  menyimpan ulang tata letak terakhir di `RunEvent::Exit`, sebelum pane dihentikan (`layout::Store::save_last`).
  Batasan: saat crash, sesi yang muncul sejak perubahan tata letak terakhir hilang.
- Uji di aplikasi setelah perbaikan: p1 diberi kata kode, ⌘Q → `layout.json` menyimpan sesi p1; buka lagi, Jalankan
  p1 → proses berjalan dengan `--resume`, riwayat lama tampil, Claude menjawab kata kodenya. Tombol pane pulihan belum
  memberi tahu bahwa percakapan dilanjutkan.

## D-031 — Blok perintah OSC 133 di pane shell (2026-10-23)
- Ide dari BridgeMind (RESEARCH §3): blok perintah ala Warp. Hanya pane shell zsh; pane agen tidak berubah.
- Integrasi per peluncuran (sejalan D-005, tidak ada dotfile/config global yang ditulis): `launch::shell` menulis
  `src-tauri/shell/zsh/.zshenv` (di-embed) ke `$TMPDIR/agentdeck-zsh-<uid>/.zshenv` lalu mengisi env `ZDOTDIR`. Berkas
  itu menghapus `ZDOTDIR`, men-source `~/.zshenv` pengguna, sehingga `.zprofile`/`.zshrc`/`.zlogin` dibaca dari
  tempat biasanya (juga `ZDOTDIR` milik pengguna yang diset di `.zshenv`-nya). Pola sama dengan Ghostty/kitty.
- Hook (`add-zsh-hook`, dipasang sebelum `.zshrc`): `precmd` → `D;<exit>` (hanya bila ada perintah yang jalan), `A`,
  dan `B` di akhir `PS1`; `preexec` → `C`. `$?` tetap benar walau precmd pengguna lain jalan lebih dulu (dites).
- Keamanan: zsh men-source berkas itu, jadi folder dibuat 0700 dan ditolak bila bukan milik uid kita, bukan folder
  biasa (symlink), atau bisa ditulis grup/lainnya. Tanpa `TMPDIR` folder ada di `/tmp`. Ditolak → shell polos tanpa
  integrasi. Berkas ditulis ulang tiap peluncuran lewat tulis-lalu-rename.
- UI (`src/pane/commands.ts`, `TerminalPane.tsx`): `registerOscHandler(133)`; marker di baris A, prompt tanpa perintah
  dibuang. Saat D, dekorasi batang 3 px di padding kiri `.xterm` (8 px yang sudah ada, jadi lebar kolom tidak berubah):
  `--ok` bila exit 0, `--danger` bila gagal, `--muted` bila kode tidak dikirim; tooltip `Kode exit N`.
- Pintasan: **⌘↑ / ⌘↓** perintah sebelumnya/berikutnya (sama dengan Terminal.app). Tidak bentrok dengan
  `src/layout/keys.ts`: ⌘⌥+panah fokus pane, ⌘ tanpa ⌥ + panah tidak dipetakan, dan xterm.js mengabaikan ⌘+panah.
  Dipasang di `TerminalPane.tsx` (`attachCustomKeyEventHandler`), bukan `keys.ts`; tanpa marker tidak terjadi apa-apa,
  sama seperti sebelumnya. Usulan: pindahkan ke `keys.ts` + UI-UX §7 saat berkas itu bebas disentuh.
- Batasan: bash/fish tanpa integrasi. `.zshrc` yang menimpa `precmd_functions`/`preexec_functions` utuh (bukan
  menambah) mematikan hook; tema yang menyusun ulang `PS1` di precmd-nya sendiri menghapus `B` (tidak dipakai UI).
  Prompt instan p10k tercetak sebelum precmd pertama, jadi prompt pertama tanpa A. Perintah di dalam `ssh`/`tmux`
  tidak ditandai. Penanda ikut hilang bersama scrollback.
- Uji: `cargo test` (`launch::tests`: hanya zsh mendapat `ZDOTDIR`; folder 0777/symlink ditolak; zsh asli dengan
  `$HOME` palsu: `.zshenv`/`.zshrc` pengguna tetap terbaca, `ZDOTDIR` hilang, urutan `A B C D;1 … A B` dengan precmd
  pengguna yang mengembalikan 7 lebih dulu). vitest `src/pane/commands.test.ts` (parser, prompt kosong dibuang,
  loncat). zsh dengan dotfile asli pemilik di PTY (`script`): `false` → `D;1`, `echo` → `D;0`.
- Uji di aplikasi (2026-10-23, Vite di port 1440 karena 1420 dipakai checkout utama): aplikasi jalan dengan env
  bersih, tetapi `screencapture -l` gagal ("could not create image from window") dan ketikan ⌘Q tidak sampai, jadi
  izin Screen Recording/Accessibility untuk Claude Code 2.1.308 belum ada; pane shell tidak diklik di aplikasi. Sebagai
  gantinya `TerminalPane` asli dirender di browser lewat Vite yang sama dengan `mockIPC`, diberi byte rekaman zsh
  asli: batang merah di `false` dan `ls /nope` (tooltip `Kode exit 1`), hijau di `echo`, prompt kosong tanpa batang,
  ⌘↑ loncat sampai perintah pertama, ⌘↓ ke berikutnya lalu ke bawah, tidak ada byte yang ditulis ke PTY. Belum
  terlihat: penanda gutter dan ⌘↑/⌘↓ di jendela agentdeck (belum ada screenshot jendela), dan pane shell yang dibuka
  dari aplikasi (jalur `panes.rs` → `launch::shell` → PTY hanya tercakup tes unit). Aplikasi port 1440 ditutup dengan
  SIGTERM ke prosesnya sendiri karena ⌘Q tidak sampai.

## D-032 — Pustaka prompt dan preset sesi (2026-10-23)
- Ide dari BridgeMind (RESEARCH §3: pustaka Prompts, preset Solo/Pair). Hanya konsep; teks dan nama UI milik kita.
- **Pustaka prompt** disimpan di `settings.json` sebagai `prompts: [{name, text}]` lewat `settings_set` yang sudah ada
  (tanpa command baru); patch mengganti seluruh daftar. Validasi di Rust (`settings::validate_prompts`): paling banyak
  100 prompt, nama 1–80 karakter satu baris dan unik tanpa beda huruf besar/kecil, teks 1–10.000 karakter, karakter
  kontrol hanya tab/LF/CR. ESC ditolak karena teks berisi `ESC[201~` bisa mengakhiri bracketed paste lebih awal lalu
  menjalankan sisanya. Kunci asing (mis. `args`) ditolak. UI mencerminkan aturan yang sama hanya untuk menolak di tempat.
- **Menempel**: palet `⌘P` (bebas di UI-UX §7; `⌘K` dihindari karena di terminal macOS berarti hapus layar) mengirim
  event `paste` sintetis ke textarea xterm.js pane yang fokus. Jalurnya sama persis dengan `⌘V`: xterm.js membungkus
  dengan bracketed paste bila program memintanya, `onData` → `pane_write`, dan jam ketikan pane (`inputClock`, dipicu
  event `paste`) menandainya `typed`, jadi antrean pesan menunda kiriman agen seperti saat orang mengetik. Tidak ada
  perubahan di `src/pane`. Baris baru di ujung dibuang dan Enter tidak ditambahkan. Batasan: di program tanpa bracketed
  paste, baris baru di tengah teks berlaku sebagai Enter, sama seperti `⌘V`.
- **Preset sesi** adalah konstanta di `src/prompts/prompts.ts`: id agen dari katalog + arah split, tanpa argumen, flag,
  atau env (aturan 4); peluncuran lewat `pane_create` yang sama dengan peluncur, jadi argumen hanya dari katalog. Semua
  atau tidak sama sekali: bila satu pane gagal, pane yang sudah jalan ditutup dan workspace dibuang.
- Belum: preset buatan pengguna (menunggu permintaan; bentuknya sudah bisa disimpan seperti prompt), variabel di prompt,
  kirim prompt ke beberapa pane sekaligus.
- Uji: `cargo test` (`settings::tests::prompt_library_is_validated`), vitest `src/prompts/prompts.test.tsx` (cari,
  simpan, ⌘P, susunan preset, tampilan palet). Di aplikasi (2026-10-23, Vite di port 1430 karena 1420 dipakai checkout
  utama): prompt dua baris disimpan lewat palet dan masuk `settings.json`; ditempel ke pane zsh → tampil sebagai
  bracketed paste, tidak dijalankan; ditempel ke Claude Code 2.1.309 → dua baris di kotak input, tidak terkirim;
  preset Pair → workspace baru, Claude Code kiri (fokus), Codex kanan, keduanya siap. Penanda `typed` tidak diamati
  langsung; ia mengikuti jam ketikan pane yang dipicu event `paste` yang sama dengan `⌘V`.

## D-033 — Pengaman kuota `pane_spawn` dan klaim berkas `pane_claim` (2026-10-23)
- Ide dari riset MXterm Pro (RESEARCH §2), konsep saja; tidak ada teks atau kode yang disalin.
- **Pengaman kuota.** `pane_spawn` ditolak dengan `QUOTA_LOW` bila kuota 5 jam Claude pemanggil (`usage::read`,
  D-029) ≥ ambang. Bawaan 85% (seperti MXterm), rentang aman 50–100%. Diperiksa setelah sakelar, nesting, dan batas,
  sebelum katalog dan kartu; hanya pada pemeriksaan pertama, agar penolakan tidak pernah memegang slot rate/anak.
  Pane tanpa bacaan (Codex, atau Claude sebelum giliran pertama) tidak ditolak. Penolakan diaudit (`spawn_refused`)
  dan dikirim ke UI sebagai `Notice::Refused { code: "QUOTA_LOW", limit: ambang }`.
- Berlaku apa pun agen anaknya, sesuai permintaan: pemanggil tetap memakai kuotanya untuk mengoordinasi anak. Bila
  nanti spawn Codex dari Claude yang hampir habis ingin diizinkan, cukup ubah `Spawner::policy`.
- Ambang disimpan di `Limits` (`quota_guard`, `set_quota_guard`), bukan sebagai varian `Limit`: `settings.rs`
  mencocokkan semua varian `Limit` dan sedang diubah sesi lain. Akibatnya ambang belum bisa diatur dari Settings dan
  UI belum punya toast untuk `QUOTA_LOW` (`refusedText` mengembalikan `null`, agen tetap membaca galatnya).
- **Klaim berkas.** Tool baru `pane_claim {paths}` dipilih daripada argumen `files` di `pane_report`: laporan dikirim
  sekali di akhir, klaim dibutuhkan di awal kerja. Bentuknya sekecil mungkin: satu argumen, panggilan mengganti klaim
  sebelumnya, `[]` melepas. Tanpa argumen perintah, flag, env, atau pane lain (AGENTS.md aturan 4).
- Path relatif terhadap `cwd` pane, ≤ 10, tetap di dalam `cwd` setelah symlink diurai (`PATH_OUTSIDE` bila tidak).
  Disimpan absolut supaya pane dengan `cwd` berbeda (anak di subfolder) bisa dibandingkan; anak `worktree: true`
  otomatis tidak bertabrakan dengan pohon induk. Tumpang tindih per komponen dengan klaim pane hidup lain di
  workspace yang sama → `CLAIMED` yang menyebut pane pemiliknya; klaim pemanggil tidak berubah.
- Dilepas saat proses keluar (`PaneEvent::Exited`) dan saat pane ditutup; `pane_list` menampilkan `claims` tiap pane.
- Hanya penanda kerja sama: agentdeck tidak bisa dan tidak mencoba memblokir penulisan berkas. Peer di workspace yang
  sama melihat klaim lewat `pane_list` meski tidak boleh saling kirim.
- Uji: unit (`policy` ambang dan rentang, `spawn` QUOTA_LOW sebelum kartu tanpa memakai slot, `mcp::claim` path
  keluar `cwd`/symlink/bentuk, `panes` tumpang tindih dan pelepasan, `mcp::tools` alur tool), integrasi HTTP
  (`tests/mcp_server.rs`: `CLAIMED` menyebut pemilik, `pane_list` memuat klaim). Belum diuji dengan agen asli.

## D-034 — Peluncur multi-terminal ala MXterm; flag bypass untuk pane yang dibuka orang (2026-10-23)
- Permintaan pemilik: di dialog workspace baru pilih folder, jumlah terminal sekaligus (berapa Claude, berapa Codex),
  sakelar "bypass izin untuk semua terminal", dan sakelar "worktree baru per terminal".
- Acuan konsep: peluncur MXterm Pro (jumlah terminal per agen, satu sakelar bypass untuk semua, worktree per terminal).
  Flag bypass dari dokumentasi CLI: Claude `--dangerously-skip-permissions`, Codex
  `--dangerously-bypass-approvals-and-sandbox`.
- Jumlah per agen dan worktree per terminal tidak bertentangan dengan apa pun: memperluas preset D-032 dan worktree T4.1.
- **Bypass bertentangan dengan ARCHITECTURE §4 "Yang tidak pernah dilakukan"** (`--dangerously-*` dilarang total),
  sementara PRD prinsip 2 hanya melarangnya untuk agen yang dibuka agen. Risiko utama (PRD §10): rantai prompt
  injection — pesan `pane_send` dari agen lain dijalankan pane ber-bypass tanpa ada yang ditanya.
- Pilihan untuk pemilik: (a) tidak ada bypass; (b) mode semi-otomatis tanpa `--dangerously-*` (Claude
  `--permission-mode acceptEdits`, Codex `--full-auto`: sandbox tetap, edit di folder kerja tanpa tanya); (c) bypass
  penuh hanya untuk pane yang dibuka orang, mati bawaan, dengan pagar: konfirmasi saat dinyalakan, badge merah di header,
  pane ber-bypass tidak menerima `pane_send` dari peer, anak dari `pane_spawn` tetap tanpa bypass.
- **Diputuskan pemilik 2026-10-23: (c).** Bypass penuh hanya untuk pane yang dibuka orang lewat peluncur, mati bawaan,
  dengan pagar di atas. Agen yang dibuka agen (`pane_spawn`) tetap tidak pernah mendapat bypass (AGENTS.md aturan 4,
  PRD prinsip 2). ARCHITECTURE §4 "Yang tidak pernah dilakukan" disesuaikan: `--dangerously-*` hanya dari sakelar
  orang di peluncur, tidak pernah dari agen, tool MCP, atau katalog pengguna.
- **Implementasi (2026-10-23).** Flag dari tabel `BYPASS` di `catalogue.rs` (`Agent::bypass_arg`); agen dari
  `agents.toml`, termasuk yang `replace = true`, ditandai `from_user` dan tidak pernah mendapatnya; TOML tetap menolak
  flag bypass saat parse. `launch::agent(.., bypass)` menaruh flag sebagai elemen sendiri tepat setelah program.
  `Spec { worktree, bypass }` (`serde(default)`), diperiksa `check_spec`: hanya pane agen baru tanpa induk; pane yang
  dijalankan ulang dan pane dari `layout.json` selalu tanpa bypass (layout tidak menyimpannya), jadi bypass dan resume
  (D-030) tidak pernah bersamaan. `policy::can_send` menolak `TARGET_BYPASS` kecuali pengirimnya anak pane itu;
  `pane_read` tidak terpengaruh. `pane_list` memuat `bypass`. Peluncuran ber-bypass diaudit (`pane_bypass`).
  Worktree: `panes::start_in_worktree` dipakai bersama `pane_spawn` (kode worktree spawn dipindah ke sini),
  `pane_create` memakai `worktree::repo_root` dan menolak folder bukan repo git. `Started` membawa `cwd`/`branch`/
  `bypass`, sehingga UI menyimpan dan menjalankan ulang pane di worktree-nya.
- UI: peluncur workspace (stepper per program, pintasan, dua sakelar, `BypassDialog`), `openMany` menggantikan
  `openPreset` dengan rollback yang sama, `gridTree` membagi rata (menggantikan `presetTree` yang 1/2, 1/4, 1/4).
- Batasan: Claude menanyakan "trust this folder" di setiap worktree baru dan menampilkan layar persetujuan bypass-nya
  sendiri; jawabannya milik orang dan disimpan Claude sendiri (agentdeck tidak menulis config, aturan 3). Ukuran
  minimum pane tidak diperiksa sebelum 8 pane dibuka.
- Uji: unit Rust (katalog, launch argv, policy, registri: 200 lulus), vitest (peluncur, badge, `gridTree`, `expand`:
  120 lulus), `tests/bypass_cli.rs` (manual, `--ignored`) dengan Claude 2.1.308 dan Codex 0.163.0 asli lewat
  `start_in_worktree`: proses anak berjalan dengan flag bypass masing-masing, tiap pane di `.worktrees/pN` sendiri,
  saat ditutup worktree dan branch yang tidak berubah terhapus, tidak ada proses tersisa. Peluncur belum diklik di
  aplikasi: layar terkunci saat diuji (tidak ada app frontmost, screenshot jendela gagal).

## D-035 — Deteksi status dari layar dengan aturan herdr untuk agen tanpa OSC 7501 (2026-10-23)
- **Masalah.** Agen katalog pengguna (`status = "none"`, mis. Gemini, OpenCode, Cursor, Qwen) tidak melapor OSC 7501.
  Statusnya hanya heuristik output di UI, jadi antrean pesan memperlakukannya `unknown` (menunggu layar sepi) dan
  `needs_you` tidak pernah muncul saat agen menunggu izin.
- **Sumber.** [herdr](https://github.com/herdrdev/herdr), Apache-2.0, Rust, commit acuan `6b52662` (2026-10-23);
  pemberitahuan pihak ketiga MXterm Pro juga mengkreditnya (RESEARCH §2). Diambil langsung dari repo herdr (aturan 8).
  herdr bukan library (tidak ada `lib.rs`; crate `herdr` 0.1.0 di crates.io basi), jadi kode dan data disalin.
- **Lisensi (Apache-2.0 §4).** Sertakan `LICENSE` herdr di bundel aplikasi dan entri `THIRD_PARTY_NOTICES`; setiap
  berkas `.rs`/`.toml` hasil port diberi kepala: asal (repo + commit), lisensi, dan "diubah oleh agentdeck". herdr tidak
  punya berkas NOTICE dan tidak menyebut pemegang hak cipta; kredit tetap ditulis.
- **Yang diambil.**
  - Data aturan: `src/detect/manifests/*.toml` (22 agen, ±50 KB), ditanam dengan `include_str!` di
    `src-tauri/herdr/manifests/`. **Tanpa** pembaruan jarak jauh herdr (`herdr.dev/agent-detection`), berkas override,
    atau cache: agentdeck lokal saja (PRD prinsip 4). Aturan diperbarui dengan menyalin ulang dari commit herdr yang
    baru lalu menjalankan tesnya (aturan herdr berubah 1–3 kali/minggu mengikuti UI CLI).
  - Mesin (`manifest.rs` herdr, ±550 baris skema/kompilasi/pencocokan/region + ±200 baris validasi) ke modul baru
    `src-tauri/src/detect.rs`: skema aturan (`state`, `priority`, `region`, `contains`/`regex`/`line_regex`,
    `all`/`any`/`not`, `visible_*`, `skip_state_update`), prioritas tertinggi menang (seri: aturan lebih awal), batas
    validasi herdr (128 aturan, kedalaman 8, 512 karakter per pola), region sesuai engine v3.
  - Debounce (`agent_detection.rs` herdr, ±100 baris): poll 300 ms (100 ms saat menunggu konfirmasi idle), working→idle
    biasa baru dipublikasikan setelah 3 bacaan atau 700 ms, `blocked`/idle terlihat langsung, masa tenggang 3 s setelah
    agen mulai, blocker stabil diulang tiap 800 ms, layar tak berubah dilewati.
- **Rancangan di agentdeck.**
  - `screen.rs`: teks deteksi sesuai aturan snapshot herdr (baris fisik sebanyak tinggi layar, berakhir di baris non-kosong
    terakhir atau kursor, `trim_end`, tanpa baris kosong di ujung); judul OSC 0/2 dan progres OSC 9 ditangkap lewat
    `vt100::Callbacks` (`set_window_title`, `unhandled_osc`), dibersihkan (karakter kontrol dibuang, maks. 256 karakter).
  - Katalog: `status = "screen"` + `detect = "<id herdr>"` untuk agen pengguna; id divalidasi terhadap manifest tertanam.
    Claude dan Codex bawaan **tetap OSC 7501** (sinyal resmi, lebih andal); deteksi layar tidak menimpanya.
  - Satu thread deteksi untuk semua pane `screen` (poll 300 ms) menghasilkan `PaneEvent::Status` yang sama dengan jalur
    OSC, jadi UI, antrean pesan, dan `pane_list` tidak berubah. Pemetaan: `working` → `working`, `blocked` → `needs_you`
    (label "butuh kamu"), `idle` → `idle`, `unknown`/`skip_state_update` → status sebelumnya. Tidak pernah mengirim input.
  - Dependensi baru: `regex = "1"` (crate resmi rust-lang, dipakai mesin herdr); `toml`/`serde` sudah ada.
- **Tes.** Port 24 tes aturan herdr (`src/detect/manifest/tests.rs`), tes snapshot layar dengan teks layar buatan per agen
  (blocked/working/idle), tes debounce dengan jam palsu, tes katalog (`detect` tak dikenal ditolak). Di aplikasi: agen
  pengguna asli yang terpasang (Gemini/OpenCode bila ada), selain itu fake-agent yang menggambar layar contoh.
- **Ukuran.** ±1000 baris Rust termasuk tes, satu sesi kerja.
- **Diputuskan pemilik 2026-10-23:** `regex` ditambah; Claude/Codex tetap hanya OSC 7501 (cadangan layar menunggu
  kasus nyata; tinggal `status = "screen"` + `detect = "claude"` di katalog pengguna).
- **Implementasi.** `herdr/manifests/` (22 berkas, kepala asal-usul), `herdr/LICENSE`, `THIRD_PARTY_NOTICES.md`
  (keduanya ikut di bundel: `Contents/Resources/licenses/`), `detect.rs` (mesin + `Smoother`), `Screen::detection_text`
  (baris layar terlihat + judul OSC 0/2 + payload OSC 9 lewat `vt100::Callbacks`), katalog `status = "screen"` +
  `detect` (divalidasi, `detect` tanpa `screen` ditolak), thread `pane-N-detect` per pane di `pty.rs` yang berhenti saat
  proses keluar dan mengirim `PaneEvent::Status` yang sama dengan jalur OSC (`blocked` → `needs_you` "butuh kamu").
  Layar yang tidak berubah tidak dinilai ulang. Penghalus yang di-port: masa tenggang 3 s dan tahan working → idle
  (3 bacaan / 700 ms); pengulangan blocker tiap 800 ms tidak di-port karena status disimpan core.
- **Uji.** 13 tes `detect` (11 di-port dari herdr + aturan bawaan pada layar contoh + penghalus dengan jam palsu),
  tes `screen` (OSC), tes katalog dan launch, tes PTY: program yang menggambar dialog izin Gemini dilaporkan
  `needs_you` setelah masa tenggang. `cargo test` 216 lulus.
- **Uji dengan agen asli (Gemini CLI 0.46.0, `tests/screen_cli.rs`, manual):** jalur bekerja (status pertama
  dipublikasikan dari layar asli), tetapi **aturan Gemini herdr (versi 2026.06.10.1, 2 aturan) tidak mengenali dialog
  "Do you trust the files in this folder?" maupun menu login**: keduanya terbaca `idle` (fallback), dan footer
  "Esc to cancel" pada dialog login terbaca `working`. Gemini di mesin pemilik juga tidak bisa dipakai lagi
  ("This client is no longer supported… migrate to Antigravity"). Pada uji pertama, teks prompt uji masuk ke menu login
  dan Enter memilih "Sign in with Google"; dialog lanjutannya tidak dijawab dan pane ditutup, jadi tidak ada login.
  Uji manual sejak itu hanya mengamati dan tidak mengetik apa pun.
- **Batasan.** Mutu deteksi per agen = mutu aturan herdr untuk agen itu, ditambah aturan agentdeck sendiri.
- **Aturan agentdeck sendiri (2026-10-23).** `src-tauri/detect-rules/*.toml`, skema dan batas sama dengan herdr, bukan
  bagian herdr (tidak masuk THIRD_PARTY_NOTICES). Saat dimuat, aturannya ditambahkan **setelah** aturan herdr untuk id
  yang sama (prioritas tertinggi menang; seri → aturan herdr); berkas herdr tidak diubah. `gemini.toml`: dialog trust
  folder, menu cara login, dan konfirmasi membuka browser → `blocked` prioritas 1000 (mengalahkan `esc_cancel_working`
  herdr yang membaca footer "Esc to cancel" sebagai bekerja). Tes memakai layar yang terlihat dari Gemini CLI 0.46.0;
  uji manual (hanya mengamati) dengan Gemini asli: dialog trust kini `needs_you`.

## D-036 — Tampilan profesional: Settings baru, token turunan, tanda agen netral (2026-10-24)
- Permintaan pemilik: Settings dan seluruh aplikasi terasa seperti aplikasi desktop profesional; referensi kualitas
  visual dari aplikasi lain hanya sebagai acuan, tanpa menyalin teks, aset, atau kode.
- **Settings:** modal 860 × 640 (menyusut ke jendela 900 × 600), navigasi berkelompok dengan ikon (Agen: Agen,
  Kolaborasi; Tampilan: Terminal, Pintasan; Lainnya: Tentang), judul + deskripsi per bagian, baris label/kontrol di
  kartu. Kontrol: switch (`input type=checkbox role=switch`), select native untuk tema, stepper satu bingkai
  (dipakai juga di peluncur), path manual dengan dialog pilih berkas, pratinjau perintah dengan Salin. Panah atas/bawah
  memindah bagian. Agen: kerangka saat memuat, galat dengan Coba lagi. Bagian baru: Pintasan (dari §7) dan Tentang
  (versi, lisensi, perintah pasang, folder log, disclaimer).
- **Token turunan** di `tokens.css`: `--hover`, `--pressed`, `--accent-soft` (color-mix dari token warna, jadi ikut
  tema tanpa hex baru), `--knob`, `--shadow-pop`, `--radius-modal`, `--text-heading`, `--text-title`. Tombol, input,
  dialog, sidebar, header pane, status bar, dan toast memakai token itu.
- **Saran skill desain ditolak sebagian:** Inter dari Google Fonts dan tema gelap saja bertentangan dengan "lokal
  saja" (font sistem) dan UI-UX §6 (terang + gelap). Daftar periksanya (fokus, kontras, gerak dikurangi) dipakai.
- **Ikon agen:** tidak ada aset logo Claude/Codex resmi di repo, dan D-026 melarang merek provider sebagai identitas
  agentdeck. Jadi `AgentMark`: glif Lucide netral (sparkles, bot, terminal) di petak berwarna token; nama agen di
  sebelahnya yang membedakan. Logo resmi hanya bila asetnya resmi dan lisensinya jelas.

## D-037 — Pasang agentdeck dengan satu perintah (`install.sh`) (2026-10-24)
- Permintaan pemilik: memasang aplikasi lewat CLI seperti proyek lain (`curl … | sh`).
- Belum ada rilis biner bertanda tangan (D-026, T5.2), jadi skrip membangun dari sumber: cek prasyarat (Xcode CLT,
  git, Node 22+, pnpm, cargo), clone/perbarui ke `~/.agentdeck/src`, `pnpm tauri build --bundles app`, salin ke
  `/Applications` (atau `~/Applications` bila tidak bisa ditulis) lewat salinan lalu tukar.
- Pagar: tidak memasang toolchain (hanya mencetak perintahnya), tanpa sudo, menolak folder sumber asing atau yang punya
  perubahan lokal, menolak bila agentdeck sedang berjalan, `--ref` divalidasi, seluruh isi di dalam `main()` supaya
  unduhan terpotong tidak dieksekusi, `--dry-run` tidak mengubah apa pun. Hanya macOS; platform lain ditolak dengan
  rujukan ke `docs/PLATFORMS.md`.
- Setelah notarisasi ada, skrip bisa beralih mengunduh `.dmg` rilis; Homebrew tap menyusul bila ada rilis.
- Di dalam aplikasi, perintah pasang agen (katalog) dan perintah ini hanya ditampilkan dengan tombol Salin; aplikasi
  tidak pernah menjalankan perintah pasang sendiri.

## D-038 — Status platform jujur; fondasi portabilitas tanpa klaim Windows (2026-10-24)
- Audit kode (dicatat di `docs/PLATFORMS.md`): Windows belum bisa dikompilasi (sinyal Unix di `pty.rs`), dan butuh
  env anak Windows, penanganan `.cmd` npm yang aman, shell bawaan, pintasan `Ctrl+Shift`, Job Object. Linux belum
  pernah dibangun; pemindaian turunan proses hanya ada di macOS.
- `cargo check --target x86_64-pc-windows-msvc` di Mac tidak bisa berjalan (tauri-build butuh `llvm-rc`), jadi bukti
  kompilasi Windows harus dari mesin Windows atau CI `windows-latest`.
- Dikerjakan sekarang, tanpa perubahan perilaku di macOS: `split_paths`/`is_absolute` untuk `PATH` dan `$SHELL`,
  `#[cfg(unix)]` pada bit eksekusi dan integrasi zsh. `pty.rs` sengaja belum disentuh: jalur Windows-nya perlu desain
  (ChildKiller + Job Object) dan uji di Windows asli.
