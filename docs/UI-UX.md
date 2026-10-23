# agentdeck — Spesifikasi UI/UX

Aplikasi desktop macOS untuk developer: padat, cepat, dikendalikan keyboard. Terminal adalah isi utama; semua kontrol
lain kecil dan tenang supaya tidak bersaing dengan output agen. Persyaratan yang dirujuk (TERM-, MSG-, SPN-, ...) ada di
`docs/PRD.md`.

Bahasa UI: Indonesia (pengguna tunggal). Teks yang dibaca agen (penanda pesan, instructions MCP) tetap Inggris
(`docs/MCP-TOOLS.md`).

## 1. Prinsip
1. **Terminal dulu.** Chrome aplikasi (sidebar, header, panel) memakai ruang sekecil mungkin; tidak ada dekorasi.
2. **Tidak pernah mencuri fokus.** Kartu, toast, dan pesan masuk tidak memindahkan fokus keyboard dari pane tempat orang
   mengetik. Ketikan orang tidak boleh berakhir di tempat yang salah.
3. **Status selalu terbaca tanpa warna.** Setiap titik status disertai teks atau ikon berbeda.
4. **Setiap aksi agen terlihat.** Spawn, pesan, dan jeda tampil di tempat yang jelas dan bisa dilacak di log.
5. **Keyboard penuh.** Semua aksi punya pintasan atau bisa dicapai dengan Tab; fokus selalu terlihat.

## 2. Anatomi jendela

