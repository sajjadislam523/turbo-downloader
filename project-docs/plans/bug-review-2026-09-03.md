# Plan — full-project bug review (2026-09-03)

Source: full read-through of `src-tauri/src/lib.rs` and `src/App.tsx`, cross-checked against `project-docs/ISSUES.md`.

## Bugs

1. **[FIXED] Batch cancel doesn't stop the batch (High)** — `start_batch_download`'s
   loop has no cancellation flag; `stop_download` only kills the current file's PID, so the
   loop launches the next queued URL after a "cancelled" item and can later emit
   `download-complete`/`download-error`, silently overriding the UI's cancelled state.
   `src-tauri/src/lib.rs:588-679`, `:682-703`.
   Fix: added `cancelled: Arc<AtomicBool>` to `DownloadState`, reset at the start of every
   `start_*_download` call and set by `stop_download`; the batch loop checks it before each
   item, during the inter-file sleep (in 200ms increments so cancel takes effect almost
   immediately instead of after up to 8s), and suppresses the trailing
   `download-complete`/`download-error` emit when the run was cancelled. `spawn_and_stream`
   (single/keyword downloads) and the batch loop's failure handling likewise suppress
   `download-error` when the non-zero exit was caused by a deliberate cancel.

2. **[FIXED] Keyword search breaks on apostrophes (High)** — `escape_regex` doesn't
   escape `'`, which delimits the string literal in yt-dlp's `--match-filters title ~= '...'`.
   A query like `don't` produces `title ~= 'don't'`, breaking the filter's quoted-string
   parsing. `src-tauri/src/lib.rs:129-132`, `:524-534`.
   Fix: `build_match_filter` now replaces literal `'` with `\'` after regex-escaping.
   Verified with `cargo check` (clean compile, no warnings on the changed code).

3. **[FIXED] Keyword download can scan deeper than validation promised (Medium)** —
   `validate_keyword_source_blocking` bounds its simulate pass with `--playlist-end 200`;
   `start_keyword_download` never passes `--playlist-end`, so the real run can crawl an
   entire large channel, contradicting the "scanned up to 200 entries" message.
   `src-tauri/src/lib.rs:198-215` vs `:483-503`.
   Fix: `start_keyword_download` now passes `--playlist-end KEYWORD_SCAN_LIMIT`, matching
   the depth the pre-flight validation already scanned.

4. **[FIXED] No backend guard against overlapping downloads (Medium)** — `running_pid` was
   overwritten on every `start_*_download` call; a second concurrent call orphaned the first
   process (untracked, uncancelable).
   Fix: added `busy: Arc<AtomicBool>` to `DownloadState`, claimed via `compare_exchange` in a
   new `reserve_download_slot` helper called at the top of every `start_*_download` command,
   released once the download (or, for batch, the *whole* queue including inter-file gaps)
   finishes. `running_pid` itself was simplified back to holding only a real PID or `None`.

5. **[FIXED] Cancel leaves ffmpeg orphaned (Medium)** — `stop_download` sent SIGTERM to the
   yt-dlp PID only, not its process group; a merge already handed to `ffmpeg` kept running
   after "cancelled" was reported.
   Fix: every yt-dlp `Command` now calls `.process_group(0)` (via
   `std::os::unix::process::CommandExt`), and `stop_download`/the window-close handler kill
   the whole group (`kill -TERM -PID`) via a shared `kill_process_group` helper.

6. **[FIXED] No cleanup on app quit (Medium)** — closing the window mid-download didn't kill
   the tracked child process; yt-dlp/ffmpeg became orphans.
   Fix: added an `.on_window_event` handler in `run()` that kills the tracked process group on
   `WindowEvent::CloseRequested`.

