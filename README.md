# agentdeck

Terminal desktop untuk menjalankan Claude Code, Codex, dan agen CLI lain berdampingan, dan membuat mereka saling
melempar prompt: satu agen membuka agen lain, memberi tugas, membaca layarnya, dan menerima hasilnya. Setiap agen
adalah CLI resmi dengan login milikmu; agentdeck hanya menyediakan terminal dan jembatan MCP lokal.

**Status:** Phase 0–4 selesai (MVP + kerja paralel). Distribusi pribadi saja untuk sekarang; rilis publik ditunda
(DECISIONS D-026). Titik lanjut: [docs/PROGRESS.md](docs/PROGRESS.md).

agentdeck tidak berafiliasi dengan, atau didukung oleh, Anthropic maupun OpenAI. Kamu memasang dan login sendiri ke
Claude Code dan Codex CLI; pemakaiannya tunduk pada ketentuan masing-masing provider. Agen yang saling memberi tugas
memakai kuota paketmu sendiri, dan untuk otomasi berat provider menganjurkan kunci API.

## Dokumen
| Berkas | Isi |
|---|---|
| [docs/PRD.md](docs/PRD.md) | Apa yang dibangun, prioritas, alur, roadmap |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Cara kerja: komponen, peluncuran agen, pengiriman pesan, status, keamanan |
| [docs/MCP-TOOLS.md](docs/MCP-TOOLS.md) | Kontrak tool MCP yang dipakai agen |
| [docs/UI-UX.md](docs/UI-UX.md) | Layar, interaksi, pintasan, keadaan galat, token desain |
| [docs/IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md) | Task per phase, urutan, kriteria selesai |
| [docs/PHASE-0.md](docs/PHASE-0.md) | Panduan spike kelayakan S1–S8 + `spikes/pty_probe.py` |
| [docs/SETUP.md](docs/SETUP.md) | Pasang alat, buat proyek Tauri, perintah harian |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Log keputusan arsitektur |
| [docs/RESEARCH.md](docs/RESEARCH.md) | Analisis T3 Code dan MXterm Pro |
| [AGENTS.md](AGENTS.md) | Aturan untuk coding agent (dibaca Codex; `CLAUDE.md` mengimpornya) |

## Build untuk dipakai sendiri
```bash
pnpm install
pnpm tauri build
```
Hasilnya `src-tauri/target/release/bundle/macos/agentdeck.app` (dan `.dmg` di `bundle/dmg/`). Salin `.app` ke
`/Applications`. Build tidak ditandatangani, jadi saat pertama dibuka macOS bisa meminta konfirmasi (klik kanan → Open).
Pembaruan: `git pull`, lalu build ulang.

## Mulai
Prasyarat dan langkah ada di [docs/SETUP.md](docs/SETUP.md). Phase 0 sudah selesai ([docs/PHASE-0.md](docs/PHASE-0.md));
lanjutkan dari [docs/PROGRESS.md](docs/PROGRESS.md) dan task di [docs/IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md).
