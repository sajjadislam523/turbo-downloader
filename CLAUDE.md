# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

TurboDL Ultra — a Tauri 2 desktop video downloader for Linux. React 19 + TypeScript frontend, Rust backend that shells out to `yt-dlp` (with `ffmpeg` for stream merging) using 5x parallel chunk downloading. Not Electron — uses `webkit2gtk`.

## Commands

```bash
npm run tauri dev      # start the desktop app in dev mode (Vite + Tauri, live reload)
npm run tauri build    # production build; must pass after any change
npm run build           # tsc typecheck + vite build (frontend only, no Tauri packaging)
npm run dev              # Vite dev server only (no Tauri window — invoke() calls won't work)
```

There is no test suite and no ESLint config in this repo. `npm run build` (which runs `tsc`) is the closest thing to a lint/verification step for the frontend; `cargo build` inside `src-tauri/` (or the `tauri build`/`tauri dev` wrapper) is the check for the Rust side.

Run `npm run tauri dev` and `npm run tauri build` after any change to confirm nothing broke — this is expected practice for this repo (see AGENTS.md).

### System prerequisites (Linux/Ubuntu)

Building/running requires system packages beyond `npm install`: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `ffmpeg`, and a `yt-dlp` binary on `PATH` (typically `/usr/local/bin/yt-dlp`). See README.md Step 1 for the full list if a build fails due to missing system libs.

## Architecture

```
Frontend (src/App.tsx, React+TS)  --invoke()/listen()-->  Rust backend (src-tauri/src/lib.rs)  --Command::new()-->  yt-dlp / ffmpeg
```

- **`src/App.tsx`** is the entire frontend UI as a single ~1370-line component (single/batch/keyword download modes, clipboard auto-paste, live progress). There is no component splitting yet — see "Future work" below.
- **`src-tauri/src/lib.rs`** (~725 lines) holds all Tauri `#[tauri::command]` handlers and the yt-dlp process orchestration:
  - `fetch_meta_blocking` — probes a URL via yt-dlp `-J` to get available formats/title.
  - `start_turbo_download` — single-video download.
  - `start_batch_download` — reads a `.txt` file of URLs (one per line, `#` = comment) and downloads sequentially with a sleep interval between items.
  - `start_keyword_download` — given a channel/playlist URL + keyword, uses yt-dlp `--match-filters` to download only title-matching videos.
  - `stop_download` — kills the tracked yt-dlp PID (stored in `DownloadState`, a `Mutex<Option<u32>>` managed via Tauri state).
  - `open_directory_dialog` / `open_file_dialog` — native GTK pickers via the `rfd` crate.
  - `update_yt_dlp` — self-updates the yt-dlp binary.
- **IPC contract**: frontend calls backend via `invoke("command_name", {...})`; backend pushes progress back via `app.emit(...)` on named events the frontend subscribes to with `listen(...)`: `download-progress`, `download-complete`, `download-error`, `batch-total`, `batch-item-start`.
- **Streaming/process handling**: yt-dlp's stdout (progress) and stderr (warnings) are drained on separate threads concurrently (`spawn_and_stream` / `stream_and_wait`). Reading stderr only after the process exits was a real bug — a full OS pipe buffer can block yt-dlp's `write()` indefinitely — so don't reintroduce sequential stdout-then-stderr reads.
- **Format selection** uses height-based selectors (`bestvideo[height=1080][ext=mp4]+bestaudio[ext=m4a]/...`, built by `build_height_selector`) rather than raw numeric format IDs, since format IDs aren't portable across sites.
- **Path handling**: `expand_tilde` manually expands a leading `~` against `$HOME` before handing paths to yt-dlp, because `Command::new()` doesn't go through a shell and won't do this expansion itself.

### Platform quirk: forced X11 backend

`src-tauri/src/main.rs` sets `GDK_BACKEND=x11` before Tauri initializes. This works around a WebKit/GTK3 event-loop hang on native Wayland (see `project-docs/DECISIONS.md` ADR-003). Do not remove this without understanding that ADR — the app is known to freeze under native Wayland/GTK3 without it, and requires XWayland to be present.

## Project documentation workflow (from AGENTS.md)

This repo has an existing convention (written for Codex, but apply it here too) of logging work to `project-docs/`:

- `project-docs/CHANGES.md` — rolling changelog
- `project-docs/DECISIONS.md` — architectural decisions (ADR-style, numbered)
- `project-docs/REVIEW.md` — items flagged for human review
- `project-docs/ISSUES.md` — tracked issues
- `project-docs/sessions/` — per-session detail notes
- `project-docs/plans/` — feature plans; `project-docs/plans/idea-inbox.md` for ideas not yet ready to plan

When the user describes a new feature idea or product direction, capture it in `project-docs/plans/` (either promote it to a `FEATURE-SLUG.md` plan, or append to `idea-inbox.md` if it's not yet ready) before or alongside implementation, rather than relying on conversation history alone. When making a non-obvious structural decision, add an ADR entry to `project-docs/DECISIONS.md` following the existing format (Context / Decision / Rejected alternatives / Consequences).

## Known scope for future work (not yet addressed)

- `App.tsx` is a single 1370-line component — extracting sub-components is a known-but-undone improvement, not something to fix incidentally as part of unrelated changes unless asked.
- ESLint `exhaustive-deps` is disabled for the URL debounce effect in `App.tsx`; adding `triggerFetch` to the dependency array requires stabilizing it first (e.g. `useCallback`/ref) to avoid retrigger loops.
