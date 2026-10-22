# Phase 0 — Panduan spike kelayakan

Tujuan: membuktikan delapan asumsi di `docs/ARCHITECTURE.md` §10 dengan CLI asli sebelum kode inti ditulis. Setiap
spike menghasilkan satu entri DECISIONS (lihat §Mencatat hasil). Spike yang gagal mengubah desain, bukan diakali.

Urutan yang disarankan: S1 → S4 → S5 → S7 (cukup Python + Rust kecil), lalu S8 → S2 → S3 (butuh server MCP), lalu S6
(butuh Tauri). Prasyarat: `docs/SETUP.md` §1. Semua spike memakai kuota agen sungguhan; prompt dibuat sependek mungkin.

## Persiapan
1. Catat versi: `claude --version`, `codex --version`.
2. Jalankan kedua agen sekali secara manual di `~/dev/agentdeck` dan setujui pertanyaan kepercayaan folder, supaya
   probe tidak tertahan di dialog itu. Keluar dari agen.
3. Folder hasil: `mkdir -p ~/dev/agentdeck/spikes/out` (diabaikan git, lihat `.gitignore`).

Alat: `spikes/pty_probe.py` (Python standar). Ia menjalankan program di PTY 40×120, menjawab query terminal dasar,
menempel satu prompt sebagai bracketed paste lalu Enter, menyimpan semua byte output, dan mencetak urutan kejadian
(OSC, bracketed paste, alternate screen, saat tempel). Sudah diuji terhadap `zsh -f -i` (tempelan dua baris dieksekusi
sebagai satu input).

```bash
python3 -I spikes/pty_probe.py --help
```

## S1 — Tempel + Enter
**Pertanyaan:** apakah bracketed paste lalu `\r` terkirim sebagai satu prompt utuh?

Jalankan agen dengan environment bersih (seperti aplikasi yang dibuka dari Finder), bukan environment terminal tempat
probe dijalankan; bila probe dijalankan dari sesi Claude Code, variabel `CLAUDE_CODE_*` ikut terbawa dan mengubah
perilaku agen. Teks `--expect` tidak boleh ada di dalam tempelan (TUI menampilkan ulang tempelan; probe menolaknya).

