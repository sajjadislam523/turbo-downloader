# TurboDL Ultra — Browser Extension

Sends a page or link to the TurboDL Ultra desktop app via its `turbodl://` deep link, instead of copy-pasting a URL.

See the main project's [README.md § Browser Extension](../README.md#browser-extension) for install and usage instructions.

## Files

- `manifest.json` — Manifest V3 declaration (`contextMenus` + `activeTab` permissions only — no `scripting`, no `host_permissions`).
- `background.js` — service worker: creates the two context-menu entries and opens `redirect.html` as a new tab with the target URL attached.
- `redirect.html` / `redirect.js` — the page that actually triggers the `turbodl://add?url=...` deep link (see below).
- `popup.html` / `popup.js` — toolbar-button popup with a "Send Current Tab" button, for discoverability without right-clicking; delegates to `background.js`.

## Why a dedicated redirect page

MV3 service workers have no `window`, so `background.js` can't navigate to a custom scheme directly — and `chrome.tabs.create`/`chrome.tabs.update` won't reliably navigate straight to an arbitrary unknown scheme either. What a browser actually needs to show its "Open TurboDL Ultra?" prompt is a **real, loaded page** attempting (or being clicked toward) that navigation. `redirect.html` is exactly that: an ordinary extension page (`chrome-extension://<id>/redirect.html`, which `chrome.tabs.create` always allows since it's the extension's own resource) that both attempts an automatic `window.location.href = deepLink` navigation on load *and* shows a manual "Open TurboDL Ultra" button — an actual click on an `<a href="turbodl://...">` is the most universally-honored way browsers trigger an external-protocol handoff, so it's kept as a guaranteed fallback rather than relying on the automatic attempt alone.

Verified independently that the OS-level and app-side halves of this are correct: `xdg-mime query default x-scheme-handler/turbodl` resolves to the app's registered `.desktop` file, and `gio open 'turbodl://add?url=...'` — the mechanism GTK/Chrome actually use on Linux for this, not the `xdg-open` CLI script — correctly launches the app with the link. (An `xdg-open` CLI test can be misleading here: on at least one Linux desktop setup, its own desktop-file search path didn't include `~/.local/share/applications` and it silently fell back to opening the link in a plain browser instead — a red herring unrelated to whether the actual OS/browser integration works.) What was NOT independently verified is the extension's own `chrome.tabs.create`/redirect-page behavior inside a real Chrome — there's no browser available in the environment this was built in.

## Not yet implemented

Sending multiple selected links from a page at once (tracked in `project-docs/plans/parallel-download-queue-and-browser-extension.md`, Phase 4) — for now, send links one at a time.
