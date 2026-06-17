# Changes — turbo-downloader

Codex logs every change it makes here.
Format: - [TYPE] scope: what changed → see sessions/FILENAME.md
Types: Added | Changed | Fixed | Removed | Refactored | Security | Docs

## [Unreleased]

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
