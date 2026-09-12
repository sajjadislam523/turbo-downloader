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

## ADR-003 — 2026-07-11 — Force GDK_BACKEND=x11 to work around Wayland/GTK3 crashes

**Context:** The app froze and showed "not responding" on Ubuntu 26.04 with native Wayland (`GDK_BACKEND=wayland`). The terminal printed `"Couldn't get key from code: AudioVolumeMute"` — a debug message from `tao`'s keyboard handler when it can't map media keys, but the real crash was the WebKit/GTK3 event loop stalling on Wayland. This is a known pattern across many Tauri v2 apps on Wayland.

**Decision:** Set `std::env::set_var("GDK_BACKEND", "x11")` at the top of `main.rs` before any GTK/WebKit initialization, forcing the app to run through XWayland.

**Rejected alternatives:**
- `WEBKIT_DISABLE_COMPOSITING_MODE=1` — addresses GPU compositing crashes but does not fix the Wayland event loop hang.
- Upgrading `tao`/`wry` — the issue is in GTK3's Wayland support, not Tauri's crates specifically. Tauri is not on GTK4 yet.
- Removing `rfd` — the `rfd` file dialog crate triggers the freeze fastest, but the underlying GTK3/Wayland issue affects all window operations.

**Consequences:** XWayland must be available on the user's system (it is on stock Ubuntu). The app cannot run on a pure-Wayland-only compositor that lacks XWayland (e.g. some embedded setups). No changes to Tauri config, desktop file, or build scripts needed.

---

## ADR-004 — 2026-09-12 — Bounded worker-pool download queue + custom-URL-scheme browser bridge

**Context:** The app enforced exactly one download at a time via a single `busy: AtomicBool` guard (`reserve_download_slot`, added to fix Bug 8) and a single `running_pid: Mutex<Option<u32>>`; batch/keyword modes only ever downloaded sequentially through `spawn_sequential_downloads`. The user wants true multi-video concurrent downloading from all three modes (single, batch, keyword), plus a Chrome extension to send URLs to the app without copy-pasting.

**Decision:**
- Replace the single-slot model with a `QueueState` holding a `Vec<QueueItem>` and a bounded dispatcher/worker-pool admitting up to a fixed, user-configurable `max_concurrency` (default 3, range 1-8) — not automatic bandwidth-sensing. A per-host concurrency cap (one in-flight item per host) and per-host cooldown (replacing the old global 4-8s inter-item sleep) prevent a burst of parallel connections against a single site.
- Bridge the browser extension to the desktop app via a custom `turbodl://` URL scheme (`tauri-plugin-deep-link` + `tauri-plugin-single-instance`), not a local HTTP/WebSocket server and not Chrome Native Messaging.

**Rejected alternatives:**
- *Adaptive bandwidth-sensing concurrency* — automatically growing/shrinking concurrent downloads based on measured throughput. Rejected for the initial build as meaningfully more complex to build and tune correctly than a fixed user-set limit; recorded as future work instead.
- *Local HTTP/WebSocket server for the extension bridge* — would give the smoothest UX (no browser confirmation prompt) but adds real attack surface (needs pairing-token auth, loopback-only binding, and a CSP change to the currently locked-down `connect-src 'self'`), requiring a formal security review before shipping.
- *Chrome Native Messaging host* — Google's own sanctioned mechanism (stdio, no network port) and arguably the most "correct" choice, but requires installing and registering a native-messaging manifest, which is disproportionate packaging overhead for a Linux-only, initially-sideloaded extension.

**Consequences:** The single `busy`/`running_pid` fields and `spawn_sequential_downloads` are removed outright rather than kept alongside the queue, since running both admission models concurrently would reintroduce the overlapping-download race Bug 8 fixed. Every new shared-state `Mutex` (queue items, per-item PIDs, per-host counters) must keep the existing poison-tolerant locking pattern (`pid_lock`), since the crate builds with `panic = "abort"` (Bug 12). The deep-link bridge shows the browser's native "Open TurboDL Ultra?" confirmation per link (or "always allow" once opted in) — an accepted tradeoff for avoiding new network-facing attack surface. Any deep-link payload is OS-level, attacker-reachable input and must be strictly validated (scheme allow-list, reject values starting with `-` before they reach `Command::arg`, size-capped) before being used. See `project-docs/plans/parallel-download-queue-and-browser-extension.md` for the full phased implementation plan.

---
