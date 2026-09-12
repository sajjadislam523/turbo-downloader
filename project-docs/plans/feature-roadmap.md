# Feature Roadmap

Status: IDEA / PLANNING — this is a backlog, not an in-progress implementation. Nothing here has been started as of this writing. Items are grouped by area, roughly ordered by impact within each group. None of this blocks anything else — pick items independently.

This roadmap complements, rather than replaces, the "Future work" list already in `project-docs/plans/parallel-download-queue-and-browser-extension.md` (resume-after-restart, drag-reorder/priority, download history, Firefox/Edge extension parity, adaptive bandwidth throttling, per-site 429-aware backoff, desktop notifications, off-peak scheduling, duplicate-URL detection) — those items aren't repeated here in full, just cross-referenced where relevant.

## 1. Download quality-of-life (new yt-dlp-backed capabilities)

None of these need new infrastructure — `yt-dlp` already supports all three; the app just doesn't expose them yet. All output is currently forced to MP4 via `--merge-output-format mp4 --remux-video mp4` (`build_height_selector` in `lib.rs`), so each of these needs a UI toggle plus a corresponding change to the command-building code in `queue.rs`/`lib.rs`.

- **Audio-only extraction** — an "Audio only (MP3/M4A)" toggle next to the resolution picker, mapping to `-x --audio-format mp3` (or `m4a` to skip transcoding). Useful for podcasts/music-adjacent content where video is wasted bandwidth.
- **Subtitle download/embedding** — a checkbox to fetch subtitles (`--write-subs --sub-langs ...`) and optionally burn them in (`--embed-subs`).
- **Thumbnail/metadata embedding** — `--embed-thumbnail --embed-metadata`, likely worth defaulting to *on* since it's low-cost and most users want it.

## 2. Queue robustness

- **Persist the queue across app restarts.** Currently `QueueState` is in-memory only (`queue.rs`) — closing the app loses any pending/paused items. This is explicitly called out as future work in the queue-engine plan doc; the natural approach is serializing `QueueState` to a JSON file on every mutation (or on a debounce/interval) and reloading it on startup, re-validating that in-flight items' partial files still exist before resuming them.
- **Drag-to-reorder / priority.** Right now queue order is purely insertion order with no way to bump an item up. Needs a stable per-item `order` field the dispatcher respects when picking the next `Pending` item to admit.
- **Download history.** Once an item is cleared from the completed list it's gone for good — no record of what was downloaded, when, or where it landed. A simple append-only history log (or a "keep last N completed" option instead of an all-or-nothing Clear Completed) would help.

## 3. UX polish

- **Restore full settings persistence.** `localStorage`'s `turbodl.settings.v1` today only persists `{ mode, savePath }`. An earlier version (per `project-docs/ISSUES.md`) also persisted `batchResolution`, `keywordLimit`, and `keywordResolution` — these now reset to defaults every launch, which is a regression worth restoring in the current per-panel component structure (`BatchModePanel.tsx`, `KeywordModePanel.tsx`).
- **Desktop notifications on completion.** A native notification (Tauri's notification API) when a download finishes or fails, so the app doesn't need to stay in focus/visible to know something completed.
- **Keyword mode's full match-list picker.** Currently keyword search auto-queues its top N matches with no way to review and deselect individual videos first (tracked as "Phase 2 item 4, deferred" in the queue-engine plan doc). A checkbox list between "Check Now" validation and actually queuing would close this gap — this is the single most-requested-shaped gap based on how often it's called out across `README.md`'s "Known limitation" note and the plan doc itself.

## 4. Browser extension

- **Firefox/Edge parity.** The extension is Chrome MV3 only today; Edge already works since it's Chromium-based, but Firefox needs its own manifest (MV2/MV3 hybrid) and testing.
- **Multi-select bulk send from a page.** The backend's `add-batch` deep-link path already exists (per the queue-engine plan doc, Phase 4 item 1's backend half is done) but the extension's own UI has no way to select multiple links on a page and send them all at once yet — only one-at-a-time context-menu/toolbar sends.
- **Live-verify in a real browser.** Per the plan doc, the extension "has not been exercised in a real browser in this environment" — the redirect-page/deep-link flow has been reasoned through and unit-adjacent-tested but never actually clicked through in Chrome. Worth a manual pass before calling it stable.

## 5. Testing / confidence

`project-docs/REVIEW.md` documents that a number of recent fixes (the keyword-download freeze fix, the link-scan false-positive fix, the process-group/CSP changes) were verified only via `cargo build`/`cargo test`/`npm run build` in a sandboxed environment with no display or live network access — never actually clicked through in a running GUI session or tested against a real target site. A lightweight recurring QA pass (or eventually a basic Playwright/WebDriver smoke test covering "paste a URL → download completes") would close this gap and catch regressions the build alone can't.
