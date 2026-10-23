# Progres dan titik lanjut

Terakhir diperbarui: 2026-10-23. Phase 0–3 selesai (MVP); sisa Phase 4 dan uji ulang bagian Codex. Baca ini dulu saat melanjutkan, lalu `docs/IMPLEMENTATION-PLAN.md`.

## Serah terima 2026-10-23 (baca ini dulu)

**T3.2 sudah di-merge ke main dan di-push (2026-10-23)** (`pane_spawn` + audit + worktree + uji alur B, D-024). MVP
(alur B) berjalan; yang belum hanya bagian Codex (lihat di bawah).

**Belum terbukti (uji ulang):**
- Codex sebagai peninjau di alur B, `tool_timeout_sec=330` di Codex (kartu dijawab > 60 s), dan Codex sebagai pemanggil
  `pane_spawn`: server OpenAI terus "Selected model is at capacity" (juga `-m gpt-5.6-luna`).
- Permintaan kerja dari Codex ke Claude (yang terbukti baru Claude → Codex dikerjakan).
- Alur D: pita pengaman putaran di 12 pesan, Lanjutkan/Hentikan, dengan agen asli.
- `pane_read` dipanggil agen asli; pesan menunggu saat kotak input berisi ketikan orang (baru di fake-agent).
- Notifikasi macOS kartu persetujuan (perlu dilihat dan diklik pemilik).
- 6 pane agen aktif di MacBook Air M1 8 GB tanpa UI tersendat.

**Task tersisa:**
tinjau T2.6; Phase 5 (nama + bundle id final, signing + notarisasi, updater, ikon, cek ulang ketentuan provider D-006).

**Risiko/batasan yang diketahui:**
- Agen tujuan yang tidak pernah melapor status lagi setelah ditempeli pesan menahan antreannya (belum terlihat).
- Saat aplikasi crash: proses yang mengabaikan SIGHUP atau membuat sesi sendiri (Bash tool Claude) bisa tertinggal;
  pid yang dipakai ulang selama masa tenggang 3 s bisa ikut di-SIGKILL (`ponytail:` di `pty.rs`).
- Worktree anak yang masih terbuka saat aplikasi keluar tidak dibersihkan (kecuali anaknya dipulihkan T4.3 lalu ditutup).
- `pane_read` mengembalikan saran samar Claude ("Try …") seperti teks biasa.
- Claude di subfolder repo yang `CLAUDE.md`-nya mengimpor berkas di luar folder menampilkan dialog "Allow external
  CLAUDE.md file imports?" dan diam sampai dijawab.
- Jendela tersembunyi: WebKit menahan render; banjir output berwarna melambat (agen tidak tertahan).
- Log Codex: thread judul mencatat `invalid transport in mcp_servers.agentdeck` (tidak memengaruhi sesi).

**Keputusan tertunda pemilik:** D-019 (teruskan kunci API lewat daftar izin?), nama final + bundle id, distribusi
pribadi atau publik.

**Catatan lingkungan pemilik:**
- Izin Accessibility + Screen Recording diberikan ke binary Claude Code di
  `~/Library/Application Support/Claude/claude-code/2.1.299/4fa9694b7975/claude.app` (bukan Claude.app). Setelah Claude
  Code diperbarui, path berganti versi dan izin mungkin perlu diberikan ulang. Dengan izin ini sesi bisa menguji UI
  sendiri (cara: memori `agentdeck-ui-testing`).
- Uji T3.5 membuat CLI menambah entri trust folder uji sementara di `~/.claude.json` dan `~/.codex/config.toml` (oleh
  CLI, bukan agentdeck).

