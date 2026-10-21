# agentdeck — Product Requirements Document

| | |
|---|---|
| Nama | agentdeck (nama kerja, D-001) |
| Versi dokumen | 0.1 (2026-10-21) |
| Platform | macOS 13+ (Apple Silicon dulu), aplikasi desktop |
| Stack | Tauri 2 + Rust, UI React + TypeScript + xterm.js (D-002) |
| Dokumen terkait | `docs/ARCHITECTURE.md` (cara kerja), `docs/MCP-TOOLS.md` (kontrak tool), `docs/UI-UX.md`, `docs/IMPLEMENTATION-PLAN.md`, `docs/PHASE-0.md`, `docs/SETUP.md`, `docs/DECISIONS.md`, `docs/RESEARCH.md` |

## 1. Ringkasan
agentdeck adalah terminal desktop untuk menjalankan beberapa agen coding CLI (Claude Code, Codex, dan lainnya) di pane
berdampingan, dan membuat agen-agen itu **bisa saling melempar prompt**: satu agen membuka agen lain, memberi tugas,
membaca layarnya, dan menerima hasilnya. Setiap agen adalah CLI resmi yang berjalan apa adanya dengan login milik
pengguna. agentdeck hanya menyediakan terminal, jembatan MCP lokal, dan pengiriman pesan yang terlihat dan terkendali.

## 2. Masalah
- Claude Code dan Codex kuat di hal yang berbeda, tetapi berjalan terpisah. Memindahkan konteks di antara keduanya
  (copy-paste diff, temuan review, instruksi perbaikan) dilakukan manual dan lambat.
- Review oleh agen yang sama dengan pembuatnya cenderung melewatkan kesalahannya sendiri.
- Alat yang ada mahal dan tertutup (MXterm Pro) atau besar dan rumit dipelajari (T3 Code), lihat `docs/RESEARCH.md`.

## 3. Pengguna
Satu pengguna: developer di Mac yang sudah memasang dan login ke Claude Code dan/atau Codex, dan bekerja di repo git
lokal. Tidak ada akun agentdeck, tidak ada server, tidak ada multi-user.

## 4. Prinsip produk
1. **Agen tetap asli.** CLI resmi, binary tidak dimodifikasi, login milik pengguna. agentdeck tidak pernah membaca,
   menyimpan, atau meneruskan kredensial agen (D-006).
2. **Orang memegang kendali.** Membuka pane agen oleh agen, menutup pane, merge, push, dan deploy selalu keputusan
   orang. Tidak ada flag bypass untuk agen yang dibuka oleh agen.
3. **Semua terlihat.** Setiap pesan antar agen tampil di pane tujuan dengan penanda pengirim dan tercatat di log.
4. **Lokal saja.** Server MCP hanya di `127.0.0.1`. Tidak ada cloud, telemetri, atau akun.
5. **Tidak meninggalkan jejak.** Konfigurasi agen disuntikkan per peluncuran; config global pengguna tidak diubah (D-005).
6. **Kecil dulu.** Satu alur ujung ke ujung (Claude ↔ Codex) sebelum fitur lain.

## 5. Tujuan dan non-goals
### Tujuan MVP
- Terminal multi-pane yang nyaman untuk shell dan agen.
- Meluncurkan Claude Code dan Codex dari katalog dengan satu klik.
- Agen bisa: melihat daftar pane, membuka agen lain (dengan persetujuan), mengirim prompt, membaca layar pane lain,
  dan menerima balasan yang ditempel otomatis ke terminalnya.
- Pola pembuat–peninjau berjalan: Claude membuat, Codex meninjau, temuan dikirim balik ke Claude, atau sebaliknya.

### Bukan tujuan MVP
- Akses jarak jauh, aplikasi mobile, sinkronisasi cloud.
- Preset tim, router otomatis, notifikasi Telegram/Slack.
- Membaca kuota langganan dari kredensial (kuota hanya ditampilkan bila agen sendiri melaporkannya, P2).
- Windows dan Linux (arsitektur tidak boleh menutup kemungkinan ini).
- Editor kode, viewer diff penuh, integrasi GitHub.
- Menjalankan agen lewat SDK/API (jalur T3 Code).