Acuan visual: prototipe Claude Design "agentdeck Prototype" (D-015). Bila prototipe dan dokumen ini berbeda, dokumen
ini yang berlaku.

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●            │ api  ~/dev/app                                              [≡ Pesan]│ title bar 32px
├──────────────────┼───────────────────────────────────────┬──────────────────────────────┤
│ WORKSPACE      + │ ● p1 · Claude Code · feat: auth  ⊟ ⊞ ✕ │ PESAN · api      [Semua ▾]   │
│ ▣ api   2 pane   │                                       │ 14:02 p1 → p2  terkirim      │
│ ▣ web  ● butuh   │  (xterm.js)                           │   Review the diff on…        │
│                  ├───────────────────────────────────────┤ 14:09 p2 → p1  menunggu      │
│ PANE · api       │ ◐ p2 · Codex · review: auth  anak p1  │   RESULT status=done…        │
│ ● p1 Claude Code │   1 pesan                       ⊟ ✕   │                              │
│ └ ◐ p2 Codex   1 │  (xterm.js)                           │                              │
│ ⚙ Settings       │                                       │                              │
├──────────────────┴───────────────────────────────────────┴──────────────────────────────┤
│ MCP aktif · Agen membuka pane: aktif · Saling kirim: mati · Pesan antar agen 3/12        │ status bar 24px
└──────────────────────────────────────────────────────────────────────────────────────────┘
```

| Area | Ukuran | Isi |
|---|---|---|
| Title bar | 32 px | Nama dan folder workspace aktif, tombol panel Pesan. Lampu jendela macOS tetap native. |
| Sidebar | 200 px, bisa diciutkan (`⌘B`) | **Workspace:** satu baris per workspace (nama, folder, jumlah pane), titik amber + "butuh kamu" bila ada pane `needs_you`, tombol `+`. **Pane:** pohon pane workspace aktif, anak menjorok di bawah induknya, dengan titik status dan jumlah pesan menunggu; klik memfokuskan pane. Tautan Settings di bawah. Otomatis diciutkan bila jendela < 1100 px. |
| Workspace | sisa | Pohon split pane. |
| Panel Pesan | 320 px, bisa disembunyikan | Log pesan workspace aktif (§4.4). Otomatis tersembunyi bila jendela < 1300 px. |
| Status bar | 24 px | Status server MCP, sakelar workspace, penghitung pengaman putaran. Setiap item bisa diklik. |

Ukuran jendela minimum 900 × 600. Pane minimum 40 kolom × 8 baris; split yang melanggar ditolak dengan pesan singkat.

## 3. Pane

### Header (28 px)
Urutan: titik status + label status · id pane · agen · judul · folder (mono, dipendekkan dari tengah) · lencana relasi
(`anak p1`) · chip `N pesan` bila ada pesan menunggu · tombol split kanan, split bawah, perbesar, tutup.

Bila header sempit, urutan pemendekan: folder (dari tengah), lalu judul (elipsis di akhir), lalu folder disembunyikan.
Titik status, id pane, dan nama agen tidak pernah dipotong.

| Status | Titik | Label |
|---|---|---|
| `starting` | lingkaran kosong | memulai |
| `working` | setengah terisi, berdenyut (dimatikan oleh reduced motion) | bekerja |
| `idle` | penuh hijau | siap |
| `needs_you` | penuh amber + ikon `!` | menurut `kind` OSC 7501: butuh izin / butuh jawaban / perlu login; tanpa `kind`: butuh kamu |
| `exited` | abu-abu bergaris | selesai (kode N) |

Bila agen melapor `error`, titik tetap `idle` dengan ikon galat kecil. `msg` dari agen (sudah didekode dan dibatasi)
muncul sebagai tooltip pada label status. Codex tidak melapor galat (ARCHITECTURE §6), jadi ikon ini hanya untuk Claude.
| `unknown` | abu-abu kosong | tidak diketahui |

Pane terfokus: garis tepi 1 px warna aksen; header pane lain memakai teks `muted`.

### Badan
xterm.js mengisi penuh. Font dan ukuran dari Settings. Tidak ada animasi di dalam terminal.

### Keadaan khusus
| Keadaan | Tampilan |
|---|---|
| Proses keluar | Pita di bawah pane: "Claude Code selesai (kode 0)" + tombol **Jalankan ulang** dan **Buka shell di sini**. Isi terminal tetap bisa digulir dan disalin. |
| Dipulihkan (T4.3, D-025) | Setelah aplikasi dibuka lagi: label status "belum dijalankan", pita "Claude Code belum dijalankan" + **Jalankan** dan **Buka shell di sini** (pane shell hanya **Jalankan**). Tidak ada proses yang jalan sendiri. |
| Gagal dijalankan | Badan pane berisi pesan galat (mis. "Tidak bisa menjalankan codex: izin ditolak") + **Coba lagi**. |
| Pesan menunggu | Chip `1 pesan` di header; hover menampilkan pratinjau 3 baris; klik membuka panel Pesan pada pesan itu. |
| Pesan sedang ditempel | Kilatan halus 150 ms di garis tepi pane (opacity saja). |

Tidak ada sorotan atau dekorasi di dalam isi terminal untuk pesan masuk. Claude Code dan Codex memakai alternate
screen dan menggambar ulang layarnya sendiri (S1, D-013), sehingga dekorasi yang ditempel ke baris xterm.js bergeser
atau hilang. Pesan dikenali dari baris penanda `[agentdeck] message from …` yang ikut tertempel, chip header, kilatan
tepi, dan panel Pesan (D-015).

### Menutup pane (SPN-04)
`⌘W` atau tombol tutup. Bila proses masih berjalan: dialog kecil "Hentikan Codex di p2?" dengan **Hentikan** (destruktif)
dan **Batal** (bawaan, terfokus). Pane yang punya anak hidup menyebutkan jumlah anak; anak tidak ikut ditutup.

## 4. Permukaan lain

### 4.1 Peluncur (workspace baru, split, agen baru)
Popover di tengah pane tujuan, dibuka `⌘T`, `⌘D`, `⌘⇧D`, atau `+`.
- Pilihan: **Shell**, **Claude Code**, **Codex** (P1: agen dari katalog pengguna). Agen yang tidak terpasang tampil
  redup dengan teks "Belum terpasang" dan tombol salin perintah pasang.
- Folder: bawaan folder pane aktif; daftar 5 folder terakhir; tombol pilih folder (dialog native).
- Tugas awal (opsional, hanya agen): textarea mono 4 baris.
- `Enter` meluncurkan (di textarea: `⌘Enter`), `Esc` menutup. Panah atas/bawah memilih agen.

### 4.2 Kartu persetujuan spawn (SPN-01)
Muncul di bagian atas pane peminta, menutupi baris teratas terminal, **tanpa mengambil fokus**.

```
┌────────────────────────────────────────────────────────────────┐
│ p1 · Claude Code ingin membuka agen                    1:47 ▁▁▁│
│ Codex · "review: auth" · ~/dev/app                             │
│ ┌────────────────────────────────────────────────────────────┐ │
│ │ Review the diff on branch feat/auth for bugs. Do not edit… │ │
│ │ (gulir, maks 8 baris, mono)                                │ │
│ └────────────────────────────────────────────────────────────┘ │
│                                    [Tolak]  [Izinkan ⌘↩]       │
└────────────────────────────────────────────────────────────────┘
```
- Hitung mundur sesuai batas waktu kartu di Settings (bawaan 120 detik) sebagai teks dan bar tipis; habis = ditolak, kartu berubah menjadi "Kedaluwarsa" 3 detik lalu hilang.
- Keyboard: `⌘J` memindahkan fokus ke kartu tertunda tertua; di kartu, `⌘Enter` mengizinkan, `Esc` menolak, `Tab`
  berpindah tombol. Tanpa `⌘J`, ketikan tetap ke terminal.
- Beberapa kartu bertumpuk, yang tertua di atas.
- Bila jendela tidak fokus: notifikasi macOS "Claude Code (p1) ingin membuka Codex"; klik membawa ke kartu.
- `role="alertdialog"`, judul dan isi diumumkan lewat `aria-live="polite"`.

### 4.3 Pita pengaman putaran (MSG-04)
Pita amber di atas workspace, di bawah title bar:
"Agen sudah saling mengirim N pesan tanpa masukanmu. Antrean dijeda." (N dari Settings, bawaan 12) · **Lanjutkan** · **Hentikan dan buang antrean** · **Lihat log**.
Tidak mengambil fokus; `⌘J` juga menjangkau pita ini.

### 4.4 Panel Pesan (MSG-05)
Daftar terbaru di bawah. Setiap baris:
`14:09` (mono) · `p2 → p1` (chip id pane, tooltip nama agen dan judul) · status · baris pertama isi.

| Status | Tampilan |
|---|---|
| `queued` | "menunggu" + lama menunggu |
| `delivered` | "terkirim" |
| `paused` | "dijeda" (amber) |
| `dropped` | "dibuang" (merah) + alasan saat dibuka |

- Klik baris: isi lengkap (mono, bisa dipilih), tombol **Salin**. Baris `queued`/`paused`: **Buang**; P1: **Kirim sekarang**.
- Filter: Semua / Menunggu.
- Kosong: "Belum ada pesan antar agen di workspace ini. Pesan muncul saat agen memakai pane_send."

### 4.5 Status bar
- `MCP aktif` (hijau) / `MCP mati` (merah, klik: alasan + **Coba lagi**). Port tidak ditampilkan.
- `Agen membuka pane: aktif|mati`: klik membuka Settings › Kolaborasi.
- `Saling kirim: aktif|mati`: klik langsung mengubah sakelar workspace (dengan konfirmasi saat menyalakan).
- `Pesan antar agen n/N` (N dari Settings, bawaan 12): berubah amber di ≥ 75%.

### 4.6 Settings (`⌘,`, modal 640 × 520 di dalam jendela utama)
Navigasi kiri per bagian; `Esc` menutup; fokus kembali ke pane sebelumnya.

| Bagian | Isi |
|---|---|
| Agen | Per agen: terdeteksi/tidak, path, versi; P1: metode login dari CLI dan peringatan `ANTHROPIC_API_KEY`; P1: path manual. |
| Kolaborasi | Sakelar global "Agen boleh membuka pane" (bawaan mati); bawaan "Saling kirim" untuk workspace baru; angka batas dengan rentang dari MCP-TOOLS (stepper, nilai di luar rentang ditolak di tempat). |
| Terminal | Font, ukuran, tinggi baris, scrollback, tema (Sistem/Gelap/Terang), mode pembaca layar xterm.js. |
| Tentang | Versi, tautan ke docs, tombol buka folder log. |

Perubahan tersimpan langsung; tidak ada tombol Simpan.

### 4.7 Keadaan awal (tanpa workspace)
Di tengah jendela: judul "Mulai kerja", tiga tombol besar **Shell**, **Claude Code**, **Codex** dengan status terpasang,
dua baris penjelasan ("Buka beberapa agen berdampingan. Agen bisa saling memberi tugas setelah kamu mengizinkannya di
Settings."), dan tautan ke Settings › Kolaborasi.

## 5. Galat dan umpan balik

| Situasi | Di mana | Teks |
|---|---|---|
| Server MCP gagal mulai | Status bar merah + pita sekali | "Agen tidak bisa saling kirim: server lokal gagal mulai." **Coba lagi** · **Lihat log** |
| CLI tidak ditemukan | Peluncur / badan pane | "codex tidak ditemukan di PATH." + perintah pasang yang bisa disalin |
| Spawn ditolak karena sakelar mati | Toast 4 detik di pane peminta | "p1 meminta membuka pane, tapi fitur ini mati." **Settings** |
| Spawn kena batas | Toast di pane peminta | "p1 mencapai batas 3 anak." |
| Pesan dibuang | Panel Pesan (merah) | Alasan: "agen tujuan sudah keluar", "bracketed paste mati", "dibuang olehmu" |
| Proses gagal dihentikan | Toast | "Codex di p2 tidak berhenti; dipaksa berhenti." |

Toast: pojok kanan bawah jendela, maksimal 3, hilang 4 detik, bisa ditutup, `aria-live="polite"`, tidak mengambil fokus.

## 6. Token desain
Didefinisikan sebagai CSS variables di `src/styles/tokens.css`; komponen hanya memakai token, bukan hex. Kontras dihitung
dengan rumus WCAG 2.x (2026-10-21).

### Warna
| Token | Gelap | Terang | Pakai |
|---|---|---|---|
| `--bg` | `#0E1116` | `#FFFFFF` | Latar terminal dan jendela |
| `--surface` | `#151922` | `#F5F6F8` | Title bar, sidebar, header pane, panel, status bar |
| `--raised` | `#1C212C` | `#FFFFFF` | Kartu, popover, toast |
| `--line` | `#262C38` | `#D9DDE3` | Garis pemisah 1 px |
| `--fg` | `#E6E9EF` | `#1A1D23` | Teks utama |
| `--muted` | `#A1A9B8` | `#525A68` | Teks sekunder |
| `--faint` | `#868FA0` | `#646C7A` | Label kecil, waktu |
| `--accent` | `#5B9DFF` | `#1A63D4` | Fokus, aksi utama, status `working` |
| `--ok` | `#3FB950` | `#1A7F37` | `idle`, terkirim |
| `--warn` | `#D29922` | `#8A5C00` | `needs_you`, dijeda, pengaman putaran |
| `--danger` | `#F26D6D` | `#CF222E` | Galat, dibuang, aksi destruktif |