```bash
cd ~/dev/agentdeck
CLEAN=(env -i HOME="$HOME" USER="$USER" LOGNAME="$LOGNAME" SHELL="$SHELL" LANG=en_US.UTF-8 PATH="$(zsh -l -c 'printf %s "$PATH"')")
"${CLEAN[@]}" python3 -I spikes/pty_probe.py --log spikes/out/s1-claude.bin --paste-at 10 --duration 60 \
  --paste $'Line one is context.\nLine two is context.\nWhat is 1234+4321? Reply with only the number.' \
  --expect 5555 -- claude
"${CLEAN[@]}" python3 -I spikes/pty_probe.py --log spikes/out/s1-codex.bin --paste-at 10 --duration 60 \
  --paste $'Line one is context.\nLine two is context.\nWhat is 1234+4321? Reply with only the number.' \
  --expect 5555 -- codex
```
Ulangi dengan tempelan besar: buat berkas ±8 KB (`spikes/out/big.txt`, teks pengisi diakhiri "What is 2468+1357?
Reply with only the number.") lalu `--paste-file spikes/out/big.txt --expect 3825`.

**Lulus:** `FOUND` untuk keempat percobaan, dan layar akhir (renderer S7) menunjukkan satu prompt dan satu jawaban
(prompt tidak terpecah per baris).
**Bila gagal:** coba `--enter-delay 0.4`; catat jeda terkecil yang selalu berhasil. Bila tetap gagal, tempel tanpa
baris baru (ganti `\n` dengan spasi) dan catat sebagai batasan.

## S4 — Status Claude Code
**Pertanyaan:** sinyal apa yang dipancarkan Claude Code untuk working / idle / needs_you?

Claude memakai Program Status Protocol (OSC 7501, rev 0.4,
https://www.superlogical.com/rex/docs/build/program-status): ia mengirim query `OSC 7501;?` dan hanya melapor bila
terminal menjawab. `--osc7501` membuat probe menjawab query itu. `CLEAN` dari S1.

```bash
"${CLEAN[@]}" python3 -I spikes/pty_probe.py --osc7501 --log spikes/out/s4-claude-idle.bin --paste-at 10 --duration 45 \
  --paste 'What is 1234+4321? Reply with only the number.' --expect 5555 -- claude
# --permission-mode default hanya untuk spike: memaksa prompt izin (lebih ketat, bukan bypass). Izin tidak dijawab.
"${CLEAN[@]}" python3 -I spikes/pty_probe.py --osc7501 --log spikes/out/s4-claude-perm.bin --paste-at 10 --duration 50 \
  --paste 'Create a file named spikes/out/s4.txt containing hi' -- claude --permission-mode default
```
`msg` berisi base64; dekode untuk membaca alasannya.

**Lulus:** `state=working`, `state=done`/`idle`, dan `state=blocked` saat prompt izin muncul. Hasil: lulus (D-016).
**Bila gagal:** hook lewat `--settings` (`UserPromptSubmit`, `Stop`, `Notification` `permission_prompt`) sebagai cadangan,
dan periksa bahwa hook pengguna tetap berjalan.

## S5 — Status Codex
Codex memancarkan OSC 7501 tanpa query (terlihat di S1). Yang belum terlihat: `blocked` saat meminta izin.

```bash
# -s read-only -a on-request hanya untuk spike: memaksa permintaan izin. Izin tidak dijawab.
"${CLEAN[@]}" python3 -I spikes/pty_probe.py --osc7501 --log spikes/out/s5-codex-perm.bin --paste-at 10 --duration 90 \
  --paste 'Create a file named spikes/out/s5.txt containing hi' -- codex -s read-only -a on-request
```
**Lulus:** `state=blocked` (dengan `kind`) saat Codex meminta izin, dan `idle` saat turn selesai. Periksa layar akhir
(renderer S7): bila tertulis "Selected model is at capacity", percobaan tidak sah, ulangi nanti.
**Bila `blocked` tidak dipancarkan:** ulangi dengan `-c 'tui.notifications=["agent-turn-complete","approval-requested"]'
-c 'tui.notification_method="osc9"'` dan catat teks OSC 9 untuk permintaan izin.

## S7 — Layar headless dengan `vt100`
**Pertanyaan:** apakah `vt100` membaca layar kedua TUI dengan benar?

Sumber tersimpan di `spikes/s7-screen/`. Awalnya proyek Rust sekali pakai di luar repo:
```bash
cargo new --bin /tmp/agentdeck-s7 && cd /tmp/agentdeck-s7 && cargo add vt100@0.18
```
`src/main.rs`:
```rust
use std::num::NonZeroU16;

fn main() {
    let path = std::env::args().nth(1).expect("usage: agentdeck-s7 <raw log>");
    let bytes = std::fs::read(path).expect("read log");
    // same size as pty_probe.py; vt100 0.18 takes NonZeroU16 sizes
    let (rows, cols) = (NonZeroU16::new(40).unwrap(), NonZeroU16::new(120).unwrap());
    let mut parser = vt100::Parser::new(rows, cols, 1000);
    parser.process(&bytes);
    let screen = parser.screen();
    println!("bracketed_paste={} alternate_screen={}", screen.bracketed_paste(), screen.alternate_screen());
    println!("{}", screen.contents());
}
```
```bash
cargo run -q -- ~/dev/agentdeck/spikes/out/s1-claude.bin
cargo run -q -- ~/dev/agentdeck/spikes/out/s1-codex.bin
```
**Lulus:** teks yang tercetak sama dengan yang terlihat di terminal sungguhan pada akhir sesi (jawaban `5555` dan
kotak input), dan `bracketed_paste=true` sebelum agen keluar. Catatan: probe menghentikan agen di akhir, jadi rekam
dengan `--duration` yang cukup panjang dan perhatikan bahwa keluarnya agen bisa mematikan bracketed paste.

## S8 — Server MCP `rmcp` + axum
**Pertanyaan:** bisakah server Rust kecil mengenali pane dari token dan dipanggil oleh kedua agen?

Sumber spike tersimpan di `spikes/s8-mcp/` (bangun dengan `cargo build --manifest-path spikes/s8-mcp/Cargo.toml`).
Awalnya proyek sekali pakai `/tmp/agentdeck-s8` dengan `rmcp` (fitur `server`, `macros`, `transport-streamable-http-server`),
`axum`, `tokio`. Mulai dari contoh resmi di repo `modelcontextprotocol/rust-sdk`: `examples/servers/src/counter_streamhttp.rs`
dan `simple_auth_streamhttp.rs`. Kebutuhan minimum:
- bind `127.0.0.1:0`, cetak port;
- dua token tetap untuk spike (`tok-a`, `tok-b`) dari env, dipetakan ke `p1`/`p2`; tanpa token → 401;
- satu tool `whoami` yang mengembalikan pane dari token;
- `instructions` berisi satu kalimat yang mudah dikenali, mis. "SPIKE-INSTRUCTIONS-OK".

**Lulus:** `curl` tanpa token mendapat 401; `curl` dengan `Origin: http://evil.example` ditolak; kedua agen (S2/S3)
memanggil `whoami` dan mendapat pane yang benar; agen bisa mengutip kalimat instructions bila ditanya.

## S2 — Injeksi MCP ke Claude Code
Dengan server S8 berjalan di port `PORT`:
```bash
export AGENTDECK_MCP_URL=http://127.0.0.1:PORT/mcp AGENTDECK_TOKEN=tok-a
jq -S '{mcpServers, project: .projects[env.PWD].mcpServers}' ~/.claude.json > spikes/out/s2-before.json
claude --mcp-config '{"mcpServers":{"agentdeck":{"type":"http","url":"${AGENTDECK_MCP_URL}","headers":{"Authorization":"Bearer ${AGENTDECK_TOKEN}"}}}}' \
       --allowedTools mcp__agentdeck
```
Di dalam Claude: `/mcp` (harus tersambung), lalu minta "call the whoami tool". Keluar, lalu:
```bash
jq -S '{mcpServers, project: .projects[env.PWD].mcpServers}' ~/.claude.json > spikes/out/s2-after.json
diff spikes/out/s2-before.json spikes/out/s2-after.json && echo "MCP config unchanged"
```
`~/.claude.json` memang berubah di setiap sesi (statistik, riwayat proyek), jadi yang dibandingkan hanya bagian MCP.

**Lulus:** `whoami` → `p1` tanpa prompt izin; `MCP config unchanged`. **Bila `${VAR}` tidak diekspansi:** ulangi dengan
nilai literal di JSON dan catat (cadangan di ARCHITECTURE §4).

## S3 — Injeksi MCP ke Codex
```bash
export AGENTDECK_TOKEN=tok-b
shasum ~/.codex/config.toml > spikes/out/s3-before.txt
codex -c 'mcp_servers.agentdeck.url="http://127.0.0.1:PORT/mcp"' \
  -c 'mcp_servers.agentdeck.bearer_token_env_var="AGENTDECK_TOKEN"' \
  -c 'mcp_servers.agentdeck.default_tools_approval_mode="approve"'
```
Di dalam Codex: `/mcp` (harus terdaftar), lalu minta "call the whoami tool". Keluar, lalu
`grep -n agentdeck ~/.codex/config.toml || echo "no agentdeck entry"`. (Codex boleh menulis hal lain ke config, mis.
status kepercayaan folder; yang dicek hanya tidak ada entri agentdeck.)

**Lulus:** `whoami` → `p2` tanpa prompt; `no agentdeck entry`. Hasil: lulus (D-017). Bila layar akhir berisi
"Selected model is at capacity", percobaan tidak sah; ulangi atau pilih model lain dengan `-m` (khusus spike).

## S6 — Throughput PTY lewat Tauri Channel
Butuh proyek Tauri dari `docs/SETUP.md` §2. Spike di branch `spike/s6`, dibuang setelah dicatat:
- satu pane: `portable-pty` menjalankan `$SHELL -l`, thread baca mengirim byte lewat `tauri::ipc::Channel`
  (`InvokeResponseBody::Raw`), xterm.js menulis `Uint8Array`;
- `invoke("pty_write")` untuk ketikan, `invoke("pty_resize")` untuk ukuran.

Uji: `yes | head -c 50000000`, lalu `cat` berkas log 50 MB, sambil mengetik di pane kedua.
Sumber spike tersimpan di `spikes/s6-throughput/` (`lib.rs`, `App.tsx`; salin ke kerangka Tauri untuk menjalankan).
Jendela harus terlihat selama pengukuran: WebKit menahan timer dan rendering di jendela tersembunyi.

**Lulus:** UI tidak beku lebih dari 1 detik, ketikan di pane lain tetap < 50 ms (ukur dengan Performance panel Web
Inspector), memori stabil setelah selesai. Hasil: lulus dengan flow control (D-018).

## Mencatat hasil
Satu entri per spike di `docs/DECISIONS.md`:
```
## D-0xx — Phase 0 S1: tempel + Enter (YYYY-MM-DD)
Versi: Claude Code x.y.z, Codex x.y.z. Perintah: ... Hasil: lulus/gagal + angka (jeda Enter, ukuran tempelan).
Bukti: spikes/out/<berkas> (tidak di-commit). Dampak ke desain: ...
```
Bila hasil mengubah ARCHITECTURE, MCP-TOOLS, atau PRD, ubah dokumen itu di commit yang sama dan hapus tanda **[Phase 0]**
yang sudah terjawab. Phase 0 selesai bila semua spike lulus atau desainnya sudah disesuaikan dan dicatat.