## 6. Persyaratan
Prioritas: **P0** wajib untuk MVP, **P1** setelah alur inti stabil, **P2** nanti.

### 6.1 Terminal (TERM)
| ID | P | Persyaratan |
|---|---|---|
| TERM-01 | P0 | Tab dan split pane (horizontal/vertikal), ubah ukuran dengan seret, fokus dengan klik dan pintasan keyboard. |
| TERM-02 | P0 | Pane shell memakai shell login pengguna (`$SHELL -l`) di folder pilihan. |
| TERM-03 | P0 | Rendering xterm.js: warna 256/truecolor, Unicode/emoji, salin-tempel, scrollback ≥ 10.000 baris, cari teks. |
| TERM-04 | P0 | Ukuran PTY mengikuti ukuran pane (resize). |
| TERM-05 | P0 | Menutup pane atau aplikasi menghentikan seluruh grup proses pane; tidak ada proses yatim. |
| TERM-06 | P1 | Tata letak tab dan pane dipulihkan saat aplikasi dibuka lagi (tanpa memulihkan proses). |
| TERM-07 | P1 | Tema terang/gelap mengikuti sistem; font dan ukuran bisa diatur. |

### 6.2 Agen (AGT)
| ID | P | Persyaratan |
|---|---|---|
| AGT-01 | P0 | Katalog agen bawaan berisi `claude-code` dan `codex`: perintah, cara memberi tugas awal, cara menyuntik MCP, sumber status. |
| AGT-02 | P0 | Deteksi apakah CLI terpasang (`PATH` dari shell login). Bila tidak, tampilkan cara memasang; jangan memasang otomatis. |
| AGT-03 | P0 | Meluncurkan agen di pane baru dengan folder kerja dan tugas awal opsional. |
| AGT-04 | P0 | Header pane menampilkan agen, folder, dan status (`starting`, `working`, `idle`, `needs_you`, `exited`, `unknown`). |
| AGT-05 | P1 | Tampilkan metode login agen bila CLI menyediakannya tanpa membaca kredensial (mis. `claude auth status`), dan peringatkan bila `ANTHROPIC_API_KEY` ada di environment (akan dipakai dan ditagih per token). |
| AGT-06 | P1 | Agen tambahan lewat berkas katalog pengguna (gemini, opencode, dsb.). |
| AGT-07 | P2 | Pemilihan model per peluncuran bila CLI mendukung flag model. |

### 6.3 Jembatan MCP (MCP)
| ID | P | Persyaratan |
|---|---|---|
| MCP-01 | P0 | Server MCP *streamable HTTP* di `127.0.0.1` dengan port acak, hidup selama aplikasi berjalan. |
| MCP-02 | P0 | Token acak per pane; server mengenali pemanggil dari token, bukan dari argumen. |
| MCP-03 | P0 | Server disuntikkan ke agen per peluncuran tanpa mengubah config global (D-005). |
| MCP-04 | P0 | Tool P0: `pane_list`, `pane_spawn`, `pane_send`, `pane_read` (kontrak di `docs/MCP-TOOLS.md`). |
| MCP-05 | P0 | `instructions` server MCP menjelaskan cara memakai tool dan aturan kerja sama dengan teks kita sendiri. |
| MCP-06 | P1 | `pane_report` (hasil terstruktur ke pane pembuka) dan opsi worktree pada `pane_spawn`. |
| MCP-07 | P0 | Tolak permintaan dengan `Host`/`Origin` asing (perlindungan DNS rebinding). |

### 6.4 Pengiriman pesan (MSG)
| ID | P | Persyaratan |
|---|---|---|
| MSG-01 | P0 | Pesan masuk antrean per pane tujuan dan ditempel saat tujuan siap: status `idle`, atau output sepi dan pengguna tidak sedang mengetik. |
| MSG-02 | P0 | Setiap pesan diawali baris penanda pengirim (pane, agen) dan dikirim sebagai satu tempelan lalu Enter. |
| MSG-03 | P0 | Batas 8 KB per pesan; karakter kontrol dibuang kecuali baris baru. |
| MSG-04 | P0 | **Pengaman putaran:** setelah N pesan antar agen tanpa masukan orang di workspace (bawaan 12), antrean dijeda dan orang ditanya. |
| MSG-05 | P0 | Panel log pesan per workspace: waktu, pengirim, tujuan, status (`queued`, `delivered`, `dropped`), isi. |
| MSG-06 | P0 | Pengiriman antar pane yang bukan induk–anak hanya bila sakelar workspace "Agen boleh saling kirim" aktif (bawaan mati). |
| MSG-07 | P1 | Pesan yang tertahan lebih dari 10 menit ditandai dan bisa dikirim paksa atau dibuang oleh orang. |