**Cara kerja yang disepakati:** sesi paralel di worktree untuk task independen (brief melarang merge/push dengan tegas),
uji gabungan di worktree sementara sebelum merge, cek `git reflog main` dan `grep '^## D-' docs/DECISIONS.md` setiap
merge, hapus worktree begitu masuk main, commit tanpa atribusi AI, push hanya atas persetujuan pemilik. Memori proyek di
`~/.claude/projects/-Users-adrian-dev-agentdeck/memory/` berisi aturan ini.

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
| S6 throughput | Lulus dengan flow control (jendela kredit 128 KB sejak D-020, ack kumulatif) | D-018, `spikes/s6-throughput/` |
| T0.7 | Tinjauan docs (44 temuan) diterapkan; sisa yang belum terbukti di ARCHITECTURE §12 | D-019 |
| T1.2 | Token desain + layout statis (title bar, sidebar, pane, panel Pesan, status bar, keadaan awal) dengan data contoh | `src/styles/tokens.css`, `src/layout/`; title bar native masih tampil sampai `titleBarStyle: Overlay` dipasang di `tauri.conf.json` |
| T1.3 | `PaneManager` (`src-tauri/src/pty.rs`): pane shell, tulis, resize, `pane_attach`/`pane_detach`, `pane_ack` kumulatif, jendela kredit 128 KB (D-020), buffer putar ulang 1 MB, `pane_close` | Tutup = `SIGHUP`+`SIGTERM` ke grup, `SIGKILL` setelah 3 s (lihat catatan kerja); belum dicoba di aplikasi (UI di T1.4) |
| T2.1 | `tokens.rs` (token CSPRNG, waktu-konstan, Debug disamarkan) + `policy.rs` (relasi §5.4, spawn, batas + rentang, rate limit, pengaman putaran) | Logika murni, belum disambungkan ke MCP (T2.2–T3.2) |
| OSC 7501 | `status.rs`: pemindai streaming, jawaban query, pemetaan status + label `kind` | Logika murni, disambungkan ke thread baca di T1.7 |
| T1.4 | `TerminalPane` (`src/pane/`): xterm.js + fit + webgl (cadangan DOM), tema dan font dari token, `pane_attach` lewat `Channel` + ack kumulatif via microtask, `onData` → `pane_write` dengan penanda `typed` (ketikan/tempelan DOM vs balasan xterm), resize, unmount = detach; operasi per pane diserialkan (StrictMode); pane `p1` di aplikasi = shell sungguhan di `~` | S6 diukur ulang: D-020 (ketikan p95 53–62 ms dengan jendela 256 KB saat banjir output berwarna, 19–24 ms dengan 128 KB; keputusan tertunda) |
| T2.2 | Server MCP (`src-tauri/src/mcp/`): rmcp + axum di `127.0.0.1:0`, token lewat `tokens.rs`, `Origin` apa pun 403, `Host` asing 403, protokol 2026-07-28 dan 2025-06-18, `instructions`, tool sementara `whoami` | 4 tes integrasi HTTP; server dimulai di `setup`, galat start tidak mematikan aplikasi; `whoami` dihapus di T2.4 |
| T2.4 + T2.7 (core) | Tool MCP `pane_list`, `pane_read` (READ_RATE 30/menit, alternate screen ≤ tinggi layar), `pane_send` (`to` id atau `"parent"`, sanitasi + 8192 byte, FIFO maks 5, hasil `queued`/`delivered`/`paused`, semua kode galat MCP-TOOLS) di `mcp/tools.rs` dengan skema tulis tangan; galat = hasil tool `isError` `CODE: …`; `whoami` dihapus. Registri memegang antrean, catatan pesan, sakelar peer, pengaman putaran per workspace (satu kunci). `delivery::Engine`: worker per pane tujuan (syarat siap §5.2 dari status, layar, `typed_at`), tempelan ke PTY, log JSONL setiap status (tanpa token), event `agentdeck://messages-changed`/`agentdeck://loop-guard`, Enter orang (`pane_write` typed + `\r`) mengatur ulang. Perintah UI `messages_list`, `message_drop`, `loop_guard_get/continue/stop`, `workspace_set_peer/get_peer` | Tes: unit tool (setiap kode galat, relasi §5.4, READ_RATE, TOO_LARGE, QUEUE_FULL, anotasi), unit registri dengan jam palsu (antar, ketikan, input tidak kosong, jeda/Lanjutkan/Hentikan/Enter, Buang, keluar), HTTP `isError`, integrasi 3 fake-agent lewat `panes::start` + MCP (induk↔anak, peer `PEER_SEND_OFF` lalu lolos, ketikan ≥ `typing_ms`, pengaman putaran batas 2, keluar → dropped, JSONL tanpa token). CLI asli: lihat catatan kerja T2.7. `quiet_ms`/`typing_ms` belum disetel (bawaan 1500/2000). Belum dicoba di `pnpm tauri dev` (UI T2.8) Uji manual pemilik 2026-10-22 lulus (alur C): pane_list (peer, can_send false), saling kirim mati → Claude tidak mengirim, sakelar dinyalakan di status bar, Claude → Codex `delivered`, Codex membalas lewat pane_send, Claude menerima; status bar 2/12. |
| T3.1 | `approvals.rs` (`request(req, timeout)` → Approved/Denied/Timeout, bertumpuk, batal saat pane peminta keluar), kartu di atas pane peminta (`src/cards/`: hitung mundur, ⌘Enter/Esc, tidak mencuri fokus, ⌘J), notifikasi macOS (`tauri-plugin-notification`, D-022), `approval_demo` khusus build debug | Belum dipakai `pane_spawn` (T3.2); notifikasi belum terlihat dengan mata |
| T3.3 + T4.7 | `settings.rs` + `settings.json` (validasi rentang di Rust, tulis atomik, berkas rusak → bawaan), batas/sakelar langsung berlaku, modal Settings (⌘,: Agen/Kolaborasi/Terminal/Tentang), tema/font/ukuran/scrollback terminal dari settings | Bagian Agen memakai `agents_info` (fallback `agents_list`) Uji manual pemilik 2026-10-23 lulus: ⌘,, tema/font langsung berubah, bagian Agen (versi/path/login), stepper menolak di luar rentang, sakelar spawn ikut di status bar, Esc. |
| T4.4 + T4.5 (core) | Katalog pengguna `agents.toml` (validasi + tolak flag bypass), path manual per agen, metode login lewat `claude auth status` (tanpa membaca kredensial), nama kunci API di shell login tanpa nilai; `agents_info`/`agent_set_path` (D-023) | UI di Settings › Agen |
| T4.2 (core) | Tool `pane_report` (RESULT ke induk lewat jalur pane_send, NO_PARENT, REPORT_RATE 6/menit), laporan terakhir per anak, event `agentdeck://pane-report`, `pane_report_last` | Header pane anak: lencana `hasil: selesai · tes lulus` (warna + kata), tooltip ringkasan/berkas/jam, dihapus saat Jalankan ulang (tes vitest). Uji dengan CLI asli 2026-10-23 (Claude Code 2.1.309): p1 memanggil `pane_spawn` (worktree), anak p2 memanggil `pane_report`, header p2 menampilkan `hasil: selesai · tes lulus`, p1 menerima RESULT |
| T3.2 + T3.4 + T4.1 | `spawn.rs` (`pane_spawn`: validasi argumen, sakelar/`NESTED`/batas/rate dengan kartu menunggu dihitung sebagai anak, katalog + terpasang, `cwd` dikanonikalisasi di dalam folder pemanggil, kartu dengan batas waktu Settings, anak lewat `panes::start` dengan induk = pemanggil, `SPAWN_FAILED`), `worktree.rs` (`.worktrees/<id>` di `agent/<id>`, exclude lokal, hook mati; dihapus saat ditutup bila tanpa perubahan, dibiarkan + toast bila berubah), audit `spawn_*`/`pane_closed`, UI: anak di samping induk dengan lencana, event `pane-spawned`/`pane-event`/`spawn-refused`, toast. `"timeout":330000` di `--mcp-config` Claude (D-024) | Tes unit tiap kode galat, `cwd` + symlink keluar, rate/kartu menunggu, audit tanpa token, worktree dibuat/dihapus/dibiarkan; integrasi fake-agent: `CALL pane_spawn` → kartu → jawab → anak dengan induk benar → `pane_report` → RESULT di induk, DENIED, worktree. Uji di aplikasi dengan CLI asli: lihat T3.5 |
| T3.5 | Alur B di `pnpm tauri dev` dengan Claude Code 2.1.308 + Codex 0.163.0 | Lulus kecuali bagian Codex (server "at capacity", juga `gpt-5.6-luna`): kartu disetujui setelah 95 s berhasil (perlu perbaikan timeout), Codex terbuka di samping dengan "anak p1", Claude Code sebagai peninjau pengganti melapor dan induk menerima RESULT; DENIED, APPROVAL_TIMEOUT, SPAWN_OFF + toast, RATE, worktree hapus/biarkan + toast. Rincian D-024 |
| T4.6 | Pesan queued/paused > 10 menit ditandai `held`, `message_force` (abaikan sepi/ketikan/input, tetap tolak bila keluar/bracketed paste mati/dijeda), panel: chip + "Kirim sekarang" dengan konfirmasi | — |
| T4.3 | `layout.rs` (`layout.json`: validasi + tulis atomik, dibaca di `setup`, pane didaftarkan sebagai `exited` dengan id lama), `Registry::restore`, `worktree::still_ours`, `layout_restore`/`layout_save`; UI `persist.ts` (potret + hidrasi), simpan dengan jeda 400 ms, pita "belum dijalankan" + Jalankan (D-025) | Tes unit Rust (validasi, berkas rusak, id, worktree palsu ditolak, registri) + vitest. Di aplikasi: 2 shell split → keluar → buka → pulih, Jalankan jalan, pane baru p3. Dengan CLI asli: anak p2 (worktree `agent/p2`) pulih dengan `anak p1` dan menjorok di sidebar; menutupnya menghapus worktree + branch yang bersih |
| T2.4 (sebagian) | `screen.rs`: `vt100` per pane, `read(lines)` (alternate screen dibatasi tinggi layar), bracketed paste, `input_state` Empty/HasText/Unknown untuk Claude dan Codex | "HasText" hanya diuji dengan byte buatan (belum ada log nyata berisi ketikan); tool `pane_read` belum |
| T2.7 (sebagian) | `delivery.rs`: sanitasi, batas 8192 byte, penanda ≤ 256 byte, urutan tempel + 150 ms + Enter, FIFO maks 5, syarat siap dengan jam palsu, dropped saat keluar | Logika murni; disambungkan ke PTY/MCP di T2.7 |
| T1.5 | Workspace sungguhan + pohon split (`src/layout/tree.ts`, `SplitView.tsx`), fokus, pintasan UI-UX §7 tanpa ⌘J (`keys.ts`), dialog tutup, toast, ukuran font terminal, seret pembatas; title bar overlay macOS | Data contoh `sample.ts` dihapus (tipe pindah ke `model.ts`); menu app diganti dari JS (`src/menu.ts`) karena menu bawaan Tauri memakai ⌘W untuk Close Window; diuji di `pnpm tauri dev` dengan harness sementara (54 cek, catatan kerja) Uji manual pemilik 2026-10-22 lulus: ⌘W dari keyboard fisik menutup pane (dialog), title bar overlay tidak menimpa teks dan bisa diseret. |
| T1.8 | `PaneManager::close_all` paralel (batas 5 s) di `RunEvent::Exit` (jendela terakhir, ⌘Q, `app.exit`); `terminate` memotret keturunan pane (`proc_listchildpids`) lalu `SIGHUP`+`SIGTERM`, `SIGKILL` setelah 3 s ke grup dan setiap keturunan | Tes: 3 pane × 7 keturunan (job abaikan HUP/TERM, `setsid`, foreground) habis dalam satu masa tenggang; crash (helper di-`SIGKILL`) membersihkan shell + job biasa. Batasan saat crash: ARCHITECTURE §12. Uji manual pemilik 2026-10-22 lulus: `sleep 601 &` + `claude` di shell, ⌘Q, tidak ada proses tersisa |
| T2.5 | `tests/fake-agent` (std saja): mode claude/codex, bracketed paste, OSC 7501 sesuai Phase 0, `ASK` → blocked, `CALL` ke server MCP dengan token (dua versi protokol); `src-tauri/tests/fake_agent.rs` | 5 tes integrasi PTY memakai `status.rs` + `screen.rs` + `delivery.rs` + `mcp::server` |
| T1.6 | `catalogue.rs` + `catalogue.toml`: katalog claude-code/codex, `PATH` dari `$SHELL -l -i -c` dengan penanda, deteksi + versi, penolakan flag bypass, argv dengan `--` | Terdeteksi di mesin pemilik: claude 2.1.306, codex 0.163.0 |
| T2.3 | `panes.rs`: registri pane di core (id `pN` dari core, tidak dipakai ulang; workspace dari perintah UI dengan `peer_messaging` bawaan mati; agen/judul/cwd/induk; PTY; `Screen` diberi makan thread baca; status OSC terakhir; token hanya pane agen, dicabut saat keluar, ditutup, gagal jalan, atau dijalankan ulang; `list` = isi `pane_list` lewat `policy.rs`). Injeksi MCP di `launch::agent` (env `AGENTDECK_*`, argv katalog, token hanya di env, `Debug` env disamarkan). Server MCP memetakan token lewat registri. Perintah `pane_create`/`pane_close`/`workspace_create`/`workspace_close` di `panes.rs`; UI menerima id pane dan workspace dari core | Tes: unit registri (4), argv/env injeksi + tugas 16 KB diawali `-`, fake-agent lewat `panes::start` (`whoami` = id pane kedua mode, 401 setelah keluar/ditutup), 401 setelah tutup di `mcp_server.rs`. CLI asli lewat `panes::start` (harness sementara, tidak di-commit): Claude 2.1.308 dan Codex 0.163.0 `WHOAMI=p2`, tugas 16 KB utuh, tanpa prompt izin, `mcpServers` `~/.claude.json` dan shasum `~/.codex/config.toml` tetap. Belum diklik di `pnpm tauri dev` Uji manual pemilik 2026-10-22 lulus: whoami dari Claude dan Codex di aplikasi = id pane, tanpa prompt izin. |
| T1.7 | Peluncur (`src/launcher/`, ⌘T/⌘D/⌘⇧D/+, folder + 5 terakhir + dialog native D-021, tugas awal ⌘Enter), `launch.rs` (argv langsung, tugas setelah `--`, env dari nol), `pty.rs` menjawab `OSC 7501;?` dan mengirim event `status`/`exited` lewat `Channel` per pane, header status + tooltip `msg` + ikon galat, heuristik output, pita "selesai (kode N)" + Jalankan ulang / Buka shell di sini, "Gagal dijalankan" + Coba lagi, titik amber sidebar | Uji di `pnpm tauri dev` dengan harness sementara (salinan di catatan sesi `t1.7-e2e/`): alur A Claude (`memulai → siap`, dengan tugas `memulai → siap → bekerja → perlu login`), Codex (`memulai → siap`), CLI tidak terpasang (`SHELL=/bin/sh` → claude "Belum terpasang" + perintah pasang), config global tidak berubah. Uji manual pemilik 2026-10-22 lulus (peluncur Claude/Codex, Esc, pita + Jalankan ulang / Buka shell di sini, tampilan) |

