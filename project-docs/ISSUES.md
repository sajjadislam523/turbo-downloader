# Issues — turbo-downloader

Auto-generated during code review on 2026-07-11.
Each issue is tracked to resolution; resolved items are marked `[✓]`.

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