Kontras terendah tiap token teks terhadap `bg`/`surface`/`raised`:
- Gelap: `fg` 13.3, `muted` 6.8, `faint` 4.95, `accent` 5.9, `ok` 6.3, `warn` 6.4, `danger` 5.5.
- Terang: `fg` 15.6, `muted` 6.4, `faint` 4.9, `accent` 5.1, `ok` 4.7, `warn` 5.4, `danger` 4.95.

Semua ≥ 4.5:1. Teks di tombol aksen: `--bg` di atas aksen gelap (6.9), putih di atas aksen terang (5.6). Warna status
tidak pernah dipakai sendirian (§3 tabel status). Tema terminal (16 warna ANSI) terpisah dan bisa diatur; bawaannya
mengikuti `--bg`/`--fg`.

### Tipografi
| Token | Nilai | Pakai |
|---|---|---|
| `--font-ui` | `-apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif` | Semua UI |
| `--font-mono` | `"SF Mono", Menlo, monospace` | Terminal (bawaan, bisa diganti), id pane, waktu, path, isi pesan |
| UI dasar | 13 px / 18 px | Ukuran standar aplikasi macOS |
| Label | 11–12 px, mono untuk data | Header pane, status bar, waktu |
| Judul | 13 px semibold | Judul kartu dan bagian Settings |
| Terminal | 13 px, tinggi baris 1.2 | Bisa diatur 10–24 px |

