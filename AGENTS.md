# AGENTS.md — turbo-downloader

## Log folder
All Codex logs for this project live in .codex/
- .codex/CHANGES.md     → rolling changelog
- .codex/DECISIONS.md   → architectural decisions
- .codex/REVIEW.md      → items for human review
- .codex/sessions/      → per-session detail files
- .codex/plans/         → feature plan files

## Idea capture protocol
Codex must preserve every user feature thought before implementation detail is
settled.

When the user describes an idea, feature, workflow, product direction, or rough
plan for this project:
- Create `.codex/plans/` if it does not exist.
- If the idea is not ready for a full feature plan, append it to
  `.codex/plans/idea-inbox.md` with the date, source message summary,
  known requirements, open questions, and current status.
- If the idea is ready for implementation, create or update the relevant
  `.codex/plans/FEATURE-SLUG.md` file before editing application code.
- If the user changes or expands an earlier idea, update the same plan or inbox
  entry instead of relying on conversation memory.
- Never discard an idea because it is incomplete. Mark unclear parts as open
  questions.

## Project architecture
Single React application (frontend only).

Folder structure:
- /src/            → Source code
- /components/     → React/Vue components
- /lib/            → Utilities and helpers
- /pages/          → Page components (if using pages router)
- /app/            → App router pages (if using app router)
- /public/         → Static assets

Core rules:
- All code must be TypeScript
- Single application — no separate backend
- If backend is needed, create a /server folder or separate project

## Commands to run after every change
- npm run tauri dev       (desktop development app — confirm it starts)
- npm run tauri build     (production desktop build — must pass)

## Project metadata (detected by codex-init)
- Type: single_app
- Structure: frontend_only
- Framework: React
- Language: TypeScript
- Build tool: npm
- Monorepo: no

## Notes for Codex
Codex will follow these rules when working on this project:
- Respect the folder structure described above — never mix frontend and backend code
- Run the commands listed above after every change to verify nothing broke
- Log every session to .codex/sessions/ with details about what changed
- Update .codex/CHANGES.md and .codex/DECISIONS.md as work progresses