## S6 throughput: selesai (D-018)
Lulus dengan flow control: jendela kredit di Rust (256 KB di D-018, diturunkan ke 128 KB di D-020) + `pane_ack` kumulatif dari callback `term.write` (microtask),
perintah `async`. Sumber referensi: `spikes/s6-throughput/`. Branch `spike/s6` masih ada secara lokal (tidak di-push,
tidak di-merge); boleh dihapus dengan `git branch -D spike/s6`.

## Berikutnya
- Ulangi alur B dengan Codex sebagai peninjau saat server OpenAI tidak "at capacity", termasuk kartu dijawab setelah
  90 s di Codex (`tool_timeout_sec=330`) dan Codex sebagai pemanggil `pane_spawn`.
- Belum diuji manual: alur D (pengaman putaran di 12 pesan + pita), `pane_read` dari agen, pesan tertahan saat kotak input berisi teks.
- T2.8: pengaman putaran + sakelar "Saling kirim" + panel Pesan di UI memakai perintah/event T2.7. T2.6: tinjau sisa.
- Ulangi uji CLI asli T2.7 (`cargo test --test real_cli -- --ignored --nocapture`) saat Codex tidak "at capacity":
  Claude mengerjakan permintaan dari tempelan, balasan yang mengutip baris terakhir tempelan 8 KB.


