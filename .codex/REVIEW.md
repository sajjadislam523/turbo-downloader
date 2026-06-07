# Review queue — turbo-downloader

Check this file after every Codex session.

## Pending review
- Port `5173` was already in use during `npm run tauri dev`; check the host process using that port before rerunning the desktop dev app.
- Restart Codex so `/home/kabila/.codex/config.toml` is reloaded, then verify sandboxed command execution with `pwd` or `rg --files`.

## Resolved
- 2026-06-07: Updated `AGENTS.md` to use README-backed Tauri commands and removed the expectation that `npm run lint` exists.
