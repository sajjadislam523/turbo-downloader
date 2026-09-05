# Changes — turbo-downloader

Codex logs every change it makes here.
Format: - [TYPE] scope: what changed → see sessions/FILENAME.md
Types: Added | Changed | Fixed | Removed | Refactored | Security | Docs

## [Unreleased]

- [Added] rust: `busy: Arc<AtomicBool>` guard on `DownloadState` rejects a second `start_*_download` call while one is already running (including during a batch's inter-file gaps), closing a race where an overlapping call could orphan the first process → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: cancel now kills the whole yt-dlp process group (`process_group(0)` + `kill -TERM -PID`), not just the yt-dlp PID, so an in-progress ffmpeg merge is killed too instead of continuing to write the output file → see project-docs/plans/bug-review-2026-09-03.md
- [Added] rust: closing the app window while a download is active now kills the tracked process group via a new `on_window_event` handler, instead of leaving yt-dlp/ffmpeg running as orphans → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: `start_keyword_download` now passes `--playlist-end` matching the pre-flight validation's scan depth (`KEYWORD_SCAN_LIMIT`), so the real download can no longer crawl arbitrarily deeper into a channel than what validation told the user to expect → see project-docs/plans/bug-review-2026-09-03.md
- [Added] rust+ui: batch downloads with some (not all) files failing now emit a distinct `download-partial` event with success/failure counts, shown in the UI instead of reading as a total failure → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: replaced `.lock().unwrap()` on the shared PID mutex with a poison-tolerant `pid_lock` helper, so a poisoned lock recovers instead of aborting the whole app (release profile uses `panic = "abort"`) → see project-docs/plans/bug-review-2026-09-03.md
- [Security] tauri: set a restrictive CSP in `tauri.conf.json` (was `null`) — `default-src`/`script-src`/`connect-src` limited to `'self'`, fonts to Google Fonts' two origins → see project-docs/plans/bug-review-2026-09-03.md
- [Added] rust: unit tests for the pure helper functions in `lib.rs` (20 tests: URL/query validation, format selectors, tilde expansion, regex/match-filter escaping, source-info extraction, the new overlap guard) → see project-docs/plans/bug-review-2026-09-03.md
- [Added] ci: `.github/workflows/ci.yml` — frontend job (`npm run build`) and Rust job (`cargo check` + `cargo test` + `cargo clippy -D warnings`) on push/PR → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: fixed 4 pre-existing clippy `lines_filter_map_ok` warnings (`.lines().flatten()` → `.lines().map_while(Result::ok)` on the yt-dlp stdout/stderr readers) so CI's clippy gate starts clean → see project-docs/plans/bug-review-2026-09-03.md
- [Added] ui: `mode`, `savePath`, `batchResolution`, `keywordLimit`, and `keywordResolution` now persist to `localStorage` and restore on next launch → see project-docs/plans/bug-review-2026-09-03.md

- [Fixed] rust: batch download cancel now actually stops the queue — added a shared `cancelled` flag on `DownloadState`, checked at the top of each loop iteration and in short-increment sleeps between files, instead of the loop silently continuing to the next queued URL after the current file was killed → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: `stop_download` no longer produces a spurious `download-error` toast after a deliberate cancel (single, batch, and keyword downloads) — background threads now check the `cancelled` flag before reporting a killed process's non-zero exit as a failure → see project-docs/plans/bug-review-2026-09-03.md
- [Fixed] rust: `build_match_filter` now escapes literal `'` characters, fixing keyword searches for titles containing apostrophes (e.g. "don't"), which previously broke yt-dlp's `--match-filters` quoted-string parsing → see project-docs/plans/bug-review-2026-09-03.md

- [Fixed] rust: force GDK_BACKEND=x11 to prevent Wayland/GTK3 "not responding" crashes → see sessions/2026-07-11-002.md

- [Fixed] rust: removed dead code path in `extract_source_info` (unreachable array branch) → see sessions/2026-07-11-001.md
- [Fixed] rust: removed redundant yt-dlp validation in `start_keyword_download` (skip re-probe during download start) → see sessions/2026-07-11-001.md
- [Removed] build: removed unused `postcss` devDependency (Tailwind v4 + Vite plugin doesn't need it) → see sessions/2026-07-11-001.md
- [Fixed] build: removed unnecessary `async` keyword from `vite.config.ts` → see sessions/2026-07-11-001.md
- [Added] ui: `ErrorBoundary` component wrapping the app to prevent blank-screen crashes → see sessions/2026-07-11-001.md
- [Added] rust+ui: download cancel button with `stop_download` Tauri command (kills yt-dlp by PID) → see sessions/2026-07-11-001.md
- [Docs] codex: created `project-docs/ISSUES.md` tracking all findings and their resolution → see sessions/2026-07-11-001.md

- [Added] ui: wired keyword download button to backend `start_keyword_download` command → see sessions/2026-06-17-001.md
- [Changed] ui: unified download handler to support all three modes (single, batch, keyword) with proper state management
- [Changed] ui: enabled keyword download button when query is entered and app is ready
- [Verified] build: `npm run tauri dev` starts correctly with keyword mode UI fully functional
- [Verified] build: `npm run tauri build` completes successfully with release binary and .deb package
- [Added] ui: added keyword search download controls pending backend wiring → see sessions/2026-06-07-005.md
- [Fixed] ui: made batch download controls reachable in constrained app windows → see sessions/2026-06-07-005.md
- [Docs] codex: planned responsive layout repairs and keyword search downloads → see sessions/2026-06-07-005.md
- [Docs] codex: corrected project verification commands to use Tauri scripts → see sessions/2026-06-07-004.md
- [Docs] codex: added mandatory idea capture rules and plan inbox → see sessions/2026-06-07-003.md
- [Docs] codex: inspected project state and noted missing prior feature description → see sessions/2026-06-07-002.md
- [Fixed] codex: disabled restrictive global allowed_dirs setting blocking sandboxed shell startup → see sessions/2026-06-07-001.md

## [0.0.0] — 2026-06-07

- [Added] project: Codex log architecture initialised