7. **[FIXED] Partial batch/keyword failures look like total failure (Low/Medium)** — a single
   `download-error` fired if *any* file failed, with no success count.
   Fix (batch only — see note below): `start_batch_download` now tracks a `succeeded` counter
   alongside `failures`, and emits a new `download-partial` event (`{succeeded, failed,
   errors}`) instead of `download-error` when at least one file succeeded and at least one
   failed. The frontend listens for it and shows a distinct "`N of M downloaded — F failed`"
   status plus a toast listing the failures, rather than a plain error. Scoped to batch only:
   keyword downloads run as a single yt-dlp process covering all matched videos (with
   `--ignore-errors`), so there's no clean per-video success/failure count available without
   parsing yt-dlp's output more deeply — left as a known limitation.

8. **[FIXED] `panic = "abort"` + `.lock().unwrap()` (Low)** — a poisoned mutex from any panic
   would abort the whole app instead of failing one command.
   Fix: added a `pid_lock` helper (`guard.lock().unwrap_or_else(|p| p.into_inner())`) and
   replaced every `.lock().unwrap()` on `running_pid` with it, so a poisoned lock recovers
   instead of cascading into a full-process abort.

9. **[FIXED] CSP disabled (Low)** — `tauri.conf.json` set `"csp": null`.
   Fix: set a CSP restricting `default-src`/`script-src`/`connect-src` to `'self'`, fonts to
   Google Fonts' two origins, and `img-src` to `'self' data:` (for the inline SVG noise
   texture). `style-src` keeps `'unsafe-inline'` since the UI sets many dynamic inline
   `style={{...}}` values in React — not verified against a running GUI (no display in this
   environment); recommend a smoke test on a machine that can launch the app.

## Other suggestions (not bugs) — all done

- **[DONE] Rust unit tests** — added a `#[cfg(test)] mod tests` in `lib.rs` covering
  `expand_tilde`, `build_output_template`, `build_height_selector`,
  `validate_keyword_result_count`, `validate_http_url`, `escape_regex`, `build_match_filter`
  (includes a regression test for bug #2's apostrophe case), `extract_source_info`, and
  `reserve_download_slot`. 20 tests, all passing (`cargo test`).
- **[DONE] CI** — added `.github/workflows/ci.yml`: a frontend job (`npm ci` + `npm run
  build`, i.e. tsc + vite build) and a Rust job (installs the same Tauri Linux system deps
  from the README, then `cargo check` + `cargo test` + `cargo clippy -D warnings`). Fixed 4
  pre-existing clippy warnings (`lines_filter_map_ok` — `.lines().flatten()` →
  `.lines().map_while(Result::ok)` on the yt-dlp stdout/stderr readers) so the new `-D
  warnings` clippy gate starts from a clean baseline instead of failing immediately.
- **[DONE] Settings persistence** — `mode`, `savePath`, `batchResolution`, `keywordLimit`,
  and `keywordResolution` now persist to `localStorage` (`turbodl.settings.v1`) and are
  restored on next launch. Deliberately excludes session-specific fields (URL inputs, batch
  file path, fetched metadata) since restoring those on a fresh launch would be confusing
  rather than convenient.
- **[NOT DONE, by design]** `App.tsx` monolith (1370+ lines, one component) and the disabled
  `exhaustive-deps` lint — both pre-existing, already tracked in `project-docs/ISSUES.md` as
  acknowledged future work, and out of scope here (large, architecturally risky refactors
  rather than bugs).

## Verification

`cargo check`, `cargo test` (20/20 passing), `cargo clippy --all-targets -- -D warnings`
(clean), and `cargo build` (dev profile, confirms linking against webkit2gtk/gtk3 succeeds)
all pass. Frontend: `npm run build` (tsc typecheck + vite build) passes. Not verified: the
app was not actually launched in a GUI in this environment (no display available), so the
CSP change and the runtime behavior of the process-group/cancel changes should get a manual
smoke test (`npm run tauri dev`) before relying on them, per this repo's own AGENTS.md
convention of running that after every change.

## Decision

User asked to resolve all remaining issues (#3-#9) plus the "other suggestions" sequentially,
following up on the earlier #1/#2 fixes from the same review. All 9 numbered bugs are now
fixed, and all three "other suggestions" (tests, CI, settings persistence) are done. The two
explicitly-deferred items (`App.tsx` split, `exhaustive-deps`) were left alone as pre-existing
acknowledged future work, not bugs from this review.
