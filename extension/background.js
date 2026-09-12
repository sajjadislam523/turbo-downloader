// Sends a URL to the TurboDL Ultra desktop app via its turbodl:// deep link
// (see handle_incoming_deep_link in src-tauri/src/lib.rs).
//
// MV3 service workers have no `window`, so `window.location` isn't
// available here — and `chrome.tabs.create`/`chrome.tabs.update` reject an
// arbitrary custom scheme outright (Chrome's tabs API only accepts a small
// allow-list of schemes for programmatic navigation; a `turbodl://` URL is
// silently refused, which is why simply calling chrome.tabs.create with the
// deep link never actually opened anything). The one thing that DOES make a
// browser show its "Open TurboDL Ultra?" external-protocol prompt is a real
// page navigating itself to that scheme — so this injects a one-line script
// into the current tab (a real page context, with a real `window`) that
// sets `window.location.href` to the deep link. The page itself never
// actually navigates away (the browser intercepts the unknown scheme before
// that happens), the same way clicking a `mailto:` link doesn't leave the
// page you're on.
function sendToApp(url, tabId) {
    if (!tabId) {
        flashBadge("!", "#ff4455");
        console.error("TurboDL: no target tab to send from");
        return;
    }
    const deepLink = `turbodl://add?url=${encodeURIComponent(url)}`;
    chrome.scripting
        .executeScript({
            target: { tabId },
            func: (link) => {
                window.location.href = link;
            },
            args: [deepLink],
        })
        .then(() => flashBadge("✓", "#00ff99"))
        .catch((err) => {
            console.error("TurboDL: failed to open deep link", err);
            flashBadge("✗", "#ff4455");
        });
}

function flashBadge(text, color) {
    chrome.action.setBadgeText({ text });
    chrome.action.setBadgeBackgroundColor({ color });
    setTimeout(() => chrome.action.setBadgeText({ text: "" }), 2000);
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

chrome.contextMenus.onClicked.addListener((info, tab) => {
    const tabId = tab?.id;
    if (info.menuItemId === "turbodl-send-link" && info.linkUrl) {
        sendToApp(info.linkUrl, tabId);
    } else if (info.menuItemId === "turbodl-send-page" && info.pageUrl) {
        sendToApp(info.pageUrl, tabId);
    }
});

// Lets popup.js delegate the actual send here, so the deep-link-opening
// logic lives in exactly one place. The popup has to pass its own tabId
// explicitly — a message from the popup has no `sender.tab` the way a
// content-script message would.
chrome.runtime.onMessage.addListener((message) => {
    if (message?.type === "send-to-turbodl" && message.url && message.tabId) {
        sendToApp(message.url, message.tabId);
    }
});