## Keputusan tertunda (pemilik)
- D-019: apakah kunci API (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) boleh diteruskan ke agen lewat daftar izin.
- Login Claude Code CLI pemilik kedaluwarsa per 2026-10-22: jalankan `claude`, lalu `/login` (dilakukan pemilik sendiri).

## Catatan kerja
- Jalankan spike agen dengan env bersih (`CLEAN` di PHASE-0 S1); sesi Claude Code membocorkan `CLAUDE_CODE_*`.
- Server OpenAI sering "Selected model is at capacity"; untuk spike pakai `-m gpt-5.6-luna` atau `-m gpt-6-luna`.
- T1.3: `pane_close` juga mengirim `SIGHUP`, bukan hanya `SIGTERM`: zsh interaktif mengabaikan `SIGTERM`, dan job
  latar belakangnya (grup proses sendiri) hanya ikut berhenti lewat `SIGHUP` dari shell. Tanpa ini tes job latar gagal.
  Thread baca tetap menguras PTY setelah tutup: di macOS proses yang keluar tertahan (`E`) sampai output-nya terbaca.
- T1.8: Bash tool Claude Code menjalankan setiap perintah di sesi sendiri, jadi `killpg` ke grup pane tidak
  menjangkaunya; karena itu `terminate` juga mengirim sinyal ke setiap keturunan. Server MCP tidak dihentikan eksplisit
  saat keluar (socket tertutup bersama proses). Risiko kecil: pid yang dipakai ulang selama masa tenggang 3 s bisa
  ikut di-`SIGKILL` (komentar `ponytail:` di `pty.rs`).
