# TurboDL Ultra — Browser Extension

Sends a page or link to the TurboDL Ultra desktop app via its `turbodl://` deep link, instead of copy-pasting a URL.

See the main project's [README.md § Browser Extension](../README.md#browser-extension) for install and usage instructions.

## Files

- `manifest.json` — Manifest V3 declaration (`contextMenus` + `activeTab` permissions only).
- `background.js` — service worker: creates the two context-menu entries and opens the `turbodl://add?url=...` deep link.
- `popup.html` / `popup.js` — toolbar-button popup with a "Send Current Tab" button, for discoverability without right-clicking.

## Why a script injection to send the link

MV3 service workers have no `window`, so there's no `window.location` to navigate to a custom scheme — and `chrome.tabs.create`/`chrome.tabs.update` reject an arbitrary custom scheme outright (Chrome's tabs API only accepts a small allow-list of schemes for programmatic navigation). What actually makes the browser show its "Open TurboDL Ultra?" prompt is a real page navigating *itself* — so `background.js` uses `chrome.scripting.executeScript` to inject a one-line `window.location.href = deepLink` into the current tab. The page never actually navigates away; the browser intercepts the unknown scheme before that happens, the same way a `mailto:` link doesn't leave the page you're on.

## Not yet implemented

Sending multiple selected links from a page at once (tracked in `project-docs/plans/parallel-download-queue-and-browser-extension.md`, Phase 4) — for now, send links one at a time.
