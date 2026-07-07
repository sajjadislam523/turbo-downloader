# Architectural decisions — turbo-downloader

Record non-obvious structural decisions here.

## ADR-001 — 2026-06-07 — Disable global allowed_dirs for Codex shell startup

**Context:** Sandboxed command execution failed before commands could run, including `/bin/sh`, `/bin/bash`, and `/usr/bin/zsh`, while unsandboxed shell startup worked.

**Decision:** Comment out the global `allowed_dirs` setting in `/home/kabila/.codex/config.toml` so Codex can use its normal sandbox behavior without hiding system executables.

**Rejected alternatives:** Adding `/bin`, `/usr/bin`, and system library directories to `allowed_dirs` was rejected because it broadens global filesystem configuration more than needed and still risks missing dynamically loaded paths.

**Consequences:** Codex must be restarted to reload the config. Project-level trust and per-session sandbox writable roots remain the effective protection for edits.

---

## ADR-002 — 2026-06-07 — Capture incomplete ideas in a plan inbox

**Context:** The project already required full feature plans, but incomplete user ideas could be lost if they were not ready for implementation planning.

**Decision:** Add an idea capture protocol to `AGENTS.md` and keep early or incomplete ideas in `.codex/plans/idea-inbox.md` until they can be promoted to a feature plan.

**Rejected alternatives:** Relying only on conversation history was rejected because context can be compacted or unavailable in later sessions. Creating full feature plans for every rough idea was rejected because unclear ideas need open questions before step-by-step execution.

**Consequences:** Future Codex sessions must update `.codex/plans` whenever the user shares or changes a project idea, even before application code is touched.

---
