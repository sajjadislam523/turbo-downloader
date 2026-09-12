# Issues — turbo-downloader

Auto-generated during code review on 2026-07-11.
Each issue is tracked to resolution; resolved items are marked `[✓]`.

---

## [✓] Bug 5 — Batch download cancel doesn't stop the batch

**File:** `src-tauri/src/lib.rs` — `start_batch_download`, `stop_download`
**Severity:** High
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

`start_batch_download`'s loop had no cancellation flag; `stop_download` only killed the
current file's yt-dlp PID, so the loop launched the next queued URL after a "cancelled"
item and could later emit `download-complete`/`download-error`, silently overriding the
UI's cancelled state.

**Fix:** Added `cancelled: Arc<AtomicBool>` to `DownloadState`, reset at the start of every
`start_*_download` call and set by `stop_download`. The batch loop checks it before each
item and during the inter-file sleep (in 200ms increments), and suppresses the trailing
`download-complete`/`download-error` emit when cancelled. `spawn_and_stream` and the batch
loop's failure handling also suppress `download-error` when a non-zero exit was caused by
a deliberate cancel, so cancelling a single or keyword download no longer produces a
spurious error toast either.

---

## [✓] Bug 6 — Keyword search breaks on titles/queries with an apostrophe

**File:** `src-tauri/src/lib.rs:137-146` (`build_match_filter`)
**Severity:** High
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

`escape_regex` escaped regex metacharacters but not `'`, which delimits the string literal
in yt-dlp's `--match-filters title ~= '...'`. A query like `don't` produced
`title ~= 'don't'`, breaking the filter's quoted-string parsing.

**Fix:** `build_match_filter` now replaces literal `'` with `\'` after regex-escaping.
Verified with `cargo check` (clean compile).

---

## [✓] Bug 7 — Keyword download can scan deeper than validation promised

**File:** `src-tauri/src/lib.rs` — `validate_keyword_source_blocking`, `start_keyword_download`
**Severity:** Medium
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

Validation bounded its simulate pass with `--playlist-end 200` (`KEYWORD_SCAN_LIMIT`), but
the real download never passed `--playlist-end` at all, so it could crawl an entire large
channel — contradicting the "scanned up to 200 entries" message shown to the user.

**Fix:** `start_keyword_download` now passes `--playlist-end` set to `KEYWORD_SCAN_LIMIT`,
matching what validation already scanned.

---

## [✓] Bug 8 — No backend guard against overlapping downloads

**File:** `src-tauri/src/lib.rs` — `DownloadState`, `reserve_download_slot`
**Severity:** Medium
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

`running_pid` was simply overwritten by each `start_*_download` call; a second concurrent
call would orphan the first process (untracked, uncancelable). Only the frontend's
button-disabling prevented this in practice.

**Fix:** Added `busy: Arc<AtomicBool>`, claimed via `compare_exchange` in a new
`reserve_download_slot` helper called at the top of every `start_*_download` command and
released once the download (for batch: the whole queue, including inter-file gaps)
finishes. Rejects a second call with "A download is already in progress." `running_pid`
itself went back to holding only a real PID or `None` — the guard duty moved to `busy`
so it correctly stays "occupied" through a batch's inter-file sleeps too, which a
PID-based guard couldn't represent (no child process is alive during those gaps).

---

## [✓] Bug 9 — Cancel leaves ffmpeg orphaned

**File:** `src-tauri/src/lib.rs` — every yt-dlp `Command`, `stop_download`
**Severity:** Medium
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

`stop_download` sent SIGTERM to the yt-dlp PID only, not its process group. If yt-dlp had
already handed off to `ffmpeg` for merging, ffmpeg wasn't killed and kept writing the
output file after "cancelled" was reported.

**Fix:** Every yt-dlp `Command` now calls `.process_group(0)`
(`std::os::unix::process::CommandExt`), putting it and any child it spawns in its own
process group. `stop_download` (and the new window-close handler, Bug 11) now kill the
whole group via a shared `kill_process_group` helper (`kill -TERM -{pid}`).

---

## [✓] Bug 10 — Partial batch failures look like total failure

**File:** `src-tauri/src/lib.rs` — `start_batch_download`; `src/App.tsx` — event listeners
**Severity:** Low/Medium
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

A single `download-error` fired if *any* file in a batch failed, with no success count — a
49/50-file run read identically to a 0/50 run in the UI.

**Fix:** `start_batch_download` now tracks a `succeeded` counter and emits a new
`download-partial` event (`{succeeded, failed, errors}`) when at least one file succeeded
and at least one failed, instead of `download-error`. The frontend shows a distinct status
("N of M downloaded — F failed") and a toast listing the failures. Scoped to batch only:
keyword downloads run as a single yt-dlp process (`--ignore-errors`) covering all matched
videos, so there's no clean per-video count available without deeper output parsing — left
as a known limitation.

---

## [✓] Bug 11 — No cleanup on app quit

**File:** `src-tauri/src/lib.rs` — `run()`
**Severity:** Medium
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

Closing the app window mid-download didn't kill the tracked child process; yt-dlp/ffmpeg
became orphans running in the background with no way to stop them short of a terminal
`pkill`.