### 6.5 Membuka agen oleh agen (SPN)
| ID | P | Persyaratan |
|---|---|---|
| SPN-01 | P0 | `pane_spawn` menampilkan kartu persetujuan (agen, judul, folder, tugas). Tanpa jawaban dalam 120 detik = ditolak. |
| SPN-02 | P0 | Batas: 3 anak hidup per pane, 8 pane agen dibuka agen per aplikasi, 5 spawn per 10 menit. Pane anak tidak boleh spawn. |
| SPN-03 | P0 | Folder kerja anak harus di dalam repo/folder pembuka. Tidak ada argumen untuk perintah, flag, atau env. |
| SPN-04 | P0 | Hanya orang yang menutup pane. |
| SPN-05 | P1 | `worktree: true` membuat `git worktree` di `.worktrees/<id>` dengan branch `agent/<id>`; worktree tanpa perubahan dihapus saat pane ditutup. |

### 6.6 Pengaturan (SET)
| ID | P | Persyaratan |
|---|---|---|
| SET-01 | P0 | Sakelar global "Agen boleh membuka pane" (bawaan mati) dan sakelar per workspace "Agen boleh saling kirim". |
| SET-02 | P0 | Angka pengaman putaran (MSG-04) dan batas spawn bisa diatur dalam rentang aman. |
| SET-03 | P1 | Path CLI agen bisa ditimpa manual. |

## 7. Alur pengguna dan acceptance criteria

### A. Membuka agen manual
1. Pengguna menekan "+ Agen", memilih Claude Code, memilih folder.
2. Pane baru menjalankan `claude` di folder itu; status berubah `starting` → `idle`.
- AC: CLI yang belum terpasang menampilkan cara memasang dan tidak membuka pane rusak.
- AC: tidak ada entri `agentdeck` yang tertinggal di `~/.claude.json` (bagian `mcpServers`, global dan per proyek) maupun di `~/.codex/config.toml` setelah peluncuran (cara cek: `docs/PHASE-0.md` S2/S3).

### B. Claude meminta Codex meninjau (alur inti MVP)
1. Di pane Claude, pengguna menulis: "kerjakan X, lalu minta Codex review".
2. Claude memanggil `pane_spawn {agent: "codex", title: "review: X", task: ...}`; kartu persetujuan muncul; pengguna setuju.
3. Pane Codex terbuka di samping dan mulai meninjau.
4. Codex selesai dan memanggil `pane_send {to: "parent", text: "RESULT ... temuan ..."}`.
5. Pesan ditempel ke pane Claude saat Claude `idle`; Claude memperbaiki dan bisa mengirim satu putaran lagi.
- AC: setiap pesan terlihat di kedua pane dan di log pesan.
- AC: menolak kartu membuat `pane_spawn` mengembalikan galat `DENIED`, dan Claude melanjutkan sendiri.
- AC: pesan tidak pernah ditempel saat pengguna sedang mengetik di pane tujuan.

### C. Dua agen yang dibuka manual saling kirim
1. Pengguna membuka Claude dan Codex di workspace yang sama dan menyalakan "Agen boleh saling kirim".
2. Claude memanggil `pane_list`, menemukan pane Codex, lalu `pane_send`.
- AC: dengan sakelar mati, `pane_send` ke peer ditolak `PEER_SEND_OFF`.

### D. Pengaman putaran
1. Dua agen saling membalas otomatis.
2. Setelah 12 pesan tanpa masukan orang, antrean berhenti dan muncul kartu "Lanjutkan / Hentikan".
- AC: mengetik di pane mana pun di workspace itu juga mengatur ulang hitungan.

