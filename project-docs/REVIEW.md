# Review queue — turbo-downloader

Check this file after every Codex session.

## Pending review

- 2026-09-04 changes were verified with `cargo check`/`cargo test`/`cargo clippy -D warnings`/`cargo build` and `npm run build` only — the app itself was never launched in a GUI (no display in the environment this was written in). Before trusting the cancel/process-group/CSP changes, run `npm run tauri dev` and manually check: cancelling a batch mid-queue actually stops it (no further files start); cancelling a single/keyword download doesn't leave a stray "download failed" toast; the app still renders correctly (fonts, styles) under the new CSP; closing the window mid-download doesn't leave an orphaned yt-dlp/ffmpeg process (`ps aux | grep yt-dlp` after quitting).
- Keyword download UI is fully functional; ready for end-to-end test with real YouTube search
- Build system working correctly (dev and production)
- Error boundary catches React crashes with a styled fallback

## Resolved

- 2026-09-04: Resolved all remaining findings from the 2026-09-03 full-project review (7 bugs: keyword scan-depth mismatch, no overlapping-download guard, ffmpeg orphaned on cancel, no cleanup on app quit, partial batch failures reported as total failure, poisoned-mutex abort risk, disabled CSP) plus three improvements (Rust unit tests, GitHub Actions CI, settings persistence). See `project-docs/plans/bug-review-2026-09-03.md` and `ISSUES.md` Bugs 7-13.
- 2026-09-03: Fixed 2 high-severity bugs from a full-project review — batch cancel not stopping the queue, keyword search breaking on apostrophes. See `project-docs/plans/bug-review-2026-09-03.md` and `ISSUES.md` Bugs 5-6.
- 2026-07-11: Code review and fix session — dead code removal, redundant yt-dlp validation eliminated, unused dep removed, error boundary and cancel button added, `ISSUES.md` created.
- 2026-06-17: Keyword search download feature complete (UI + backend integration + builds verified)
- 2026-06-07: Updated `AGENTS.md` to use README-backed Tauri commands and removed the expectation that `npm run lint` exists.
- Port `5173` was already in use during `npm run tauri dev`; check the host process using that port before rerunning the desktop dev app.
- Restart Codex so `/home/kabila/.codex/config.toml` is reloaded, then verify sandboxed command execution with `pwd` or `rg --files`.
