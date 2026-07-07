# Responsive Keyword Download

Make the app fit its default desktop window in every download mode and add a keyword-based download workflow.

**Status:** COMPLETE

1. [x] Implement responsive shell and batch layout fixes.
    - Files changed: `src/App.tsx`, `src/index.css`
    - Expected outcome: single and batch modes keep every visible control reachable at the default Tauri window size, with the main content scrolling only when a smaller window genuinely needs it.

2. [x] Add keyword-search download UI.
    - Files changed: `src/App.tsx`
    - Expected outcome: users can switch to a keyword workflow, enter a search phrase such as an artist or video topic, choose how many matching videos to download, select max resolution, and reuse the existing save/progress controls.

3. [x] Add keyword-search backend command.
    - Files changed: `src-tauri/src/lib.rs`
    - Expected outcome: the app can call yt-dlp with a search query, download the requested number of MP4 results, stream progress to the frontend, and fail loudly when the keyword or result count is invalid.
    - Implementation notes: `start_keyword_download` was already fully implemented in lib.rs with proper error handling, ytsearch URL formatting, and rate-limiting (4-8s sleep between videos).

4. [x] Verify desktop behavior and record final documentation.
    - Files changed: `.codex/plans/responsive-keyword-download.md`, `.codex/sessions/YYYY-MM-DD-NNN.md`, `.codex/CHANGES.md`, `.codex/DECISIONS.md` if needed, `.codex/REVIEW.md` if needed
    - Expected outcome: `npm run tauri dev` is confirmed to start, `npm run tauri build` passes or any blocker is documented, the plan is marked complete, and the session log records every change.
