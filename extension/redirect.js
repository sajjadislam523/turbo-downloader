// This page is a real, fully-loaded tab (unlike the background service
// worker, which has no `window` at all) — that's what makes it a reliable
// place to trigger the turbodl:// external-protocol prompt. It tries an
// automatic navigation first (works in most browsers/configurations), and
// always shows a manual link as a fallback: an actual user click on an
// <a href="turbodl://..."> is the single most universally-honored way
// browsers trigger an external-protocol handoff, more reliable than a
// programmatic `window.location.href` assignment in some browser versions.
const params = new URLSearchParams(window.location.search);
const targetUrl = params.get("url");
const targetEl = document.getElementById("target");
const statusEl = document.getElementById("status");
const linkEl = document.getElementById("open-link");

if (!targetUrl) {
    targetEl.textContent = "(nothing to send)";
    statusEl.textContent = "No URL was passed to this page.";
} else {
    targetEl.textContent = targetUrl;
    const deepLink = `turbodl://add?url=${encodeURIComponent(targetUrl)}`;
    linkEl.href = deepLink;
    statusEl.textContent = "Attempting automatic open…";
    setTimeout(() => {
        window.location.href = deepLink;
    }, 150);
}