Font sistem saja: tidak ada font dari jaringan (prinsip "lokal saja") dan tidak ada berkas font yang perlu dibundel.
Ukuran 13 px menyimpang dari saran web 16 px karena ini aplikasi desktop macOS yang padat; teks di bawah 11 px tidak dipakai.

### Bentuk, jarak, gerak
- Grid 4 px. Padding header 0 8 px; kartu 12 px; jarak antar kontrol 8 px.
- Radius 4 px; kartu dan popover 8 px.
- Lapisan dibedakan oleh warna permukaan dan garis `--line`. Bayangan hanya untuk kartu, popover, dan toast.
- Gerak hanya `opacity` dan `transform`, 120–160 ms, `cubic-bezier(0.2, 0, 0, 1)`; keluar lebih cepat dari masuk.
  `prefers-reduced-motion: reduce` mematikan semuanya, termasuk denyut status.
- Ikon SVG satu set (mis. Lucide, dibundel lokal); tidak ada emoji sebagai ikon.

## 7. Pintasan keyboard

| Aksi | Pintasan |
|---|---|
| Workspace baru (peluncur) | `⌘T` |
| Split kanan / bawah (peluncur) | `⌘D` / `⌘⇧D` |
| Tutup pane | `⌘W` |
| Pindah fokus pane | `⌘⌥` + panah |
| Pindah workspace | `⌘1`–`⌘9`, `⌘⇧[` / `⌘⇧]` |
| Perbesar / kembalikan pane | `⌘⇧Enter` |
| Panel Pesan | `⌘⇧M` |
| Ciutkan / tampilkan sidebar | `⌘B` |
| Ke kartu/pita tertunda | `⌘J` |
| Izinkan / tolak (fokus di kartu) | `⌘Enter` / `Esc` |
| Cari di pane | `⌘F` |
| Ukuran font | `⌘=` / `⌘-` / `⌘0` |
| Settings | `⌘,` |

