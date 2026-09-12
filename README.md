# ⚡ TurboDL Ultra

**A fast, native desktop video downloader built on Tauri — a concurrent download queue, keyword-based channel scraping, and a companion Chrome extension, wrapped around `yt-dlp` and `ffmpeg`.**

![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-19-61DAFB?logo=react&logoColor=black)
![TypeScript](https://img.shields.io/badge/TypeScript-5.8-3178C6?logo=typescript&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-2021-000000?logo=rust&logoColor=white)
![Platform](https://img.shields.io/badge/platform-Linux-informational)
![License](https://img.shields.io/badge/license-MIT-green)

Built with **Tauri 2**, **React 19**, **TypeScript**, **Tailwind CSS v4**, and a **Rust** backend that shells out to **yt-dlp**/**ffmpeg** — no Electron, no Chromium bundled with the app, just `webkit2gtk`. Several videos download in parallel through a bounded worker-pool queue, and each individual file itself uses 5× parallel chunk downloading via `yt-dlp`.

---

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Architecture](#architecture)
- [Platform Support](#platform-support)
- [Getting Started](#getting-started)
- [Running It as an Installed App on Your PC](#running-it-as-an-installed-app-on-your-pc)
- [How to Use](#how-to-use)
- [Concurrent Download Queue](#concurrent-download-queue)
- [Browser Extension](#browser-extension)
- [Troubleshooting](#troubleshooting)
- [Roadmap](#roadmap)
- [Contributing](#contributing)
- [License](#license)

---

## Overview

TurboDL Ultra is a small, self-contained desktop app for downloading videos — the kind of tool you reach for instead of a browser extension that phones home or a website full of ads. Paste one link, paste a hundred, drop in a `.txt` file of URLs, or point it at a channel/playlist and a keyword and let it find every matching video for you (e.g. source `https://www.youtube.com/@nasa/videos` + keyword `mars`). Everything lands in one shared download queue with live progress, pause/resume, and adjustable concurrency, and a companion Chrome extension lets you send a link straight from the browser without copy-pasting.

It exists to be **fast** (parallel downloads at two levels — several files at once, and multiple chunks per file), **transparent** (thin Rust wrapper around `yt-dlp`, nothing hidden), and **light** (a native WebView, not a bundled Chromium).

---

## Features

| Feature | Details |
|---|---|
| 🚀 Concurrent download queue | Several videos download in parallel (configurable 1–8 at once, adjustable live from the queue view) instead of one at a time — see [Concurrent Download Queue](#concurrent-download-queue) |
| ⏸️ Pause / Resume | Pause any running download and resume it later — it picks up from where it left off (`yt-dlp` resumes partial files by default) |
| 📋 Three input modes, one queue | **Single** (paste one or several URLs, per-video format picker), **Batch** (paste a list or load a `.txt` file), **Keyword** (channel/playlist + keyword → auto-discovers matches) — all three feed the same queue |
| 🧩 Chrome extension | Right-click a page or link in Chrome → "Send to TurboDL" — no copy-pasting. See [Browser Extension](#browser-extension) |
| 🔍 Zero-click auto-fetch | A single pasted URL → formats appear instantly (600ms debounce), no button needed |
| 📋 Clipboard monitor | Polls every 1.5s and auto-fills a freshly copied video link (single mode only) |
| ⚡ 5× parallel chunks per file | `--concurrent-fragments 5` + 10 MB HTTP chunks, on top of the multi-file concurrency above |
| ⌕ Keyword downloads | Give a source URL (channel/playlist) + a keyword; `yt-dlp`'s `--match-filters` (or an automatic page-link scan when a site has no listing-page extractor at all) finds matching videos, which are then queued and downloaded concurrently with everything else |
| 🎚️ Height-based format selection | `bestvideo[height=1080][ext=mp4]+bestaudio[ext=m4a]/...` — works across sites, unlike raw numeric format IDs, and always merges/remuxes to MP4 |
| 📁 Native folder picker | GTK directory dialog via the cross-platform `rfd` crate |
| 📊 Live per-item progress | Speed, ETA, file size, % per queue row — streamed line-by-line from each `yt-dlp` process, with stderr drained concurrently to avoid pipe-buffer deadlocks |
| 🔄 Self-updating `yt-dlp` | One click updates the `yt-dlp` binary in place, with clear guidance if it's not user-writable |
| 🦀 Rust backend | No Electron; Tauri uses `webkit2gtk`, so the shipped app is a fraction of the size of a Chromium-bundling alternative |

**Not yet supported** — audio-only (MP3/M4A) extraction, subtitle download/embedding, thumbnail/metadata embedding, and a queue that survives an app restart. All four are tracked in [Roadmap](#roadmap).

---

## Architecture

```
┌──────────────────────────────────────────┐
│  Frontend  (React + TS + Vite)            │
│  App.tsx (shell) + state/QueueContext     │
│  components/{QueueList,panels/*}          │
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
│  CLI Engine  (yt-dlp + ffmpeg)             │
│  --concurrent-fragments 5 per item         │
│  --http-chunk-size 10 MB per item          │
└─────────────────────────────────────────────┘
                      ▲
                      │ turbodl://add?url=...
┌─────────────────────┴──────────────────────┐
│  extension/  (Chrome MV3)                  │
│  right-click a page/link → opens the       │
│  deep link → OS launches/focuses the app   │
└─────────────────────────────────────────────┘
```

See [Concurrent Download Queue](#concurrent-download-queue) for how the queue engine works, and [Browser Extension](#browser-extension) for how to install and use the Chrome extension.

---

## Platform Support

| Platform | Status |
|---|---|
| 🐧 Linux (X11 or Wayland+XWayland) | ✅ Supported and tested — the only platform this app ships and is verified on today |
| 🪟 Windows | 🧭 Planned, not yet built — see [`project-docs/plans/windows-port.md`](project-docs/plans/windows-port.md) |
| 🍎 macOS | Not planned; icon assets exist in the tree from Tauri's scaffolding but nothing is wired up or tested |

**Linux runs under a forced X11 backend.** `src-tauri/src/main.rs` sets `GDK_BACKEND=x11` before Tauri starts — native Wayland triggers a known WebKit/GTK3 event-loop stall that makes the window freeze ("not responding"). This means the app needs **XWayland present** even on a Wayland session; it cannot run on a pure-Wayland compositor with no XWayland fallback (most desktop distros ship XWayland by default, so this is rarely something you need to think about). See `project-docs/DECISIONS.md` ADR-003 for the full investigation.

The Windows/macOS icon files (`icon.ico`, `icon.icns`, Windows Store tile PNGs) already sitting in `src-tauri/icons/` are Tauri scaffold leftovers, not evidence of a working build — only the Linux `.deb` bundle target is currently wired up in `tauri.conf.json`.

---

## Getting Started

### Step 1 — System Prerequisites (Linux/Ubuntu)

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

`curl` is also used at runtime — it's the fallback mechanism keyword search uses to scan a page's links when a site has no `yt-dlp` extractor at all for its listing pages.

### Step 2 — Clone & Install

```bash
git clone https://github.com/sajjadislam523/turbo-downloader.git
cd turbo-downloader
npm install
```

<details>
<summary>Starting from scratch instead of cloning?</summary>

If you're bootstrapping a fresh Tauri scaffold and mapping this project's files onto it:

```bash
npm create tauri-app@latest turbo-downloader -- --template react-ts
cd turbo-downloader
npm install
```

Then replace the generated files with this project's versions:

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

</details>

### Step 3 — Run in Development Mode

```bash
npm run tauri dev
```

This spins up the Vite dev server and opens a live-reloading Tauri window. (`npm run dev` alone only starts the Vite server with no Tauri window — `invoke()` calls won't work there.)

### Step 4 — Production Build

```bash
npm run tauri build
```

This produces a `.deb` package under:
```bash
ls src-tauri/target/release/bundle/deb/
```

---

## Running It as an Installed App on Your PC

Once you have a `.deb` from Step 4 above, install it like any other package:

```bash
sudo dpkg -i src-tauri/target/release/bundle/deb/turbo-downloader_*_amd64.deb
```

Tauri's bundler registers a desktop entry and icon for you automatically, so **"Turbo Video Downloader Ultra"** should now show up in your application launcher (GNOME Activities, KDE's app menu, etc.) just like any natively installed app — no terminal needed to run it day-to-day after this. If the icon/entry doesn't appear immediately, log out and back in, or run `update-desktop-database ~/.local/share/applications 2>/dev/null; sudo update-desktop-database` to force a refresh.

A few things worth knowing once it's installed:

- **`yt-dlp` and `ffmpeg` are independent of the app** — they must stay on `PATH` (Step 1 above). Installing/uninstalling TurboDL Ultra never touches them.
- **Updating**: pull the latest source, repeat Step 4's build, then re-run the `dpkg -i` command above — `dpkg` upgrades an existing install in place rather than erroring.
- **Uninstalling**:
  ```bash
  sudo dpkg -r turbo-downloader
  ```
- **Running without installing at all**: the built binary works standalone too, if you'd rather not touch `dpkg`:
  ```bash
  ./src-tauri/target/release/turbo-downloader
  ```
  This skips the app-launcher/desktop-entry integration but is otherwise identical — handy for quickly testing a fresh build.

---

## How to Use

**Single mode** — paste one video URL and formats (grouped by resolution) appear automatically within a second; pick one and add it to the queue. Paste several URLs at once (one per line) and it switches to a bulk sub-mode with one shared resolution picker instead of per-video formats.

**Batch mode** — paste a list of URLs into the textarea, or load a `.txt` file (one URL per line; lines starting with `#` are treated as comments and skipped). Pick a shared max resolution and queue the whole list at once.

**Keyword mode** — give a channel/playlist URL and a keyword (e.g. source `https://www.youtube.com/@nasa/videos`, keyword `mars`); the app validates the source live as you type, shows a sample of matching titles, then queues every match up to your chosen result count when you hit download.

Whichever mode you use, everything lands in the **queue** at the bottom of the window: watch live progress, pause/resume/cancel individual items, or use Cancel All / Clear Completed for the whole queue. Set your download folder once via **Save To** in the header — it applies to every mode, including links sent in from the browser extension.

---

## Concurrent Download Queue

Every download — single, batch, or keyword — goes into one shared queue instead of running immediately. A background dispatcher (`src-tauri/src/queue.rs`) admits queued items up to a concurrency limit you control, so several videos download at once instead of one at a time.

### How it works

- **Max Concurrent** (top-right of the queue view, 1–8, default 3) is live — change it at any time, including while downloads are running. Raising it admits more queued items immediately; lowering it never kills anything already running, it just pauses new admissions until the running count drops back under the new limit.
- **Per-host cap.** Downloads from the *same site* are capped separately from the global limit — roughly half of Max Concurrent, rounded up (e.g. 2 at the default of 3, 4 at 8) — so a burst of same-host connections doesn't trigger a rate limit or a temporary ban. This still lets same-host batches (the most common case — e.g. several results from one keyword search) run in real parallel, just not *all* of them at once against Max Concurrent's full limit.
- **Per-host cooldown.** After a download for a given host finishes, that host gets a short (4–8s) breather before the next one from the same host starts — this replaces what used to be a single global pause between every download.
- **Disk-space check.** Before starting a new download, the app checks free space on its target drive (via `df`) and refuses to start (with a clear error on that row) if it's under 500MB, rather than risk a half-written file.
- **Pause / Resume.** Pausing a running item kills its `yt-dlp` process but keeps the row in the queue; Resume re-queues it, and `yt-dlp` resumes the partially-downloaded file by default — no data is re-downloaded from scratch. A paused item doesn't count against your concurrency limit, so it frees up a slot for something else immediately.
- **Cancel** removes an item for good (whether it was running, pending, or paused). **Cancel All** and **Clear Completed** act on the whole queue at once.

### Known limitation

Keyword search still auto-queues its top N matches rather than letting you pick individual videos from the full match list first — every match still lands as its own row and downloads in parallel with everything else, but there's no checkbox list to deselect a few before they're queued. Tracked in [`project-docs/plans/feature-roadmap.md`](project-docs/plans/feature-roadmap.md).

---

## Browser Extension

A small Chrome extension (`extension/`) lets you send a page or a link straight to TurboDL Ultra without copying and pasting a URL into the app.

### Install it (unpacked, not on the Chrome Web Store)

1. Open `chrome://extensions` in Chrome (or any Chromium-based browser — Edge, Brave, etc. use the same page at their own address).
2. Turn on **Developer mode** (top-right toggle).
3. Click **Load unpacked** and select this project's `extension/` folder.
4. The extension icon appears in your toolbar — pin it for easy access (puzzle-piece icon → pin).

TurboDL Ultra itself must already be installed and runnable (see [Getting Started](#getting-started) above) — the extension only sends the link; the desktop app is what actually registers the `turbodl://` link type with your system, which it does automatically the first time it launches.

### Using it

**From a right-click:**
- Right-click anywhere on a page → **"Send this page to TurboDL"** sends the current page's URL.
- Right-click a link → **"Send this link to TurboDL"** sends that specific link instead of the page you're on.

**From the toolbar button:** click the extension icon, then **"Send Current Tab"**.

Either way, this opens a new tab (`TurboDL Ultra` — sending the link) that does the actual work: it tries to open the app automatically, and always shows an **"Open TurboDL Ultra"** button as a guaranteed fallback if nothing happens on its own — click it. Your browser will then ask **"Open TurboDL Ultra?"** (this is your browser's own safety prompt for external links, not something from this app) — check **"Always allow"** if you don't want to see it every time. If TurboDL Ultra isn't already running, this launches it; if it's already open, sending a link brings it to the front with the new item already appearing in the queue. You can close that helper tab once you've seen the prompt or clicked the button.

### What to expect

- **Where to look for it**: the sent link appears as a new row in the **queue** at the bottom of the TurboDL Ultra window — visible regardless of which tab (Single/Batch/Keyword) is currently selected, marked with a 🧩 icon. That's also where you watch its progress.
- **Where the file lands**: whatever folder is currently set as **Save To** in the app (shown in the header), at the best available quality — the extension has no UI of its own to pick a format or folder before sending, so it always uses the app's current Save To setting, kept in sync automatically. Change Save To in the app *before* sending a link if you want it to land somewhere else; changing it after a link is already queued doesn't move that item.
- A toast in the app confirms what happened — either "Added N links from the browser extension" or a specific rejection reason (e.g. the page's URL wasn't a supported `http(s)` link).
- Sending several links one at a time is fine — there's no limit on how many you queue this way. A dedicated "select multiple links on a page and send them all at once" flow doesn't exist yet — see [Roadmap](#roadmap).

### If it's not working

Work through these in order — each one isolates whether the problem is the app/OS side or the browser/extension side:

1. **Confirm the app is registered as the handler**, independent of any browser:
   ```bash
   xdg-mime query default x-scheme-handler/turbodl
   ```
   This should print something like `turbo-downloader-handler.desktop`. If it prints nothing, launch TurboDL Ultra at least once (it registers itself on startup) and check again.

2. **Confirm the OS can actually launch the app from the link**, still independent of any browser:
   ```bash
   gio open 'turbodl://add?url=https://example.com/test'
   ```
   Use `gio open`, not the `xdg-open` CLI script — on at least one Linux desktop setup, `xdg-open`'s own desktop-file search path didn't include `~/.local/share/applications` and it silently opened a plain browser instead, which looks like a failure but isn't representative of what browsers actually do internally. `gio` is the mechanism GTK-based browsers (Chrome included) actually use on Linux. If this launches (or focuses) the app, the app/OS side is confirmed working and the problem is in step 3.

3. **If steps 1–2 both work but the extension still does nothing**: open `chrome://extensions`, find TurboDL Ultra, click "service worker" under "Inspect views" to open its devtools console, and check for errors there. Also confirm the redirect tab actually opened at all when you click "Send" — if it didn't, the issue is in the extension's own context-menu/popup wiring; if it opened but the automatic navigation did nothing, use the manual "Open TurboDL Ultra" button on that page instead, which is the most reliable trigger regardless of what the automatic attempt did.

4. If you previously dismissed the "Open TurboDL Ultra?" prompt with an "always deny"-style option, your browser will remember that per-scheme — check your browser's site/protocol permission settings to reset it.

---

## Troubleshooting

**`yt-dlp: command not found`**
Re-run [Getting Started](#getting-started) Step 1 item 3 and verify `/usr/local/bin/yt-dlp` is executable.

**`libwebkit2gtk-4.1-dev` not found**
On older Ubuntu (< 22.04) try `libwebkit2gtk-4.0-dev`. Update `Cargo.toml`:
`tauri = { version = "2", features = ["protocol-asset"], ... }` — Tauri 2 requires 4.1.

**Clipboard auto-paste not working**
The Clipboard API requires a secure context. In `tauri dev`, it works out of the box. In production, no extra config needed.

**FFmpeg merge fails**
Ensure `ffmpeg` is installed: `ffmpeg -version`. The format selectors require FFmpeg to be present for merging separate video/audio streams.

**The app freezes/hangs on launch, or the window says "not responding"**
This is the native-Wayland WebKit/GTK3 stall described in [Platform Support](#platform-support) — make sure your system has XWayland available (installed by default on most distros). See `project-docs/DECISIONS.md` ADR-003 for the full investigation.

**Curious what's fixed vs. still shaky?**
Most functionality has been verified end-to-end, but a handful of recent fixes were only confirmed via `cargo build`/`cargo test`/`npm run build` in a sandboxed environment without a live display or network — not yet clicked through in a real GUI session. See [`project-docs/REVIEW.md`](project-docs/REVIEW.md) for the current "pending live verification" list, and [`project-docs/ISSUES.md`](project-docs/ISSUES.md) for the full resolved-bug history.

---

## Roadmap

The full backlog lives in two planning docs so this README doesn't go stale every time an idea gets added:

- [`project-docs/plans/feature-roadmap.md`](project-docs/plans/feature-roadmap.md) — download features not yet built (audio-only extraction, subtitles, thumbnail/metadata embedding), queue robustness (persistence across restarts, drag-reorder, history), UX polish, and browser-extension parity.
- [`project-docs/plans/windows-port.md`](project-docs/plans/windows-port.md) — the plan for eventually shipping a Windows build.

If you have an idea that isn't there yet, it belongs in [`project-docs/plans/idea-inbox.md`](project-docs/plans/idea-inbox.md) until it's ready to become a full plan.

---

## Contributing

This is currently a solo/portfolio project, but issues and pull requests are welcome. If you're picking up a bug or a roadmap item, a quick look at `project-docs/CHANGES.md` and the relevant file in `project-docs/plans/` will save you from re-deriving context that's already written down. Run `npm run build` (frontend typecheck) and `cargo build`/`cargo test` inside `src-tauri/` (backend) before opening a PR — there's no separate lint step configured.

---

## License

MIT — see [`LICENSE`](LICENSE).
