# Review queue — turbo-downloader

Check this file after every Codex session.

## Pending review

- Keyword download UI is fully functional; ready for end-to-end test with real YouTube search
- Build system working correctly (dev and production)
- Cancel button works for active downloads (kills yt-dlp by PID)
- Error boundary catches React crashes with a styled fallback
- Redundant yt-dlp validation removed for keyword downloads — verify no regression
- Dead code removed from `extract_source_info`

## Resolved

- 2026-07-11: Code review and fix session — dead code removal, redundant yt-dlp validation eliminated, unused dep removed, error boundary and cancel button added, `ISSUES.md` created.
- 2026-06-17: Keyword search download feature complete (UI + backend integration + builds verified)
- 2026-06-07: Updated `AGENTS.md` to use README-backed Tauri commands and removed the expectation that `npm run lint` exists.
- Port `5173` was already in use during `npm run tauri dev`; check the host process using that port before rerunning the desktop dev app.
- Restart Codex so `/home/kabila/.codex/config.toml` is reloaded, then verify sandboxed command execution with `pwd` or `rg --files`.