- T3.1, T3.3+T4.7, T4.4+T4.5, T4.2, T4.6 dikerjakan paralel di lima sesi worktree lalu digabung (2026-10-23). `App.tsx`
  disusun ulang dengan tangan (T3.3 membungkus render dengan `TerminalContext.Provider`), `fake_agent.rs` digabung dua tes.
- T1.5, T1.8, T2.5 dikerjakan paralel di tiga sesi worktree lalu digabung (2026-10-22).
- T1.4, T2.2, screen.rs, delivery.rs dikerjakan paralel di empat sesi worktree lalu digabung (2026-10-22). Saat
  menggabung, git menaruh `vt100` di bagian `[dev-dependencies]`; diperbaiki manual. Periksa `Cargo.toml` setiap merge.
- T1.2, T1.3, T2.1, OSC 7501, T1.6 dikerjakan paralel di lima sesi worktree lalu digabung (2026-10-22).
- T1.4: harness ukur S6 untuk aplikasi nyata tidak di-commit; ARCHITECTURE §12 baris T1.4 belum diperbarui (lihat D-020).
  `htop` tidak terpasang di mesin pemilik; diuji dengan `top`. Tampilan visual (WebGL) belum dilihat mata: sesi agen tidak
  punya izin tangkapan layar, pemeriksaan lewat isi buffer xterm.js.
