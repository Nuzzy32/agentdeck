# Progres dan titik lanjut

Terakhir diperbarui: 2026-10-22. **Phase 1 selesai.** Baca ini dulu saat melanjutkan, lalu `docs/IMPLEMENTATION-PLAN.md`.

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
| T2.4 (sebagian) | `screen.rs`: `vt100` per pane, `read(lines)` (alternate screen dibatasi tinggi layar), bracketed paste, `input_state` Empty/HasText/Unknown untuk Claude dan Codex | "HasText" hanya diuji dengan byte buatan (belum ada log nyata berisi ketikan); tool `pane_read` belum |
| T2.7 (sebagian) | `delivery.rs`: sanitasi, batas 8192 byte, penanda ≤ 256 byte, urutan tempel + 150 ms + Enter, FIFO maks 5, syarat siap dengan jam palsu, dropped saat keluar | Logika murni; disambungkan ke PTY/MCP di T2.7 |
| T1.5 | Workspace sungguhan + pohon split (`src/layout/tree.ts`, `SplitView.tsx`), fokus, pintasan UI-UX §7 tanpa ⌘J (`keys.ts`), dialog tutup, toast, ukuran font terminal, seret pembatas; title bar overlay macOS | Data contoh `sample.ts` dihapus (tipe pindah ke `model.ts`); menu app diganti dari JS (`src/menu.ts`) karena menu bawaan Tauri memakai ⌘W untuk Close Window; diuji di `pnpm tauri dev` dengan harness sementara (54 cek, catatan kerja) Uji manual pemilik 2026-10-22 lulus: ⌘W dari keyboard fisik menutup pane (dialog), title bar overlay tidak menimpa teks dan bisa diseret. |
| T1.8 | `PaneManager::close_all` paralel (batas 5 s) di `RunEvent::Exit` (jendela terakhir, ⌘Q, `app.exit`); `terminate` memotret keturunan pane (`proc_listchildpids`) lalu `SIGHUP`+`SIGTERM`, `SIGKILL` setelah 3 s ke grup dan setiap keturunan | Tes: 3 pane × 7 keturunan (job abaikan HUP/TERM, `setsid`, foreground) habis dalam satu masa tenggang; crash (helper di-`SIGKILL`) membersihkan shell + job biasa. Batasan saat crash: ARCHITECTURE §12. Uji manual pemilik 2026-10-22 lulus: `sleep 601 &` + `claude` di shell, ⌘Q, tidak ada proses tersisa |
| T2.5 | `tests/fake-agent` (std saja): mode claude/codex, bracketed paste, OSC 7501 sesuai Phase 0, `ASK` → blocked, `CALL` ke server MCP dengan token (dua versi protokol); `src-tauri/tests/fake_agent.rs` | 5 tes integrasi PTY memakai `status.rs` + `screen.rs` + `delivery.rs` + `mcp::server` |
| T1.6 | `catalogue.rs` + `catalogue.toml`: katalog claude-code/codex, `PATH` dari `$SHELL -l -i -c` dengan penanda, deteksi + versi, penolakan flag bypass, argv dengan `--` | Terdeteksi di mesin pemilik: claude 2.1.306, codex 0.163.0 |
| T1.7 | Peluncur (`src/launcher/`, ⌘T/⌘D/⌘⇧D/+, folder + 5 terakhir + dialog native D-021, tugas awal ⌘Enter), `launch.rs` (argv langsung, tugas setelah `--`, env dari nol), `pty.rs` menjawab `OSC 7501;?` dan mengirim event `status`/`exited` lewat `Channel` per pane, header status + tooltip `msg` + ikon galat, heuristik output, pita "selesai (kode N)" + Jalankan ulang / Buka shell di sini, "Gagal dijalankan" + Coba lagi, titik amber sidebar | Uji di `pnpm tauri dev` dengan harness sementara (salinan di catatan sesi `t1.7-e2e/`): alur A Claude (`memulai → siap`, dengan tugas `memulai → siap → bekerja → perlu login`), Codex (`memulai → siap`), CLI tidak terpasang (`SHELL=/bin/sh` → claude "Belum terpasang" + perintah pasang), config global tidak berubah. Uji manual pemilik 2026-10-22 lulus (peluncur Claude/Codex, Esc, pita + Jalankan ulang / Buka shell di sini, tampilan) |

## S6 throughput: selesai (D-018)
Lulus dengan flow control: jendela kredit di Rust (256 KB di D-018, diturunkan ke 128 KB di D-020) + `pane_ack` kumulatif dari callback `term.write` (microtask),
perintah `async`. Sumber referensi: `spikes/s6-throughput/`. Branch `spike/s6` masih ada secara lokal (tidak di-push,
tidak di-merge); boleh dihapus dengan `git branch -D spike/s6`.

## Berikutnya
- T2.3: injeksi MCP saat peluncuran (`launch::agent` kini memanggil `build_args(None, …)`; tinggal isi URL + env token).
- Phase 2 sisa: T2.3 (injeksi saat peluncuran), T2.4 tool `pane_list`/`pane_read`, T2.6, T2.7 penyambungan, T2.8, T2.9.


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
- Commit tanpa atribusi AI (aturan global pemilik).
