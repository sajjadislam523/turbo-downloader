# ⚡ TurboDL Ultra — Tauri Edition

A blazing-fast, ultra-minimal desktop video downloader for Ubuntu/Linux.
Built with **Tauri 2**, **React 18**, **TypeScript**, **Tailwind CSS v4**, and a **Rust** backend that shells out to **yt-dlp** with 5× parallel chunk downloading.

---

## Architecture

```
┌─────────────────────────────────┐
│  Frontend  (React + TS + Vite)  │  ← src/App.tsx
│  Tailwind CSS · Custom fonts    │
└────────────────┬────────────────┘
                 │ invoke() / listen()
                 │  (Tauri IPC bridge)
┌────────────────▼────────────────┐
│  Backend  (Rust · Tauri 2)      │  ← src-tauri/src/lib.rs
│  open_directory_dialog          │
│  open_file_dialog               │
│  fetch_video_meta                │
│  start_turbo_download            │
│  start_batch_download            │
│  start_keyword_download          │
└────────────────┬────────────────┘
                 │ std::process::Command
┌────────────────▼────────────────┐
│  CLI Engine  (yt-dlp + ffmpeg)  │
│  --concurrent-fragments 5       │
│  --http-chunk-size 10 MB        │
└─────────────────────────────────┘
```

---

## Step 1 — System Prerequisites

```bash
# 1. Graphics / build libraries (required by Tauri + webkit2gtk)
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev build-essential curl wget file \
  libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev

# 2. FFmpeg (for stream merging) + Python
sudo apt install -y python3-pip ffmpeg

# 3. yt-dlp binary
sudo wget https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp \
     -O /usr/local/bin/yt-dlp
sudo chmod a+rx /usr/local/bin/yt-dlp

# 4. Node.js LTS
sudo apt install -y nodejs npm
# Or via nvm (recommended):
# curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.7/install.sh | bash
# nvm install --lts

# 5. Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Choose option 1 (default install), then:
source "$HOME/.cargo/env"
```

---

## Step 2 — Clone & Install

If you are starting fresh with the Tauri scaffold, run:

```bash
npm create tauri-app@latest turbo-downloader \
  -- --template react-ts
cd turbo-downloader
npm install
```

Then **replace** the generated files with the ones provided in this project:

| Path | What it does |
|---|---|
| `src/App.tsx` | Complete React UI — single / batch / keyword modes |
| `src/index.css` | Global styles + Tailwind directives |
| `src-tauri/src/lib.rs` | Rust commands: fetch, folder/file pickers, single/batch/keyword downloads |
| `src-tauri/src/main.rs` | Tauri entry point |
| `src-tauri/tauri.conf.json` | Window config, bundle targets — note `identifier` lives at the **top level**, not nested under `bundle` |
| `src-tauri/Cargo.toml` | Rust dependencies |
| `tailwind.config.js` | Tailwind + custom font/color tokens |
| `vite.config.ts` | Vite dev server on port 5173 |
| `index.html` | Loads Google Fonts (Syne, DM Sans, JetBrains Mono) |

---

## Step 3 — Run in Development Mode

```bash
npm run tauri dev
```

This spins up the Vite dev server and opens a live-reloading Tauri window.

---

## Step 4 — Production Build

```bash
npm run tauri build
```

Output `.deb` package:
```
src-tauri/target/release/bundle/deb/turbo-downloader_0.1.0_amd64.deb
```

Install it:
```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/turbo-downloader_*.deb
```

---

## Features

| Feature | Details |
|---|---|
| 🔍 Zero-click auto-fetch | URL typed/pasted → formats appear instantly (600ms debounce), no button needed |
| 📋 Clipboard monitor | Polls every 1.5 s; auto-pastes copied video links (single mode only) |
| ⚡ 5× parallel chunks | `--concurrent-fragments 5` + 10 MB HTTP chunks |
| 📄 Batch downloads | Load a `.txt` file (one URL per line, `#` for comments); per-video sleep interval avoids rate-limiting |
| ⌕ Keyword downloads | Give a source URL (channel/playlist) + a keyword; yt-dlp's `--match-filters` downloads only videos whose title matches |
| 🎚️ Height-based format selection | `bestvideo[height=1080][ext=mp4]+bestaudio[ext=m4a]/...` — works across sites, unlike raw numeric format IDs |
| 📁 Native folder picker | Ubuntu GTK directory dialog via `rfd` crate |
| 📊 Live progress stream | Speed, ETA, file size, % — streamed line-by-line from yt-dlp stdout, with stderr drained concurrently to avoid pipe-buffer deadlocks |
| 🦀 Rust backend | No Electron; Tauri uses `webkit2gtk` — ~40 MB RAM footprint |

---

## Troubleshooting

**`yt-dlp: command not found`**
Re-run Step 1 item 3 and verify `/usr/local/bin/yt-dlp` is executable.

**`libwebkit2gtk-4.1-dev` not found**
On older Ubuntu (< 22.04) try `libwebkit2gtk-4.0-dev`. Update `Cargo.toml`:
`tauri = { version = "2", features = ["protocol-asset"], ... }` — Tauri 2 requires 4.1.

**Clipboard auto-paste not working**
The Clipboard API requires a secure context. In `tauri dev`, it works out of the box. In production, no extra config needed.

**FFmpeg merge fails**
Ensure `ffmpeg` is installed: `ffmpeg -version`. The format selectors require FFmpeg to be present for merging separate video/audio streams.

**Save path defaults to `~/Downloads` but files land in a folder literally named `~`**
Fixed — `build_output_template()` now expands a leading `~` against `$HOME` before handing the path to `yt-dlp`, since `Command::new()` never goes through a shell and won't expand it for you.

**Downloads with lots of yt-dlp warnings hang forever**
Fixed — stderr is now drained on its own thread concurrently with stdout. Previously stderr was only read after the process exited, so a full OS pipe buffer could block yt-dlp's `write()` call indefinitely.
