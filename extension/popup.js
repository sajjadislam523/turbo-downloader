const statusEl = document.getElementById("status");

document.getElementById("send").addEventListener("click", async () => {
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    if (!tab || !tab.url) {
        statusEl.textContent = "No active tab URL to send.";
        return;
    }
    chrome.runtime.sendMessage({ type: "send-to-turbodl", url: tab.url });
    statusEl.textContent = "Opening a tab to complete the send…";
    setTimeout(() => window.close(), 800);
});
