# ⚡ TurboDL Ultra — Tauri Edition

A blazing-fast desktop video downloader for Linux, with a concurrent download queue and a companion Chrome extension.
Built with **Tauri 2**, **React 19**, **TypeScript**, **Tailwind CSS v4**, and a **Rust** backend that shells out to **yt-dlp**/**ffmpeg**, downloading several videos in parallel (each itself using 5× parallel chunk downloading).

---

## Architecture

```
┌──────────────────────────────────────────┐
│  Frontend  (React + TS + Vite)           │
│  App.tsx (shell) + state/QueueContext    │
│  components/{QueueList,panels/*}         │
└────────────────────┬──────────────────────┘
                      │ invoke() / listen()
                      │  (Tauri IPC bridge)
┌────────────────────▼──────────────────────┐
│  Backend  (Rust · Tauri 2)                │
│  src-tauri/src/lib.rs                     │
│    fetch_video_meta, validate_keyword_*,  │
│    start_keyword_download, update_yt_dlp  │
│    turbodl:// deep-link handling          │
│  src-tauri/src/queue.rs                   │
│    QueueState + dispatcher (bounded       │
│    worker pool, live max_concurrency,     │
│    per-host cap, pause/resume, cancel)    │
└────────────────────┬──────────────────────┘
                      │ std::process::Command
                      │ (one yt-dlp child per
                      │  running queue item)
┌────────────────────▼──────────────────────┐
│  CLI Engine  (yt-dlp + ffmpeg)            │
│  --concurrent-fragments 5 per item        │
│  --http-chunk-size 10 MB per item         │
└─────────────────────────────────────────────┘
                      ▲
                      │ turbodl://add?url=...
┌─────────────────────┴──────────────────────┐
│  extension/  (Chrome MV3)                  │
│  right-click a page/link → opens the       │
│  deep link → OS launches/focuses the app   │
└─────────────────────────────────────────────┘
```

See **[Concurrent Download Queue](#concurrent-download-queue)** below for how the queue engine works, and **[Browser Extension](#browser-extension)** for how to install and use the Chrome extension.

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
| `src/App.tsx` | App shell — header, mode toggle, active mode panel, Save-To, queue view |
| `src/state/QueueContext.tsx` | Queue state (reducer) + subscribes to the backend's per-item events |
| `src/components/QueueList.tsx`, `QueueItemRow.tsx` | The queue view — one row per download, with progress/pause/resume/cancel |
| `src/components/panels/{Single,Batch,Keyword}ModePanel.tsx` | The three "add to queue" input modes |
| `src/components/ui.tsx` | Shared UI primitives (buttons, progress bar, toast, mode toggle) |
| `src/index.css` | Global styles + Tailwind directives |
| `src-tauri/src/lib.rs` | Rust commands: fetch, folder/file pickers, keyword search, `turbodl://` deep-link handling |
| `src-tauri/src/queue.rs` | The concurrent download queue engine — see [Concurrent Download Queue](#concurrent-download-queue) |
| `src-tauri/src/main.rs` | Tauri entry point |
| `src-tauri/tauri.conf.json` | Window config, bundle targets, `plugins.deep-link` scheme registration — note `identifier` lives at the **top level**, not nested under `bundle` |
| `src-tauri/Cargo.toml` | Rust dependencies |
| `extension/` | The companion Chrome extension — see [Browser Extension](#browser-extension) |
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
| 🚀 Concurrent download queue | Several videos download in parallel (configurable 1-8 at once, adjustable live from the queue view), not just one at a time — see [Concurrent Download Queue](#concurrent-download-queue) |
| ⏸️ Pause / Resume | Pause any running download and resume it later — it picks up from where it left off (yt-dlp resumes partial files by default) |
| 📋 Bulk add from every mode | Single mode accepts several pasted URLs at once; Batch mode accepts a pasted list as well as a `.txt` file; Keyword mode queues every match — all three feed the same queue |
| 🧩 Chrome extension | Right-click a page or link in Chrome → "Send to TurboDL" — no copy-pasting. See [Browser Extension](#browser-extension) |
| 🔍 Zero-click auto-fetch | A single pasted URL → formats appear instantly (600ms debounce), no button needed |
| 📋 Clipboard monitor | Polls every 1.5 s; auto-pastes a copied video link (single mode only) |
| ⚡ 5× parallel chunks per file | `--concurrent-fragments 5` + 10 MB HTTP chunks, on top of the multi-file concurrency above |
| ⌕ Keyword downloads | Give a source URL (channel/playlist) + a keyword; yt-dlp's `--match-filters` (or a page-link scan as a fallback) finds matching videos, which are then queued and downloaded concurrently with everything else. Example: Source URL `https://www.youtube.com/@nasa/videos`, Keyword `mars` — queues that channel's videos with "mars" in the title. |
| 🎚️ Height-based format selection | `bestvideo[height=1080][ext=mp4]+bestaudio[ext=m4a]/...` — works across sites, unlike raw numeric format IDs |
| 📁 Native folder picker | Linux GTK directory dialog via `rfd` crate |
| 📊 Live per-item progress | Speed, ETA, file size, % per queue row — streamed line-by-line from each yt-dlp process, with stderr drained concurrently to avoid pipe-buffer deadlocks |
| 🦀 Rust backend | No Electron; Tauri uses `webkit2gtk` |

---

## Concurrent Download Queue

Every download — single, batch, or keyword — goes into one shared queue instead of running immediately. A background dispatcher (`src-tauri/src/queue.rs`) admits queued items up to a concurrency limit you control, so several videos download at once instead of one at a time.

### How it works

- **Max Concurrent** (top-right of the queue view, 1-8, default 3) is live — change it at any time, including while downloads are running. Raising it admits more queued items immediately; lowering it never kills anything already running, it just pauses new admissions until the running count drops back under the new limit.
- **Per-host cap.** Downloads from the *same site* are capped separately from the global limit — roughly half of Max Concurrent, rounded up (e.g. 2 at the default of 3, 4 at 8) — so a burst of same-host connections doesn't trigger a rate limit or a temporary ban. This still lets same-host batches (the most common case — e.g. several results from one keyword search) run in real parallel, just not *all* of them at once against Max Concurrent's full limit.
- **Per-host cooldown.** After a download for a given host finishes, that host gets a short (4-8s) breather before the next one from the same host starts — this replaces what used to be a single global pause between every download.
- **Disk-space check.** Before starting a new download, the app checks free space on its target drive (via `df`) and refuses to start (with a clear error on that row) if it's critically low, rather than risk a half-written file.
- **Pause / Resume.** Pausing a running item kills its yt-dlp process but keeps the row in the queue; Resume re-queues it, and yt-dlp resumes the partially-downloaded file by default — no data is re-downloaded from scratch. A paused item doesn't count against your concurrency limit, so it frees up a slot for something else immediately.
- **Cancel** removes an item for good (whether it was running, pending, or paused). **Cancel All** and **Clear Completed** act on the whole queue at once.

### Known limitation

Keyword search still auto-queues its top N matches rather than letting you pick individual videos from the full match list first — every match still lands as its own row and downloads in parallel with everything else, but there's no checkbox list to deselect a few before they're queued. Tracked in `project-docs/plans/parallel-download-queue-and-browser-extension.md`.

---

## Browser Extension

A small Chrome extension (`extension/`) lets you send a page or a link straight to TurboDL Ultra without copying and pasting a URL into the app.

### Install it (unpacked, not on the Chrome Web Store)

1. Open `chrome://extensions` in Chrome (or any Chromium-based browser — Edge, Brave, etc. use the same page at their own address).
2. Turn on **Developer mode** (top-right toggle).
3. Click **Load unpacked** and select this project's `extension/` folder.
4. The extension icon appears in your toolbar — pin it for easy access (puzzle-piece icon → pin).

TurboDL Ultra itself must already be installed and runnable (see Step 4 above) — the extension only sends the link; the desktop app is what actually registers the `turbodl://` link type with your system, which it does automatically the first time it launches.

### Using it

**From a right-click:**
- Right-click anywhere on a page → **"Send this page to TurboDL"** sends the current page's URL.
- Right-click a link → **"Send this link to TurboDL"** sends that specific link instead of the page you're on.

**From the toolbar button:** click the extension icon, then **"Send Current Tab"**.

Either way, your browser will ask **"Open TurboDL Ultra?"** the first time (and periodically after, unless you tell it not to) — this is your browser's own safety prompt for external links, not something from this app. Check **"Always allow turbo.downloader.app to open these links"** (wording varies by browser) if you don't want to see it every time. If TurboDL Ultra isn't already running, this same prompt launches it; if it's already open, sending a link just brings it to the front with the new item already appearing in the queue.

### What to expect

- The sent link is added to the queue with its default settings (best-available quality, saved to `~/Downloads`) — there's currently no way to pick a format or destination folder from the extension itself before it starts; adjust those from inside the app for anything already queued.
- A toast in the app confirms what happened — either "Added N links from the browser extension" or a specific rejection reason (e.g. the page's URL wasn't a supported `http(s)` link).
- Sending several links one at a time is fine — there's no limit on how many you queue this way. A dedicated "select multiple links on a page and send them all at once" flow doesn't exist yet (tracked as future work).

### If it's not working

- Make sure the desktop app has been launched at least once since installing/updating it — that's when it registers itself as the `turbodl://` handler with your system.
- On Linux, this registration writes a `.desktop` file under `~/.local/share/applications/` and needs the `xdg-mime`/`update-desktop-database` commands (present on virtually all desktop Linux installs). If your browser doesn't prompt to open the app at all, check those commands exist (`which xdg-mime update-desktop-database`).
- If your browser silently ignores the link instead of prompting, check whether you previously dismissed the "Open TurboDL Ultra?" prompt with an "always deny" style option — browsers remember that choice per-scheme and you'll need to reset it in your browser's site/protocol settings.

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
