# agentdeck

Terminal desktop untuk menjalankan Claude Code, Codex, dan agen CLI lain berdampingan, dan membuat mereka saling
melempar prompt: satu agen membuka agen lain, memberi tugas, membaca layarnya, dan menerima hasilnya. Setiap agen
adalah CLI resmi dengan login milikmu; agentdeck hanya menyediakan terminal dan jembatan MCP lokal.

**Status:** Phase 0 hampir selesai, kerangka Tauri ada. Titik lanjut: [docs/PROGRESS.md](docs/PROGRESS.md).

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

## Mulai
Prasyarat dan langkah ada di [docs/SETUP.md](docs/SETUP.md). Urutan kerja: Phase 0 ([docs/PHASE-0.md](docs/PHASE-0.md)),
lalu task di [docs/IMPLEMENTATION-PLAN.md](docs/IMPLEMENTATION-PLAN.md).
