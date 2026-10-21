# Panduan setup pengembangan

macOS, Apple Silicon. Versi yang disebut adalah yang terbaru per 2026-10-21; versi lebih baru boleh selama tes lulus.

## 1. Prasyarat

| Alat | Cek | Pasang |
|---|---|---|
| Xcode atau Command Line Tools | `xcode-select -p` | `xcode-select --install` |
| Rust stable (rustup) | `rustc --version`, `cargo --version` | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh`, lalu buka terminal baru atau `source "$HOME/.cargo/env"` |
| Node.js 22+ | `node --version` | Sudah ada (v26) |
| pnpm | `pnpm --version` | Sudah ada (12.5) |
| Claude Code, Codex (untuk mencoba alur agen) | `claude --version`, `codex --version` | Sudah ada (2.1.303, 0.163.0); pastikan sudah login |
| jq (spike S2) | `jq --version` | Bawaan macOS |

Status mesin pemilik per 2026-10-21: semua ada. Rust 1.99.1 (clippy, rustfmt) dipasang lewat rustup; `~/.zprofile` memuat `~/.cargo/env`.

Setelah rustup:
```bash
rustup component add clippy rustfmt
```

## 2. Membuat proyek Tauri di repo yang sudah berisi docs
`create-tauri-app` membuat folder baru, sedangkan repo ini sudah berisi `docs/` dan README. Buat kerangka di folder
sementara, lalu salin tanpa menimpa berkas yang ada:

```bash
mkdir -p /tmp/agentdeck-scaffold && cd /tmp/agentdeck-scaffold
pnpm create tauri-app agentdeck -m pnpm -t react-ts --identifier local.agentdeck.desktop --tauri-version 2 -y
rsync -a --ignore-existing /tmp/agentdeck-scaffold/agentdeck/ ~/dev/agentdeck/
cd ~/dev/agentdeck && pnpm install
```
- `--ignore-existing` menjaga `README.md` dan `.gitignore` milik repo. Gabungkan isi `.gitignore` dari kerangka secara
  manual bila ada baris yang belum ada.
- `local.agentdeck.desktop` adalah identifier sementara (nama final masih pertanyaan terbuka PRD §11).

Coba jalankan:
```bash
pnpm tauri dev
```
Jendela contoh Tauri harus terbuka. Kompilasi Rust pertama butuh beberapa menit.

## 3. Dependency
Tambahkan saat task yang membutuhkannya dikerjakan (lihat `docs/IMPLEMENTATION-PLAN.md`), bukan sekaligus.

Rust (`src-tauri/`):
```bash
cd src-tauri
cargo add portable-pty@0.9 vt100@0.18
cargo add tokio --features full
cargo add axum
cargo add rmcp@3.6 --features server,macros,transport-streamable-http-server
cargo add serde --features derive
cargo add serde_json toml rand tracing tracing-subscriber
```
Frontend:
```bash
pnpm add @xterm/xterm @xterm/addon-fit @xterm/addon-webgl
pnpm add -D vitest
```
Tidak ada Tailwind atau pustaka state: token warna sebagai CSS variables (`docs/UI-UX.md` §6), state lewat React.
Tambah pustaka hanya dengan alasan yang dicatat di DECISIONS.

Skrip di `package.json` (tambahkan bila belum ada):
```json
{
  "scripts": {
    "typecheck": "tsc --noEmit",
    "test": "vitest run"
  }
}
```

## 4. Perintah harian

| Tujuan | Perintah |
|---|---|
| Jalankan aplikasi (hot reload UI, rebuild Rust) | `pnpm tauri dev` |
| Log Rust lebih rinci | `RUST_LOG=agentdeck=debug pnpm tauri dev` |
| Format Rust | `cargo fmt --manifest-path src-tauri/Cargo.toml` |
| Lint Rust | `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` |
| Tes Rust | `cargo test --manifest-path src-tauri/Cargo.toml` |
| Typecheck UI | `pnpm typecheck` |
| Tes UI | `pnpm test` |
| Build aplikasi | `pnpm tauri build` → `src-tauri/target/release/bundle/macos/agentdeck.app` |

Web Inspector untuk WebView: klik kanan di jendela saat `pnpm tauri dev` → Inspect Element.

## 5. Konfigurasi Tauri yang perlu diatur di awal
Di `src-tauri/tauri.conf.json`:
- Jendela: `minWidth` 900, `minHeight` 600, judul "agentdeck".
- `app.security.csp`: hanya sumber lokal, tanpa skrip/gaya dari jaringan, mis.
  `default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:`.
  `'unsafe-inline'` untuk gaya dibutuhkan xterm.js; jangan longgarkan `script-src`.
- `app.withGlobalTauri`: `false` (impor dari `@tauri-apps/api`).
- Jangan menambah plugin Tauri yang tidak dipakai (fs, shell, http). PTY dan server MCP ditulis di core sendiri.

## 6. Build untuk dipakai sendiri dan distribusi
- **Pribadi:** `pnpm tauri build`, lalu salin `.app` ke `/Applications`. Tanpa identitas penandatangan, macOS mungkin
  meminta konfirmasi saat pertama dibuka.
- **Distribusi (Phase 5):** butuh sertifikat Developer ID Application dan notarisasi. Tauri membaca
  `APPLE_SIGNING_IDENTITY` (atau `bundle.macOS.signingIdentity`), `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  dan untuk notarisasi `APPLE_ID` + `APPLE_PASSWORD` + `APPLE_TEAM_ID` atau `APPLE_API_ISSUER` + `APPLE_API_KEY` +
  `APPLE_API_KEY_PATH` (https://v2.tauri.app/distribute/sign/macos/). Updater: `tauri-plugin-updater`, kunci dari
  `tauri signer generate`, `TAURI_SIGNING_PRIVATE_KEY` di environment (bukan `.env`). Semua secret ini tidak pernah
  di-commit.

## 7. Masalah umum
| Gejala | Penyebab dan solusi |
|---|---|
| `cargo: command not found` setelah rustup | Terminal lama. Buka terminal baru atau `source "$HOME/.cargo/env"`. |
| Aplikasi bilang `claude`/`codex` tidak terpasang padahal ada | Aplikasi GUI tidak mewarisi `PATH` dari shell. Core mengambil `PATH` dari `$SHELL -l` saat mulai (ARCHITECTURE §3); pastikan `PATH` diatur di `~/.zprofile`/`~/.zshrc`, bukan hanya di terminal tertentu. |
| Agen tertahan di dialog "trust this folder" | Jalankan agen sekali secara manual di folder itu dan setujui. |
| Build Rust lambat | Normal untuk build pertama; build berikutnya inkremental. |