- T1.5: harness uji sementara (tidak di-commit, salinan di folder catatan sesi `t1.5-e2e/`) menjalankan App asli di
  jendela Tauri dengan event keyboard/pointer sintetis: semua pintasan, split/resize/tutup/pindah workspace dengan shell
  sungguhan, tolak < 40×8, replay setelah detach, lalu `ps` memastikan tidak ada shell tertinggal. Belum dicoba dengan
  keyboard fisik (sesi agen tidak punya izin Accessibility/Screen Recording): pemilik perlu mencoba ⌘W sekali untuk
  memastikan menu bawaan tidak lagi menutup jendela, dan melihat title bar overlay (lampu jendela tidak menimpa teks).
- T1.5 → T1.7: ⌘W tidak lagi bertanya untuk pane yang sudah keluar atau gagal; ⌘T/⌘D/⌘⇧D kini membuka peluncur.
- T1.7: dijalankan dari sesi Claude Code, proses agentdeck membawa `CLAUDE_CODE_*`; `ps eww` pada anak `claude`/`codex`
  menunjukkan nol variabel itu. Sekali (dari empat peluncuran Claude) Claude menampilkan "Gesticulating…" selama 60 s
  tanpa pernah melapor `working`, jadi header tetap "siap"; tiga run lain melapor `idle → working → blocked:auth`
  dengan benar. Kemungkinan perilaku Claude saat login kedaluwarsa; amati lagi setelah `/login`.
  Uji manual pemilik: (1) ⌘T, pilih Claude Code dengan panah, folder lewat "Pilih…" (dialog native), Enter → header
  `memulai` lalu status OSC; (2) ⌘D → Codex dengan tugas awal, ⌘Enter; (3) Esc menutup peluncur; (4) di pane shell
  `exit 3` → pita, klik Jalankan ulang / Buka shell di sini; (5) tombol Salin perintah pasang (clipboard WebView);
  (6) lihat tampilan peluncur dan pita di tema gelap dan terang.
