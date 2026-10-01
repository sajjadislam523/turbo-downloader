// Sends a URL to the TurboDL Ultra desktop app via its turbodl:// deep link
// (see handle_incoming_deep_link in src-tauri/src/lib.rs).
//
// The OS-level registration and the app's own deep-link handling are known
// good (verified directly with `gio open turbodl://...`, the mechanism
// GTK/Chrome actually use on Linux). What this file does is open the
// extension's own redirect.html as a new tab rather than trying to navigate
// straight to the turbodl:// URL from here — a service worker has no
// `window`, and chrome.tabs.create/update won't reliably navigate directly
// to an arbitrary custom scheme either. redirect.html is a real, fully-
// loaded page (chrome-extension://<id>/redirect.html, which chrome.tabs
// .create always allows since it's the extension's own resource) that
// attempts the turbodl:// navigation itself and also shows a manual,
// click-through fallback link — an actual click on an anchor pointing at an
// unknown scheme is the most universally-honored way to trigger a browser's
// external-protocol prompt.
//
// The helper tab used to stay open indefinitely (it has nothing left to do
// once the OS handoff fires, but nothing ever closed it). It now opens in
// the background (doesn't steal focus from whatever tab you were on) and
// closes itself once redirect.js reports the handoff was attempted — either
// right after the automatic attempt, or immediately after a manual
// click-through. A fallback timer here also force-closes it even if that
// message never arrives (e.g. the page failed to load), so a stray tab can
// never pile up.
const TAB_FALLBACK_CLOSE_MS = 4000;

function notifySent(url) {
    // This fires the moment the link is handed off to the OS, not once the
    // app has actually started the download (there's no channel back from
    // the app to the extension to confirm that) — worded as "sent", not
    // "downloading", so it stays accurate.
    chrome.notifications.create({
        type: "basic",
        iconUrl: chrome.runtime.getURL("icons/icon128.png"),
        title: "TurboDL Ultra",
        message: `Link sent — check the app's queue:\n${url}`,
        priority: 1,
    });
}

function closeTab(tabId) {
    if (tabId == null) return;
    chrome.tabs.remove(tabId, () => void chrome.runtime.lastError);
}

function sendToApp(url) {
    notifySent(url);
    // Stays in the foreground (not opened as a background tab): the first
    // time a browser profile sees the turbodl:// scheme it shows an "Open
    // TurboDL Ultra?" permission prompt tied to this tab, which needs to be
    // visible for the user to approve — after that one-time approval,
    // subsequent sends hand off silently and the tab just closes itself
    // almost immediately without being disruptive.
    const redirectUrl = chrome.runtime.getURL("redirect.html") + "?url=" + encodeURIComponent(url);
    chrome.tabs.create({ url: redirectUrl }, (tab) => {
        if (chrome.runtime.lastError || !tab) return;
        setTimeout(() => closeTab(tab.id), TAB_FALLBACK_CLOSE_MS);
    });
}

chrome.runtime.onInstalled.addListener(() => {
    chrome.contextMenus.create({
        id: "turbodl-send-page",
        title: "Send this page to TurboDL",
        contexts: ["page"],
    });
    chrome.contextMenus.create({
        id: "turbodl-send-link",
        title: "Send this link to TurboDL",
        contexts: ["link"],
    });
});

chrome.contextMenus.onClicked.addListener((info) => {
    if (info.menuItemId === "turbodl-send-link" && info.linkUrl) {
        sendToApp(info.linkUrl);
    } else if (info.menuItemId === "turbodl-send-page" && info.pageUrl) {
        sendToApp(info.pageUrl);
    }
});

// Lets popup.js delegate the actual send here, so this logic lives in
// exactly one place. redirect.js also reports back here (from the helper tab
// it runs in) once it has attempted the turbodl:// handoff, so the tab can
// close itself right away instead of waiting out the fallback timer above.
chrome.runtime.onMessage.addListener((message, sender) => {
    if (message?.type === "send-to-turbodl" && message.url) {
        sendToApp(message.url);
    } else if (message?.type === "redirect-done") {
        closeTab(sender.tab?.id);
    }
});