**Fix:** Added an `.on_window_event` handler in `run()` that kills the tracked process
group (via `kill_process_group`, Bug 9) on `WindowEvent::CloseRequested`.

---

## [✓] Bug 12 — `panic = "abort"` + `.lock().unwrap()` risk

**File:** `src-tauri/src/lib.rs`
**Severity:** Low
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

The release profile aborts the whole process on any panic (`panic = "abort"` in
`Cargo.toml`). Several commands called `.lock().unwrap()` on the shared PID mutex; a
poisoned lock from any prior panic would cascade into a full-app crash instead of a
contained command error.

**Fix:** Added a `pid_lock` helper
(`guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner())`) and replaced every
`.lock().unwrap()` on `running_pid` with it.

---

## [✓] Bug 13 — CSP disabled

**File:** `src-tauri/tauri.conf.json`
**Severity:** Low
**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

`security.csp` was `null` — no Content-Security-Policy enforced on the webview.

**Fix:** Set `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'
https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com; img-src 'self'
data:; connect-src 'self'`. `style-src` keeps `'unsafe-inline'` since the UI sets many
dynamic inline `style={{...}}` values in React. **Not verified against a running GUI** — no
display available in the environment this was written in; smoke-test with `npm run tauri
dev` before relying on it.

---

## [✓] Improvement — added Rust unit tests, CI, and settings persistence

**Found:** 2026-09-03 full-project review, see `project-docs/plans/bug-review-2026-09-03.md`

Three gaps noted alongside the bugs above, not bugs themselves:

- No automated tests anywhere → added a `#[cfg(test)] mod tests` in `lib.rs` (20 tests)
  covering the pure helper functions, including a regression test for Bug 6.
- No CI → added `.github/workflows/ci.yml` (frontend build; Rust check/test/clippy with
  `-D warnings`). Fixed 4 pre-existing clippy `lines_filter_map_ok` warnings so the new
  gate starts clean.
- No settings persistence → `mode`, `savePath`, `batchResolution`, `keywordLimit`,
  `keywordResolution` now persist to `localStorage` and restore on next launch.

---

## [✓] Bug 1 — Dead code path in `extract_source_info`

**File:** `src-tauri/src/lib.rs:130`
**Severity:** Low

The `json.as_array()` branch in `extract_source_info` is unreachable. Running
`yt-dlp -j --flat-playlist --playlist-end 1` outputs line-delimited JSON objects
(one per video, capped at 1), so `serde_json::from_slice` always parses a single
object — never an array.

**Fix:** Removed the array branch and the unused import of `entries.len()`.

---

## [✓] Bug 2 — Redundant yt-dlp validation in `start_keyword_download`

**File:** `src-tauri/src/lib.rs:470`
**Severity:** Medium

When the user clicks "Start Keyword Search & Download", `start_keyword_download`
re-runs `validate_keyword_source_blocking`, which spawns `yt-dlp` **twice**
(probe + simulate). The frontend's debounced pre-flight already did this. For
channels with hundreds of entries, this adds 2–10s delay before download starts.

**Fix:** Replace the full re-validation with a lightweight argument re-check.
Only validate URL format and query emptiness; skip the yt-dlp calls.

---

## [✓] Code Quality 3 — Unused `postcss` devDependency

**File:** `package.json:24`
**Severity:** Low

Tailwind v4 with the `@tailwindcss/vite` plugin does not require PostCSS. No
`postcss.config.js` exists and none is needed.

**Fix:** Removed `postcss` from `devDependencies`.

---

## [✓] Code Quality 4 — Unnecessary `async` wrapper on `vite.config.ts`

**File:** `vite.config.ts:6`
**Severity:** Low

The factory function passed to `defineConfig` is `async` but contains no `await`.

**Fix:** Removed the `async` keyword.

---

## [✓] Code Quality 1 — `App.tsx` is 1339 lines in one component

**File:** `src/App.tsx`
**Severity:** Low (future work)

All state, event listeners, sub-components, and render logic live in one
monolithic function. Every state change re-runs the entire component body.

**Status:** Acknowledged but not fixed in this session. Would require extracting
sub-components into separate files and splitting state concerns.

---

## [✓] Code Quality 2 — ESLint `exhaustive-deps` disabled for URL debounce

**File:** `src/App.tsx:472`
**Severity:** Low (future work)

The effect captures `triggerFetch` but omits it from the dependency array with
an eslint-disable comment. Works correctly in practice because `triggerFetch`
is stable.

**Status:** Acknowledged but not fixed in this session. Would require adding
`triggerFetch` to deps and wrapping it with `useEvent` or similar pattern.

---

## [✓] Missing Feature 2 — No React error boundary

**Severity:** Medium

If the React component tree throws (e.g., malformed data from yt-dlp), the
entire app window goes blank with no error message.

**Fix:** Added an `ErrorBoundary` component wrapping the app in `main.tsx`,
with a styled fallback UI and a "Restart" button.

---

## [✓] Missing Feature 1 — No way to cancel an active download

**Severity:** Medium

Once a download starts, the user has no way to stop it except closing the app.

**Fix:**
- Added `stop_download` Tauri command that kills the running `yt-dlp` process
- Added `CancelButton` component in the frontend that calls `stop_download`
- The cancel button appears only while a download is in progress
- After cancellation, disk writes from yt-dlp may leave partial files
