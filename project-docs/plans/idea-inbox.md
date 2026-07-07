# Idea Inbox

This file captures user ideas before they are specific enough to become a full
feature plan. Each entry should be promoted to `.codex/plans/FEATURE-SLUG.md`
when implementation steps are clear.

## 2026-06-07 — Preserve user ideas in plans

**Source summary:** User reported that prior thoughts were not written to
`.codex/plans` and asked Codex to update configuration so future plans or ideas
are not lost.

**Known requirements:**
- Capture every project idea in `.codex/plans`.
- Preserve incomplete ideas instead of relying on conversation memory.
- Update existing plan records when the user expands or corrects an idea.

**Open questions:**
- None for the process change.

**Status:** Converted into project instruction update.

## 2026-06-07 — Correct project run command

**Source summary:** User clarified that this Tauri project should be run with
`npm run tauri dev`, as documented in `README.md`, instead of plain
`npm run dev`.

**Known requirements:**
- Use `npm run tauri dev` to start the development desktop app.
- Use README.md as the source of truth for project run commands.
- Stop expecting `npm run lint` unless a lint script is added later.

**Open questions:**
- None for the command correction.

**Status:** Converted into project instruction update.
