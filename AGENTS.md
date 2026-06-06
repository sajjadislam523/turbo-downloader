# AGENTS.md — turbo-downloader

## Log folder
All Codex logs for this project live in .codex/
- .codex/CHANGES.md     → rolling changelog
- .codex/DECISIONS.md   → architectural decisions
- .codex/REVIEW.md      → items for human review
- .codex/sessions/      → per-session detail files
- .codex/plans/         → feature plan files

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
- npm run dev       (development server — confirm it starts)
- npm run build     (production build — must pass)
- npm run lint      (linting — must pass)

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