## 8. Kebutuhan non-fungsional
- **Responsif:** ketikan tampil < 50 ms; 6 pane agen aktif bersamaan tanpa UI tersendat di MacBook Air M1 8 GB.
- **Keamanan:** server hanya loopback; token 256-bit dari CSPRNG, hanya di memori dan env anak; tidak ada token di log
  atau disk; validasi semua argumen tool di sisi Rust.
- **Keandalan:** crash aplikasi tidak meninggalkan proses agen; antrean pesan tidak menggandakan pengiriman.
- **Privasi:** tidak ada jaringan keluar oleh agentdeck sendiri, kecuali updater (P2, bisa dimatikan).
- **Aksesibilitas:** semua aksi bisa lewat keyboard; kartu persetujuan bisa dijawab dengan keyboard; kontras memadai.

## 9. Roadmap
### Phase 0: Spike kelayakan (wajib sebelum Phase 2)
Buktikan dengan CLI asli dan catat hasilnya di DECISIONS (lihat `docs/ARCHITECTURE.md` §10):
tempel + Enter di Claude Code dan Codex; injeksi MCP per peluncuran di keduanya; hook status per sesi; throughput PTY
lewat Tauri Channel; crate emulator headless untuk membaca layar.

### Phase 1: Fondasi terminal
Proyek Tauri, tab dan split, pane shell, xterm.js, PTY, resize, penghentian proses (TERM-01..05), katalog dan peluncuran
agen manual (AGT-01..04).

### Phase 2: Jembatan MCP dan pesan
Server MCP, token, injeksi, `pane_list`, `pane_read`, `pane_send`, antrean dan pengiriman, log pesan, pengaman putaran
(MCP-01..05, MCP-07, MSG-01..06).

### Phase 3: Agen membuka agen
`pane_spawn`, kartu persetujuan, batas, status dari hook, pengaturan (SPN-01..04, SET-01..02). Alur B lulus ujung ke ujung.

### Phase 4: Kerja paralel
Worktree, `pane_report`, pemulihan tata letak, katalog pengguna, info login (MCP-06, SPN-05, TERM-06..07, AGT-05..06, MSG-07).

### Phase 5: Distribusi
Penandatanganan dan notarisasi, updater, ikon, halaman rilis.

## 10. Risiko dan mitigasi
| Risiko | Mitigasi |
|---|---|
| Pembaruan CLI mengubah perilaku TUI sehingga tempel/Enter atau deteksi status rusak | Status utama dari hook resmi, bukan membaca layar; uji cepat dengan CLI asli di setiap rilis; katalog berupa data agar mudah diperbaiki. |
| Prompt injection berantai: agen A menyuruh agen B melakukan hal berbahaya | Penanda pengirim di setiap pesan; anak tanpa flag bypass sehingga izin tetap ditanyakan ke orang; sakelar peer mati bawaan; `instructions` meminta agen memperlakukan pesan agen lain sebagai data. |
| Dua agen saling membalas tanpa henti dan menghabiskan kuota | Pengaman putaran (MSG-04), batas spawn (SPN-02). |
| Ketentuan provider berubah | Tidak menyentuh kredensial, binary tidak dimodifikasi; pantau halaman legal Claude Code dan Codex sebelum rilis publik. |
| Kurva belajar Rust | Logika inti kecil dan diuji unit; UI tetap React/TypeScript. |
| Port loopback dipindai aplikasi lain | Token per pane wajib; cek `Host`/`Origin`. |

## 11. Pertanyaan terbuka
1. Nama final dan bundle identifier.
2. Distribusi: hanya pribadi, atau rilis publik? (Mempengaruhi notarisasi dan pertanyaan Commercial Terms, lihat DECISIONS D-006.)
3. Apakah pesan peer perlu mode "inbox" (tanpa tempel) selain mode tempel? Ditunda sampai alur B dan C dipakai nyata.

## 12. Definition of done (per fitur)
Alur bekerja dengan CLI asli; galat dan status kosong ditangani; tes Rust dan TypeScript relevan lulus; tidak ada
token di log; config global agen tidak berubah; dokumen diperbarui; klaim sesuai kenyataan.