Semua pintasan memakai `⌘`, jadi tidak bentrok dengan `Ctrl`/`⌥` yang dipakai program di terminal. Saat fokus di
terminal, `⌘C`/`⌘V` menyalin/menempel; tempelan orang memakai bracketed paste xterm.js seperti biasa.

## 8. Aksesibilitas
- Fokus terlihat (cincin 2 px `--accent`, offset 2 px) di setiap kontrol, termasuk dalam kartu dan popover.
- Urutan Tab: sidebar → pane terfokus → panel Pesan → status bar.
- Kartu `role="alertdialog"`; toast dan perubahan status penting diumumkan `aria-live="polite"`.
- Tombol ikon punya `aria-label` dan tooltip ("Split kanan"), termasuk item sidebar dan status bar (prototipe D-015
  belum memenuhi ini; wajib saat implementasi).
- Opsi mode pembaca layar xterm.js di Settings › Terminal.
- Target klik minimal 24 × 24 px (aplikasi desktop dengan pointer; header pane 28 px).

## 9. Diperiksa sebelum setiap layar dianggap selesai
- Tema gelap dan terang, keduanya dengan kontras token di atas.
- Jendela 900 × 600 (minimum) dan 1440 × 900: tidak ada teks terpotong tanpa elipsis, tidak ada scroll horizontal di chrome.
- Semua aksi bisa dilakukan tanpa mouse.
- Kartu dan pesan masuk tidak memindahkan fokus saat mengetik di terminal.
- Reduced motion: tidak ada gerakan.
- Keadaan kosong, memuat, galat, dan berhasil ada untuk setiap fitur interaktif.