- T2.3: uji CLI asli memakai mode izin bawaan pengguna; Claude pemilik berjalan dengan "auto mode on" (setelan
  pengguna, bukan dari agentdeck), jadi "tanpa prompt izin" untuk Claude di run ini tidak membedakan `--allowedTools`
  dari auto mode (S2 sudah membuktikan `--allowedTools`). Codex tanpa prompt karena `default_tools_approval_mode`.
  Uji manual pemilik: `pnpm tauri dev`, luncurkan Claude Code lalu Codex dari peluncur, minta masing-masing
  "panggil tool whoami dari server MCP agentdeck"; jawaban harus id pane di header (mis. `p2`) tanpa prompt izin.
  Lalu Jalankan ulang setelah `/exit` (id tetap sama), tutup pane, dan bandingkan `jq -c .mcpServers ~/.claude.json |
  shasum` serta `shasum ~/.codex/config.toml` sebelum/sesudah.
- Commit tanpa atribusi AI (aturan global pemilik).
- T3.5: potret jendela agentdeck saat tertutup jendela lain menampilkan isi pane yang basi (WebKit menahan render);
  data tetap mengalir, setelah jendela dibawa ke depan isinya mutakhir. Bawa jendela ke depan sebelum memotret.
  Menjawab dialog trust folder Claude/Codex di repo uji menulis entri proyek ke `~/.claude.json` dan
  `~/.codex/config.toml` (oleh CLI itu sendiri, bukan agentdeck).
- T2.7 uji CLI asli (2026-10-22, Claude Code 2.1.308, Codex 0.163.0, `src-tauri/tests/real_cli.rs`, env dari nol lewat
  `panes::start`): Claude `idle` dalam 1 s; pesan core → Claude dan core → Codex langsung `delivered` (input kosong
  terdeteksi di kedua TUI asli); Codex menampilkan satu tempelan 8286 karakter (8192 + penanda + `\n`), Claude
  `[Pasted text #1 +215 lines]` lalu `working`. Codex menjawab "Selected model is at capacity" (model bawaan dan
  `-m gpt-5.6-luna` lewat pembungkus di `PATH`, hanya untuk uji), jadi alur Codex → Claude → Codex tidak terjadi.
  Temuan: Claude yang dijalankan di subfolder repo yang `CLAUDE.md`-nya mengimpor berkas dari akar repo menampilkan
  dialog "Allow external CLAUDE.md file imports?" dan diam (`unknown`) sampai dijawab; uji memakai akar worktree
  sebagai cwd. `mcpServers` `~/.claude.json` dan shasum `~/.codex/config.toml` sama sebelum/sesudah.
- T2.7 keputusan kecil: id pesan `m<N>` per run; panel memakai catatan di memori untuk run ini (workspace id juga per
  run), log JSONL tetap ditulis. `DropReason::Discarded` ditambah untuk "Buang". Setelah tempelan, status tujuan diset
  `working` di core sampai laporan OSC berikutnya; agen yang tidak pernah melapor lagi setelah tempelan akan menahan
  antreannya (belum terlihat pada kedua CLI). Worker yang menunggu memeriksa ulang tiap 100 ms.
- `whoami` sudah tidak ada sejak T2.4; uji manual T2.3 kini memakai `pane_list` (`you` = id pane).
