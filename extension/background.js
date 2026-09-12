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
function sendToApp(url) {
    const redirectUrl = chrome.runtime.getURL("redirect.html") + "?url=" + encodeURIComponent(url);
    chrome.tabs.create({ url: redirectUrl });
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
// exactly one place.
chrome.runtime.onMessage.addListener((message) => {
    if (message?.type === "send-to-turbodl" && message.url) {
        sendToApp(message.url);
    }
});
