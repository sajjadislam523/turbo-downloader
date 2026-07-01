# Review queue — turbo-downloader

Check this file after every Codex session.

## Pending review

- Keyword download UI is fully functional; ready for end-to-end test with real YouTube search
- Build system working correctly (dev and production)
- No issues found in test scenarios

## Resolved

- 2026-06-17: Keyword search download feature complete (UI + backend integration + builds verified)
- 2026-06-07: Updated `AGENTS.md` to use README-backed Tauri commands and removed the expectation that `npm run lint` exists.
- Port `5173` was already in use during `npm run tauri dev`; check the host process using that port before rerunning the desktop dev app.
- Restart Codex so `/home/kabila/.codex/config.toml` is reloaded, then verify sandboxed command execution with `pwd` or `rg --files`.
