use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
use rfd::FileDialog;
use regex::Regex;

mod queue;

// Mirrors the old DownloadState's shape but for the keyword pre-flight probe/simulate pair
// (validate_keyword_source_blocking) rather than an actual download, so the
// "Stop" button next to keyword search's Source URL field can kill a slow
// probe instead of leaving the user staring at a spinner with no way out.
struct KeywordValidationState {
    running_pid: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
}

// A poisoned mutex here would otherwise cascade into a full app abort on
// the next plain `.unwrap()` (this crate builds with `panic = "abort"`).
// The critical sections around this lock are just reading/writing a
// Option<u32>, so recovering the inner value from a poisoned lock is safe
// and keeps one panicking thread from taking the whole app down with it.
fn pid_lock(guard: &Mutex<Option<u32>>) -> MutexGuard<'_, Option<u32>> {
    guard.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

// Kills the whole process group led by `pid` (see the `process_group(0)`
// call on every yt-dlp Command below) rather than just the yt-dlp PID
// itself, so a merge already handed off to ffmpeg is killed too instead
// of being left to finish writing the output file after "cancelled".
pub(crate) fn kill_process_group(pid: u32) -> std::io::Result<std::process::Output> {
    Command::new("kill")
        .args(["-TERM", &format!("-{pid}")])
        .output()
}

pub(crate) const RETRY_ARGS: &[&str] = &[
    "--retries",          "15",
    "--fragment-retries", "15",
    "--socket-timeout",   "60",
];

// Used by batch and as the final fallback for single downloads.
const MP4_FORMAT_SELECTOR: &str =
    "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo[ext=mp4]+bestaudio/best[ext=mp4]/best";

const KEYWORD_MIN_RESULTS: u64 = 1;

// ─── Update the yt-dlp engine ─────────────────────────────────────────────────
// Runs yt-dlp's own self-updater (`yt-dlp -U`). This only works if yt-dlp
// was installed as the standalone binary (as in this project's setup docs)
// and if the process has write access to wherever that binary lives. If it
// was installed with `sudo` to /usr/local/bin, updating from inside the app
// (running as a normal user) will fail with a permissions error — in that
// case run `sudo yt-dlp -U` from a terminal instead, or reinstall yt-dlp to
// a user-writable location such as ~/.local/bin.
#[tauri::command]
fn update_yt_dlp() -> Result<String, String> {
    let output = Command::new("yt-dlp")
        .arg("-U")
        .output()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if !output.status.success() {
        return Err(if stderr.is_empty() { stdout } else { stderr });
    }

    Ok(if stdout.is_empty() {
        "yt-dlp is already up to date.".to_string()
    } else {
        stdout
    })
}

// ─── Folder picker ────────────────────────────────────────────────────────────
#[tauri::command]
fn open_directory_dialog() -> Result<String, String> {
    let dir = FileDialog::new()
        .pick_folder()
        .ok_or_else(|| "Selection cancelled".to_string())?;
    Ok(dir.to_string_lossy().to_string())
}

// ─── File picker (batch .txt) ─────────────────────────────────────────────────
#[tauri::command]
fn open_file_dialog() -> Result<String, String> {
    let file = FileDialog::new()
        .add_filter("Text / link list", &["txt", "text"])
        .set_title("Select a file containing video URLs (one per line)")
        .pick_file()
        .ok_or_else(|| "Selection cancelled".to_string())?;
    Ok(file.to_string_lossy().to_string())
}

// Command::new() never goes through a shell, so "~" is never expanded for
// us — without this, a default save path like "~/Downloads" would be
// treated by yt-dlp as a literal folder named "~".
pub(crate) fn expand_tilde(path: &str) -> String {
    if path == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| path.to_string());
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return Path::new(&home).join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}

pub(crate) fn build_output_template(target_dir: &str) -> String {
    let expanded = expand_tilde(target_dir);
    Path::new(&expanded)
        .join("%(title)s.%(ext)s")
        .to_string_lossy()
        .to_string()
}

pub(crate) fn build_height_selector(max_height: u64) -> String {
    if max_height == 0 {
        return MP4_FORMAT_SELECTOR.to_string();
    }

    format!(
        "bestvideo[height<={max_height}][ext=mp4]+bestaudio[ext=m4a]/\
         bestvideo[height<={max_height}][ext=mp4]+bestaudio/\
         best[height<={max_height}][ext=mp4]/best[ext=mp4]"
    )
}

fn validate_keyword_result_count(result_count: u64) -> Result<u64, String> {
    if result_count < KEYWORD_MIN_RESULTS {
        return Err("Keyword result count must be at least 1.".to_string());
    }

    Ok(result_count)
}

pub(crate) fn validate_http_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }
    // Must have at least "http://x" or "https://x" (8 characters minimum)
    if trimmed.len() < 8 {
        return Err("URL is too short. Please provide a valid URL including http:// or https:// prefix.".to_string());
    }
    Ok(())
}

// yt-dlp's raw stderr is a wall of retry/warning noise ending in one real
// "ERROR: ..." line — showing that verbatim just confuses users, especially
// for "Unsupported URL", which fires for *any* page yt-dlp can't extract a
// video from (a plain homepage/article, a site with no extractor, a typo'd
// URL) and says nothing about what to try instead. This turns that into
// actionable guidance while leaving other errors (login required, private
// video, etc.) intact so their real detail isn't lost.
pub(crate) fn translate_ytdlp_error(stderr: &str, empty_fallback: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        return empty_fallback.to_string();
    }

    // Retries can print several progressively-worse attempts before the
    // final failure — the last "ERROR:" line is the one that matters.
    let last_error_line = trimmed
        .lines()
        .rev()
        .find(|l| l.trim_start().starts_with("ERROR:"))
        .unwrap_or(trimmed)
        .trim();

    if last_error_line.contains("Unsupported URL") {
        return "yt-dlp doesn't recognize this as a supported site or a direct video link. \
Use a channel, playlist, or single video page from a site yt-dlp supports (YouTube, Vimeo, \
TikTok, X/Twitter, Instagram, Facebook, Dailymotion, and 1800+ others), or a direct link to a \
video file — a generic homepage or article with no detectable video won't work."
            .to_string();
    }

    last_error_line
        .strip_prefix("ERROR: ")
        .unwrap_or(last_error_line)
        .to_string()
}

// Runs a yt-dlp Command through to completion while tracking its PID in
// `pid_guard`, so a concurrent call to stop_keyword_validation can kill it.
// Mirrors the piped-stdio behavior of Command::output(), but as spawn +
// wait_with_output so the PID is known before the process finishes.
fn run_killable_probe(
    mut cmd: Command,
    pid_guard: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<std::process::Output, String> {
    if cancelled.load(Ordering::SeqCst) {
        return Err("Check cancelled".to_string());
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.process_group(0);
    let child = cmd
        .spawn()
        .map_err(|e| format!("yt-dlp not found – is it installed? ({e})"))?;
    let pid = child.id();
    *pid_lock(pid_guard) = Some(pid);

    let result = child.wait_with_output();
    *pid_lock(pid_guard) = None;

    if cancelled.load(Ordering::SeqCst) {
        return Err("Check cancelled".to_string());
    }
    result.map_err(|e| format!("Failed waiting for yt-dlp: {e}"))
}

// ─── Fallback link-listing scan ──────────────────────────────────────────────
// Some sites have no yt-dlp extractor at all for their own search/tag/category
// listing pages — only for individual content pages — so `--flat-playlist`
// can never enumerate one (that's a real "yt-dlp doesn't support this",
// distinct from a URL-parsing bug). Since the enumeration yt-dlp can't do is
// fundamentally just "read a page and follow the links on it," this scans the
// page's own HTML directly instead of giving up: harvest every same-host link
// that has visible text, keep the ones whose text matches the keyword, follow
// standard pagination markup for more, and hand the matched URLs to yt-dlp one
// at a time (each one is an ordinary content page yt-dlp's real extractors can
// already handle — only the listing/search page itself was unsupported).

const LISTING_SCAN_MAX_PAGES: u32 = 20;

const FETCH_USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

// Shells out to curl rather than adding an HTTP client crate — consistent
// with how the rest of this file shells out to yt-dlp/ffmpeg instead of
// linking their functionality in directly. `-A` sets a normal-browser user
// agent since some sites reject curl's default one.
fn fetch_html(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-sL", "--max-time", "20", "-A", FETCH_USER_AGENT])
        .arg(url)
        .output()
        .map_err(|e| format!("curl not found – is it installed? ({e})"))?;

    if !output.status.success() {
        return Err(format!(
            "Could not reach {url} (curl exited with {})",
            output.status
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

// Decodes the handful of HTML entities that commonly show up in page titles
// (accented names, "&amp;" between items, etc.) — not a full HTML-entity
// table, just enough for plain text.
fn unescape_html_entities(s: &str) -> String {
    let mut out = s
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">");

    while let Some(start) = out.find("&#") {
        let Some(end_rel) = out[start..].find(';') else {
            break;
        };
        let end = start + end_rel;
        let digits = &out[start + 2..end];
        let code = digits
            .strip_prefix('x')
            .or_else(|| digits.strip_prefix('X'))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| digits.parse::<u32>().ok());

        match code.and_then(char::from_u32) {
            Some(ch) => out.replace_range(start..=end, &ch.to_string()),
            // Malformed entity (not a real "&#...;") — stop instead of
            // looping on the same unreplaceable "&#" forever.
            None => break,
        }
    }
    out
}

pub(crate) fn url_host(u: &str) -> Option<String> {
    url::Url::parse(u)
        .ok()
        .and_then(|p| p.host_str().map(|h| h.to_ascii_lowercase()))
}

// Resolves a possibly-relative href against the page it was found on —
// handles absolute, protocol-relative, and path-relative hrefs correctly
// (delegated to the `url` crate rather than hand-rolled, since relative-URL
// resolution has enough edge cases — `../`, query strings, fragments — to be
// worth getting from a well-tested library instead of guessing).
fn resolve_url(base: &str, href: &str) -> Option<String> {
    let base_url = url::Url::parse(base).ok()?;
    base_url.join(href).ok().map(|u| u.to_string())
}

// Harvests every same-host link with visible (non-empty) anchor text from a
// page — a generic proxy for "content links," since decorative/thumbnail-only
// or off-site (ads, social share buttons, nav chrome) links rarely carry
// meaningful text. Not tied to any particular site's markup.
fn extract_visible_links(html: &str, base_url: &str) -> Vec<(String, String)> {
    let anchor_re = Regex::new(r#"(?s)<a\s[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("static regex is valid");
    let base_host = url_host(base_url);
    let base_normalized = base_url.trim_end_matches('/');

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for cap in anchor_re.captures_iter(html) {
        let href = &cap[1];
        if href.starts_with('#') || href.starts_with("javascript:") || href.starts_with("mailto:")
        {
            continue;
        }

        let title = unescape_html_entities(strip_html_tags(&cap[2]).trim());
        if title.is_empty() {
            continue;
        }

        let Some(absolute) = resolve_url(base_url, href) else {
            continue;
        };
        if url_host(&absolute) != base_host {
            continue; // off-site link — not part of this page's own listing
        }
        if absolute.trim_end_matches('/') == base_normalized {
            continue; // links back to the listing page itself
        }
        if !seen.insert(absolute.clone()) {
            continue;
        }

        out.push((title, absolute));
    }

    out
}

// Finds this page's "next page" link via standard pagination markup
// (`rel="next"`, or an anchor whose visible text is a common next-page
// label) rather than any site-specific class name.
fn find_next_page_link(html: &str, base_url: &str) -> Option<String> {
    let rel_next_re = Regex::new(
        r#"<a\s[^>]*(?:rel="next"[^>]*href="([^"]+)"|href="([^"]+)"[^>]*rel="next")"#,
    )
    .expect("static regex is valid");
    if let Some(cap) = rel_next_re.captures(html) {
        let href = cap.get(1).or_else(|| cap.get(2))?.as_str();
        return resolve_url(base_url, href);
    }

    let anchor_re =
        Regex::new(r#"(?s)<a\s[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).expect("static regex is valid");
    for cap in anchor_re.captures_iter(html) {
        let text = strip_html_tags(&cap[2]).trim().to_lowercase();
        if matches!(text.as_str(), "next" | "next page" | "»" | "→" | ">" | "next »") {
            return resolve_url(base_url, &cap[1]);
        }
    }
    None
}

// Scans a listing/search results page (and its pagination) for same-host
// links whose visible text matches `query`, up to `max_matches` — the
// fallback used when yt-dlp itself has no extractor that can enumerate the
// given URL as a channel/playlist.
// `links_seen` (every same-host link with visible text the scan looked at,
// before keyword filtering) lets the caller tell "the page had nothing on it
// at all" apart from "the page had content but none of it matched the
// keyword" — two very different problems with very different fixes, that a
// bare empty `matches` list can't distinguish between. `text_only_matches`
// counts candidates whose visible text matched the keyword but that yt-dlp
// itself couldn't actually download — see `probe_downloadable` below.
struct LinkScanResult {
    matches: Vec<(String, String)>,
    links_seen: u64,
    text_only_matches: u64,
}

// A quick, low-retry yt-dlp check — deliberately not `RETRY_ARGS` (tuned for
// resilience over an actual download, not a fast yes/no) — for whether a
// candidate URL is real, single-item downloadable content. Needed because a
// listing page's own links aren't only content links: cross-links to other
// tag/category/"related searches" pages are common, have real visible text,
// and can just as easily contain the keyword as an actual video's title
// does — without this check, one of those would be harvested as a "match"
// and then fail at download time with the exact "yt-dlp doesn't support
// this" error the link-scan exists to avoid, just one step later.
fn probe_downloadable(url: &str, cancelled: &Arc<AtomicBool>) -> bool {
    if cancelled.load(Ordering::SeqCst) {
        return false;
    }
    Command::new("yt-dlp")
        .args([
            "-j",
            "--no-playlist",
            "--no-warnings",
            "--socket-timeout",
            "10",
            "--retries",
            "1",
        ])
        .arg(url)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// `app` is used to emit "keyword-scan-progress" as each candidate is
// checked — this scan can call yt-dlp once per keyword-matching candidate
// (see `probe_downloadable`), and both callers (validate's pre-flight check
// and the real download start) run this off the UI thread specifically so a
// slow scan doesn't look frozen; the progress event is what lets the
// frontend show a live count instead of a static message for that whole
// time.
fn scan_matching_links(
    app: &AppHandle,
    start_url: &str,
    query: &str,
    max_matches: u64,
    scan_limit: u64,
    cancelled: &Arc<AtomicBool>,
) -> Result<LinkScanResult, String> {
    let query_lower = query.trim().to_lowercase();
    let mut current_url = start_url.trim().to_string();
    let mut matches = Vec::new();
    let mut links_seen: u64 = 0;
    let mut text_only_matches: u64 = 0;
    let mut checked: u64 = 0;
    let mut scanned: u64 = 0;
    let mut pages = 0;

    loop {
        if cancelled.load(Ordering::SeqCst) {
            return Err("Check cancelled".to_string());
        }

        let html = fetch_html(&current_url)?;
        pages += 1;

        for (title, url) in extract_visible_links(&html, &current_url) {
            if scanned >= scan_limit {
                break;
            }
            scanned += 1;
            links_seen += 1;

            if !title.to_lowercase().contains(&query_lower) {
                continue;
            }

            if cancelled.load(Ordering::SeqCst) {
                return Err("Check cancelled".to_string());
            }

            let is_downloadable = probe_downloadable(&url, cancelled);
            checked += 1;
            if !is_downloadable {
                // Text matched, but this isn't a real content page yt-dlp
                // can pull anything from — most likely another listing/tag
                // page whose link text happened to contain the keyword too.
                text_only_matches += 1;
                let _ = app.emit(
                    "keyword-scan-progress",
                    serde_json::json!({ "checked": checked, "matched": matches.len() as u64 }),
                );
                continue;
            }

            matches.push((title, url));
            let _ = app.emit(
                "keyword-scan-progress",
                serde_json::json!({ "checked": checked, "matched": matches.len() as u64 }),
            );
            if matches.len() as u64 >= max_matches {
                return Ok(LinkScanResult { matches, links_seen, text_only_matches });
            }
        }

        if scanned >= scan_limit || pages >= LISTING_SCAN_MAX_PAGES {
            break;
        }

        match find_next_page_link(&html, &current_url) {
            Some(next) => current_url = next,
            None => break,
        }
    }

    Ok(LinkScanResult { matches, links_seen, text_only_matches })
}

// Builds the same validation JSON shape the yt-dlp-backed path returns,
// for the fallback link-scan result.
fn listing_scan_result(
    query: &str,
    validated_count: u64,
    scan: &LinkScanResult,
) -> serde_json::Value {
    let match_count = scan.matches.len() as u64;
    let can_download = match_count > 0;

    let message = if !can_download {
        listing_scan_empty_message(query, scan.links_seen, scan.text_only_matches)
    } else {
        format!(
            "Found {} match(es) for \"{}\" on this page (scanned up to {} links). Up to {} will be downloaded.",
            match_count,
            query.trim(),
            KEYWORD_SCAN_LIMIT,
            validated_count
        )
    };

    serde_json::json!({
        "valid": true,
        "source_title": format!("Search results for \"{}\"", query.trim()),
        "source_type": "search results",
        "entry_count": serde_json::Value::Null,
        "match_count": match_count,
        "sample_titles": scan.matches.iter().take(3).map(|(title, _)| title.clone()).collect::<Vec<_>>(),
        "message": message,
        "can_download": can_download,
    })
}

// Distinguishes three different failure shapes so the message actually
// helps: a page with no readable links at all (likely JS-rendered content, a
// login wall, or a redirect curl didn't follow); a page with plenty of
// links, none of which matched the keyword; or a page where some links
// matched the keyword by text but turned out to be other listing/category
// pages rather than real downloadable content (see `probe_downloadable`).
fn listing_scan_empty_message(query: &str, links_seen: u64, text_only_matches: u64) -> String {
    if links_seen == 0 {
        return "This page had no readable links on it — it may require JavaScript, a login, \
or age verification that a plain page fetch can't get past, or the page may not have \
loaded the way a browser would render it."
            .to_string();
    }

    if text_only_matches > 0 {
        return format!(
            "Found {} link(s) matching \"{}\" by their text, but none of them were actually \
downloadable — they're likely links to other results/category pages rather than videos \
themselves. Try a more specific keyword, or paste a page that links directly to individual \
videos.",
            text_only_matches,
            query.trim()
        );
    }

    format!(
        "Scanned {} link(s) on this page but none matched \"{}\". Try a broader or \
different keyword, or double-check the page actually lists what you're looking for.",
        links_seen,
        query.trim()
    )
}

fn build_match_filter(query: &str) -> String {
    let escaped = escape_regex(query.trim());
    // The match-filter string literal is delimited by single quotes, so a
    // literal "'" in the query (e.g. "don't") must itself be escaped —
    // otherwise it terminates the quoted string early and breaks yt-dlp's
    // --match-filters parser. escape_regex() only escapes regex
    // metacharacters, not this delimiter, so it's handled separately here.
    let filter_safe = escaped.replace('\'', "\\'");
    format!("title ~= '{filter_safe}'")
}

const KEYWORD_SCAN_LIMIT: u64 = 200;

fn extract_source_info(json: &serde_json::Value) -> (String, String, Option<u64>) {
    let title = json["playlist_title"]
        .as_str()
        .or_else(|| json["title"].as_str())
        .or_else(|| json["channel"].as_str())
        .unwrap_or("Unknown source")
        .to_string();

    let source_type = if json["playlist_count"].as_u64().unwrap_or(0) > 1
        || json["_type"].as_str() == Some("playlist")
    {
        "playlist"
    } else {
        "video"
    };

    let count = json["playlist_count"].as_u64().or(Some(1));
    (title, source_type.to_string(), count)
}

// Result of having yt-dlp itself enumerate a URL as a channel/playlist and
// filter by keyword — as opposed to our own link-scan fallback below.
struct YtDlpKeywordMatch {
    source_title: String,
    source_type: String,
    entry_count: Option<u64>,
    match_count: u64,
    sample_titles: Vec<String>,
    // (title, webpage_url) pairs for every matched entry — this is what lets
    // start_keyword_download enqueue concrete per-video queue items instead
    // of running one big --match-filters yt-dlp process over the whole
    // playlist.
    entries: Vec<(String, String)>,
}

enum YtDlpKeywordOutcome {
    /// yt-dlp enumerated the URL and found at least one title match.
    Matched(YtDlpKeywordMatch),
    /// yt-dlp enumerated the URL fine, but no title matched the keyword.
    NoMatches(YtDlpKeywordMatch),
    /// yt-dlp has no extractor that can enumerate this URL as a
    /// channel/playlist at all (already translated into a user-facing
    /// message).
    CannotEnumerate(String),
}

// Runs yt-dlp's own flat-playlist probe + `--match-filters` simulate pass —
// the primary way this app enumerates a channel/playlist by keyword. Shared
// between validate_keyword_source_blocking and start_keyword_download so the
// two can't drift into disagreeing about whether yt-dlp can handle a given
// URL.
fn probe_ytdlp_keyword_source(
    source_url: &str,
    query: &str,
    validated_count: u64,
    pid_guard: &Arc<Mutex<Option<u32>>>,
    cancelled: &Arc<AtomicBool>,
) -> Result<YtDlpKeywordOutcome, String> {
    let mut probe_cmd = Command::new("yt-dlp");
    probe_cmd
        .args([
            "-j",
            "--flat-playlist",
            "--yes-playlist",
            "--playlist-end",
            "1",
            "--no-warnings",
        ])
        .args(RETRY_ARGS)
        .arg(source_url);
    let probe = run_killable_probe(probe_cmd, pid_guard, cancelled)?;

    if !probe.status.success() {
        let err = String::from_utf8_lossy(&probe.stderr).trim().to_string();
        return Ok(YtDlpKeywordOutcome::CannotEnumerate(translate_ytdlp_error(
            &err,
            "Could not access this URL. Use a video, playlist, or channel page supported by yt-dlp.",
        )));
    }

    let Ok(probe_json) = serde_json::from_slice::<serde_json::Value>(&probe.stdout) else {
        // Succeeded but printed something we can't parse as the expected
        // metadata — treat the same as "can't enumerate this," rather than
        // hard-erroring, so the link-scan fallback still gets a chance.
        return Ok(YtDlpKeywordOutcome::CannotEnumerate(
            "Could not parse this URL's metadata.".to_string(),
        ));
    };

    let (source_title, source_type, entry_count) = extract_source_info(&probe_json);

    let match_filter = build_match_filter(query);
    let scan_limit = KEYWORD_SCAN_LIMIT.to_string();
    let preview_limit = validated_count.min(KEYWORD_SCAN_LIMIT).to_string();

    // Printing "title<TAB>webpage_url" per matched entry (rather than just
    // the title) is what lets the caller enqueue concrete per-video queue
    // items below instead of re-running this whole match as one big
    // --match-filters download process.
    let mut sim_cmd = Command::new("yt-dlp");
    sim_cmd
        .args([
            "--flat-playlist",
            "--yes-playlist",
            "--simulate",
            "--no-warnings",
            "--print",
            "%(title)s\t%(webpage_url)s",
            "--match-filters",
            &match_filter,
            "--max-downloads",
            &preview_limit,
            "--playlist-end",
            &scan_limit,
        ])
        .args(RETRY_ARGS)
        .arg(source_url);
    let sim = run_killable_probe(sim_cmd, pid_guard, cancelled)?;

    if !sim.status.success() {
        let err = String::from_utf8_lossy(&sim.stderr).trim().to_string();
        return Ok(YtDlpKeywordOutcome::CannotEnumerate(translate_ytdlp_error(
            &err,
            "Keyword validation failed for this URL.",
        )));
    }

    let entries: Vec<(String, String)> = String::from_utf8_lossy(&sim.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_once('\t'))
        .map(|(title, url)| (title.to_string(), url.to_string()))
        .collect();
    let titles: Vec<String> = entries.iter().map(|(title, _)| title.clone()).collect();

    let m = YtDlpKeywordMatch {
        source_title,
        source_type,
        entry_count,
        match_count: titles.len() as u64,
        sample_titles: titles,
        entries,
    };

    if m.match_count > 0 {
        Ok(YtDlpKeywordOutcome::Matched(m))
    } else {
        Ok(YtDlpKeywordOutcome::NoMatches(m))
    }
}

fn ytdlp_match_result_json(m: &YtDlpKeywordMatch, query: &str, validated_count: u64) -> serde_json::Value {
    let can_download = m.match_count > 0;

    let message = if !can_download {
        format!(
            "No videos matching \"{}\" found at this URL. Use a channel or playlist page, or try a different keyword.",
            query.trim()
        )
    } else if m.source_type == "video" {
        format!(
            "Single video matches \"{}\" — ready to download.",
            query.trim()
        )
    } else {
        format!(
            "Found {} match(es) for \"{}\" (scanned up to {} entries). Up to {} will be downloaded.",
            m.match_count,
            query.trim(),
            KEYWORD_SCAN_LIMIT,
            validated_count
        )
    };

    serde_json::json!({
        "valid": true,
        "source_title": m.source_title,
        "source_type": m.source_type,
        "entry_count": m.entry_count,
        "match_count": m.match_count,
        "sample_titles": m.sample_titles.iter().take(3).collect::<Vec<_>>(),
        "message": message,
        "can_download": can_download,
    })
}

fn validate_keyword_source_blocking(
    app: AppHandle,
    source_url: String,
    query: String,
    result_count: u64,
    pid_guard: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
) -> Result<serde_json::Value, String> {
    validate_http_url(&source_url)?;
    if query.trim().is_empty() {
        return Err("Keyword cannot be empty.".to_string());
    }
    let validated_count = validate_keyword_result_count(result_count)?;
    let trimmed_url = source_url.trim();

    let outcome = probe_ytdlp_keyword_source(
        trimmed_url,
        &query,
        validated_count,
        &pid_guard,
        &cancelled,
    )?;

    if let YtDlpKeywordOutcome::Matched(m) = &outcome {
        return Ok(ytdlp_match_result_json(m, &query, validated_count));
    }

    // yt-dlp either couldn't enumerate this URL at all, or it could but no
    // title matched the keyword (which can also happen when yt-dlp's
    // generic extractor picks up some unrelated single item on the page,
    // e.g. a promotional embed, and reports a false "success" with nothing
    // real to match against). Either way, try scanning the page's own links
    // directly before giving up — this is what makes keyword search work on
    // sites yt-dlp has no listing/search extractor for at all.
    match scan_matching_links(&app, trimmed_url, &query, validated_count, KEYWORD_SCAN_LIMIT, &cancelled) {
        Ok(scan) if !scan.matches.is_empty() => {
            return Ok(listing_scan_result(&query, validated_count, &scan));
        }
        // The link scan ran but found nothing either — its message is more
        // diagnostic than yt-dlp's (it says whether the page had *any*
        // readable links at all, vs. just none matching the keyword), so
        // prefer it over yt-dlp's original error when we have it.
        Ok(scan) => return Ok(listing_scan_result(&query, validated_count, &scan)),
        Err(scan_err) if scan_err == "Check cancelled" => return Err(scan_err),
        Err(_) => {}
    }

    match outcome {
        YtDlpKeywordOutcome::NoMatches(m) => Ok(ytdlp_match_result_json(&m, &query, validated_count)),
        YtDlpKeywordOutcome::CannotEnumerate(err) => Err(err),
        YtDlpKeywordOutcome::Matched(_) => unreachable!("handled above"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// fetch_video_meta  — async so the UI never freezes.
//
// Key change: the stored "id" for each format is now a yt-dlp FORMAT SELECTOR
// string (e.g.  bestvideo[height=1080][ext=mp4]+bestaudio[ext=m4a]/...)
// instead of a raw numeric ID (e.g. 137).
//
// Raw IDs are site-specific and often refused at download time with
// "Requested format is not available".  Height-based selectors work
// universally across all yt-dlp supported sites.
// ─────────────────────────────────────────────────────────────────────────────
#[tauri::command]
async fn fetch_video_meta(url: String) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || fetch_meta_blocking(url))
        .await
        .map_err(|e| e.to_string())?
}

fn fetch_meta_blocking(url: String) -> Result<serde_json::Value, String> {
    let mut cmd = Command::new("yt-dlp");
    cmd.args(["-j", "--no-playlist", "--no-warnings"]);
    cmd.args(RETRY_ARGS);
    cmd.arg(&url);

    let output = cmd
        .output()
        .map_err(|e| format!("yt-dlp not found – is it installed? ({})", e))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Could not parse yt-dlp JSON: {e}"))?;

    let title = json["title"].as_str().unwrap_or("Unknown Video").to_string();

    // ── Collect unique heights that have an mp4 video stream ─────────────────
    let mut seen_heights: std::collections::BTreeSet<u64> = std::collections::BTreeSet::new();

    if let Some(fmts) = json["formats"].as_array() {
        for f in fmts {
            let ext    = f["ext"].as_str().unwrap_or("");
            let vcodec = f["vcodec"].as_str().unwrap_or("none");
            if ext == "mp4" && vcodec != "none" {
                if let Some(h) = f["height"].as_u64() {
                    if h > 0 { seen_heights.insert(h); }
                }
            }
        }
    }

    // ── Build one format entry per unique height, best-quality first ──────────
    let mut formats: Vec<serde_json::Value> = seen_heights
        .into_iter()
        .rev() // highest first
        .map(|height| {
            // Selector tries exact height first, then ≤ height as fallback.
            // This works on every site yt-dlp supports.
            let selector = format!(
                "bestvideo[height={height}][ext=mp4]+bestaudio[ext=m4a]\
                 /bestvideo[height={height}][ext=mp4]+bestaudio\
                 /bestvideo[height<={height}][ext=mp4]+bestaudio[ext=m4a]\
                 /bestvideo[height<={height}][ext=mp4]+bestaudio\
                 /best[height<={height}][ext=mp4]/best[ext=mp4]"
            );

            let label = format!("{height}p  MP4");

            serde_json::json!({ "id": selector, "label": label })
        })
        .collect();

    // Fallback when no mp4 streams were found
    if formats.is_empty() {
        formats.push(serde_json::json!({
            "id":    MP4_FORMAT_SELECTOR,
            "label": "Best MP4 (auto)"
        }));
    }

    Ok(serde_json::json!({ "title": title, "formats": formats }))
}

// ─── Keyword source validation (pre-flight check) ───────────────────────────
#[tauri::command]
async fn validate_keyword_source(
    app: AppHandle,
    state: tauri::State<'_, KeywordValidationState>,
    source_url: String,
    query: String,
    result_count: u64,
) -> Result<serde_json::Value, String> {
    // A previous check can still be running if the user hits "Check Now" (or
    // types fast enough to retrigger the debounce) before the last one
    // finished — kill it first so probes never pile up as concurrent yt-dlp
    // processes racing over the same running_pid slot.
    if let Some(pid) = pid_lock(&state.running_pid).take() {
        let _ = kill_process_group(pid);
    }
    state.cancelled.store(false, Ordering::SeqCst);

    let pid_guard = state.running_pid.clone();
    let cancelled = state.cancelled.clone();

    tauri::async_runtime::spawn_blocking(move || {
        validate_keyword_source_blocking(app, source_url, query, result_count, pid_guard, cancelled)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Cancel an in-flight keyword pre-flight check ────────────────────────────
#[tauri::command]
fn stop_keyword_validation(state: tauri::State<'_, KeywordValidationState>) -> Result<String, String> {
    let pid = pid_lock(&state.running_pid).take();
    match pid {
        Some(pid) => {
            state.cancelled.store(true, Ordering::SeqCst);
            match kill_process_group(pid) {
                Ok(_) => Ok("Check cancelled".to_string()),
                Err(e) => Err(format!("Failed to run kill: {e}")),
            }
        }
        None => Err("No active check to cancel".to_string()),
    }
}

// ─── Keyword search download ─────────────────────────────────────────────────
// Resolves keyword matches to concrete (title, url) pairs — either directly
// from yt-dlp's own flat-playlist match (now that probe_ytdlp_keyword_source
// prints webpage_url alongside title) or, when yt-dlp has no extractor that
// can enumerate the source URL at all, from the curl-based link-scan
// fallback — then enqueues them into the shared queue instead of running its
// own single/sequential download process. This is what lets keyword-search
// results download concurrently with everything else, using the same
// max_concurrency/per-host-cap machinery as single and batch mode.
#[tauri::command]
fn start_keyword_download(
    app: AppHandle,
    queue_state: tauri::State<'_, queue::QueueState>,
    source_url: String,
    query: String,
    target_dir: String,
    max_height: u64,
    result_count: u64,
) -> Result<String, String> {
    validate_http_url(&source_url)?;
    if query.trim().is_empty() {
        return Err("Search query cannot be empty.".to_string());
    }
    let validated_count = validate_keyword_result_count(result_count)?;

    let app_bg = app.clone();
    let inner = queue_state.inner.clone();

    std::thread::spawn(move || {
        let trimmed_url = source_url.trim().to_string();

        // This resolution phase (probe + optional link-scan) isn't wired to
        // a cancel command yet — the queue's per-item cancel only applies
        // once items actually exist, and this phase runs before any do.
        // Throwaway trackers keep probe_ytdlp_keyword_source /
        // scan_matching_links working unmodified; wiring a real cancel for
        // this phase is deferred to the Phase 2 UI work, alongside the rest
        // of the keyword-mode rebuild.
        let pid_guard: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let cancelled: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

        let outcome = match probe_ytdlp_keyword_source(
            &trimmed_url,
            &query,
            validated_count,
            &pid_guard,
            &cancelled,
        ) {
            Ok(outcome) => outcome,
            Err(e) => {
                let _ = app_bg.emit("download-error", e);
                return;
            }
        };

        let entries: Vec<(String, String)> = if let YtDlpKeywordOutcome::Matched(m) = outcome {
            m.entries
        } else {
            let scan = match scan_matching_links(
                &app_bg,
                &trimmed_url,
                &query,
                validated_count,
                KEYWORD_SCAN_LIMIT,
                &cancelled,
            ) {
                Ok(scan) => scan,
                Err(e) => {
                    let _ = app_bg.emit("download-error", e);
                    return;
                }
            };
            if scan.matches.is_empty() {
                let _ = app_bg.emit(
                    "download-error",
                    listing_scan_empty_message(&query, scan.links_seen, scan.text_only_matches),
                );
                return;
            }
            scan.matches
        };

        let specs: Vec<queue::NewItemSpec> = entries
            .into_iter()
            .take(validated_count as usize)
            .map(|(title, url)| queue::NewItemSpec {
                url,
                title: Some(title),
                source: queue::QueueSource::Keyword,
                format_id: None,
                resolution: Some(max_height),
                save_path: target_dir.clone(),
            })
            .collect();

        let ids = queue::push_items(&app_bg, &inner, specs);
        let _ = app_bg.emit("keyword-download-queued", ids.len());
    });

    Ok("Keyword search started — resolving matches…".to_string())
}

// Escape regex special characters to prevent injection and ensure literal matching.
// `str::replace` takes a `&str` replacement, not a closure, so a pattern-matching
// closure can't produce a per-character escaped replacement in one call — we walk
// the string manually instead.
fn escape_regex(text: &str) -> String {
    const SPECIAL: &str = ".*+?^$()[]{}|\\";
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if SPECIAL.contains(c) {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

// ─── Chrome extension bridge (turbodl:// deep link) ──────────────────────────
// The extension hands a URL to this app by opening `turbodl://add?url=...`
// (or `turbodl://add-batch?url=...&url=...` for several at once). On Linux,
// the OS launches a *new* process with that link as its only argv entry;
// tauri-plugin-single-instance (with its "deep-link" feature) intercepts that
// second launch and — like a genuine cold start — hands the link to
// tauri-plugin-deep-link, which emits it as a `deep-link://new-url` event.
// Cold start and warm (already-running) both end up funneling into this one
// function, via the two call sites in `run()` below.
//
// This is OS-level, attacker-reachable input: any application or malicious
// webpage can invoke a registered custom scheme, not just this app's own
// extension. Every value is re-validated here independently of how it
// arrived — nothing about "it came from the deep-link plugin" is trusted.
const DEEP_LINK_SCHEME: &str = "turbodl";
const DEEP_LINK_MAX_PAYLOAD_BYTES: usize = 4096;
const DEEP_LINK_MAX_BATCH: usize = 50;
// Deep-linked items don't have a frontend-chosen save path to draw from (the
// browser extension has no UI of its own for that) — land them in the same
// default the app itself starts with, same as a fresh single-mode download.
const DEEP_LINK_DEFAULT_SAVE_PATH: &str = "~/Downloads";

fn handle_incoming_deep_link(app: &AppHandle, raw: &str) {
    if raw.len() > DEEP_LINK_MAX_PAYLOAD_BYTES {
        reject_deep_link(app, "Ignored an incoming link: payload too large");
        return;
    }

    let Ok(parsed) = url::Url::parse(raw) else {
        reject_deep_link(app, "Ignored an incoming link: not a valid URL");
        return;
    };

    if parsed.scheme() != DEEP_LINK_SCHEME {
        reject_deep_link(app, "Ignored an incoming link: unrecognized scheme");
        return;
    }

    let queue_state = app.state::<queue::QueueState>();

    match parsed.host_str().unwrap_or("") {
        "add" => {
            let Some(target) = parsed
                .query_pairs()
                .find(|(k, _)| k == "url")
                .map(|(_, v)| v.into_owned())
            else {
                reject_deep_link(app, "Ignored an incoming link: missing url parameter");
                return;
            };
            match sanitize_extension_url(&target) {
                Ok(clean) => {
                    let ids = queue::push_items(
                        app,
                        &queue_state.inner,
                        vec![extension_item_spec(clean)],
                    );
                    let _ = app.emit("deep-link-added", ids.len());
                }
                Err(reason) => reject_deep_link(app, &reason),
            }
        }
        "add-batch" => {
            let urls: Vec<String> = parsed
                .query_pairs()
                .filter(|(k, _)| k == "url")
                .map(|(_, v)| v.into_owned())
                .collect();
            if urls.is_empty() {
                reject_deep_link(app, "Ignored an incoming link: no urls in batch");
                return;
            }
            if urls.len() > DEEP_LINK_MAX_BATCH {
                reject_deep_link(app, "Ignored an incoming link: too many urls in one batch");
                return;
            }

            let mut specs = Vec::new();
            let mut skipped = 0u32;
            for raw_url in urls {
                match sanitize_extension_url(&raw_url) {
                    Ok(clean) => specs.push(extension_item_spec(clean)),
                    Err(_) => skipped += 1,
                }
            }
            if specs.is_empty() {
                reject_deep_link(app, "Ignored an incoming link: no valid urls in batch");
                return;
            }
            let ids = queue::push_items(app, &queue_state.inner, specs);
            let _ = app.emit("deep-link-added", ids.len());
            if skipped > 0 {
                reject_deep_link(app, &format!("Skipped {skipped} invalid link(s) in the batch"));
            }
        }
        _ => reject_deep_link(app, "Ignored an incoming link: unrecognized action"),
    }
}

fn extension_item_spec(url: String) -> queue::NewItemSpec {
    queue::NewItemSpec {
        url,
        title: None,
        source: queue::QueueSource::Extension,
        format_id: None,
        resolution: Some(0),
        save_path: DEEP_LINK_DEFAULT_SAVE_PATH.to_string(),
    }
}

// Re-validates a URL pulled out of a deep link before it can ever reach
// build_ytdlp_command / Command::arg. validate_http_url's http(s)-prefix
// check already excludes anything starting with '-', but that check is kept
// explicit here rather than left as an implicit side effect, since yt-dlp
// treats a leading-dash argument as a flag (e.g. an --exec) and this is the
// exact class of input this validation exists to stop.
fn sanitize_extension_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('-') {
        return Err("Ignored an incoming link: value looked like a command flag".to_string());
    }
    validate_http_url(trimmed)?;
    Ok(trimmed.to_string())
}

fn reject_deep_link(app: &AppHandle, message: &str) {
    eprintln!("[deep-link] {message}");
    let _ = app.emit("deep-link-rejected", message);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Tauri application and registers the download commands.
pub fn run() {
    tauri::Builder::default()
        // Must be registered before the deep-link plugin below — its
        // "deep-link" Cargo feature makes it forward a second launch's argv
        // into tauri-plugin-deep-link automatically (the same
        // deep-link://new-url event a cold start produces), and its own
        // callback here additionally focuses the existing window so the user
        // actually sees the item land instead of nothing appearing to happen.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .manage(queue::QueueState::new())
        .manage(KeywordValidationState {
            running_pid: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
        .setup(|app| {
            // The dispatcher is one long-lived background thread that admits
            // Pending queue items up to the live max_concurrency setting —
            // see queue::start_dispatcher. Started here (not lazily on first
            // enqueue) so it's already polling by the time any command runs.
            let qstate = app.state::<queue::QueueState>().inner.clone();
            queue::start_dispatcher(app.handle().clone(), qstate);

            // Best-effort: registers this app as the OS handler for
            // turbodl:// links (writes/updates a .desktop file + xdg-mime
            // association on Linux). Idempotent — safe to call on every
            // launch — and harmless if it fails (e.g. xdg-mime missing),
            // since a dev build run via `tauri dev` still works for manual
            // testing regardless.
            let _ = app.deep_link().register_all();

            // Cold start: if the OS launched this process because of a
            // turbodl:// link, tauri-plugin-deep-link already parsed argv
            // during its own setup (before this closure runs) and recorded
            // it — reading get_current() here is more reliable than trying
            // to catch its one-shot emit, which can race a listener that
            // hasn't subscribed yet.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                for url in urls {
                    handle_incoming_deep_link(app.handle(), url.as_str());
                }
            }

            // Warm start: a later launch attempt (intercepted by
            // single-instance) or, on other platforms, a direct OS
            // notification — either way it arrives as this same event, so
            // cold and warm start share this one handler.
            let handle = app.handle().clone();
            app.listen("deep-link://new-url", move |event| {
                if let Ok(urls) = serde_json::from_str::<Vec<String>>(event.payload()) {
                    for url in urls {
                        handle_incoming_deep_link(&handle, &url);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_directory_dialog,
            open_file_dialog,
            fetch_video_meta,
            validate_keyword_source,
            stop_keyword_validation,
            start_keyword_download,
            update_yt_dlp,
            queue::enqueue_download,
            queue::enqueue_urls,
            queue::enqueue_batch_file,
            queue::cancel_queue_item,
            queue::cancel_all_queue_items,
            queue::pause_queue_item,
            queue::resume_queue_item,
            queue::set_max_concurrency,
            queue::get_queue_snapshot,
            queue::remove_completed_item,
            queue::clear_completed,
        ])
        // Without this, closing the window while a download is active
        // leaves yt-dlp (and any ffmpeg merge it handed off to) running as
        // an orphaned background process with no way to stop it short of
        // a terminal `pkill` — the app itself is already gone. Generalized
        // from killing one tracked PID to snapshotting and killing every
        // currently-running queue item's process group.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let qstate = window.state::<queue::QueueState>();
                qstate.request_shutdown();
                for pid in qstate.snapshot_running_pids() {
                    let _ = kill_process_group(pid);
                }

                let kw_state = window.state::<KeywordValidationState>();
                let kw_pid = pid_lock(&kw_state.running_pid).take();
                if let Some(kw_pid) = kw_pid {
                    kw_state.cancelled.store(true, Ordering::SeqCst);
                    let _ = kill_process_group(kw_pid);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    // A synthetic listing page — a directory of article links plus a
    // "next page" link — standing in for any site whose search/category
    // page yt-dlp itself has no extractor for.
    const LISTING_SAMPLE_HTML: &str = r#"<div class="results">
<a href="https://ads.example.com/click?id=1">Sponsored</a>
<a href="/item/101/blue-widget"><img alt="thumb"></a>
<a href="/item/101/blue-widget">Blue Widget &amp; Stand</a>
<a href="/item/102/red-widget">Red Widget - Refurbished</a>
</div>
<a href="/items/page/2" rel="next">Next</a>"#;

    const LISTING_BASE_URL: &str = "https://shop.example.com/items/widget";

    #[test]
    fn extract_visible_links_skips_offsite_and_empty_text_links() {
        let entries = extract_visible_links(LISTING_SAMPLE_HTML, LISTING_BASE_URL);

        // The sponsored off-site link and the empty-text thumbnail link are
        // both dropped, but the trailing "Next" pagination link is a same-
        // host link with visible text too, so it's harvested here just like
        // any other content link — it's scan_matching_links' keyword filter
        // upstream that excludes it in practice (it won't match a real
        // search term), not extract_visible_links' job to guess it's nav.
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            (
                "Blue Widget & Stand".to_string(),
                "https://shop.example.com/item/101/blue-widget".to_string(),
            )
        );
        assert_eq!(entries[1].0, "Red Widget - Refurbished");
        assert_eq!(entries[2].0, "Next");
    }

    #[test]
    fn find_next_page_link_follows_rel_next() {
        let next = find_next_page_link(LISTING_SAMPLE_HTML, LISTING_BASE_URL);
        assert_eq!(
            next,
            Some("https://shop.example.com/items/page/2".to_string())
        );
    }

    #[test]
    fn find_next_page_link_falls_back_to_next_text() {
        let html = r#"<a href="/p/2">Next page</a>"#;
        assert_eq!(
            find_next_page_link(html, LISTING_BASE_URL),
            Some("https://shop.example.com/p/2".to_string())
        );
    }

    #[test]
    fn find_next_page_link_returns_none_on_last_page() {
        assert_eq!(find_next_page_link("<a href=\"/x\">Foo</a>", LISTING_BASE_URL), None);
    }

    #[test]
    fn listing_scan_empty_message_distinguishes_no_links_from_no_match() {
        let no_links = listing_scan_empty_message("widget", 0, 0);
        assert!(no_links.contains("no readable links"));

        let no_match = listing_scan_empty_message("widget", 12, 0);
        assert!(no_match.contains("Scanned 12 link"));
        assert!(no_match.contains("widget"));
    }

    #[test]
    fn listing_scan_empty_message_flags_text_only_matches() {
        let text_only = listing_scan_empty_message("widget", 12, 3);
        assert!(text_only.contains("Found 3 link"));
        assert!(text_only.contains("none of them were actually"));
    }

    #[test]
    fn probe_downloadable_returns_false_when_already_cancelled() {
        // Should short-circuit on the cancelled flag without ever spawning
        // yt-dlp — verified indirectly here since there's no network access
        // in this test environment to exercise the real subprocess path.
        let cancelled = Arc::new(AtomicBool::new(true));
        assert!(!probe_downloadable("https://example.com/whatever", &cancelled));
    }

    #[test]
    fn unescape_html_entities_handles_named_and_numeric() {
        assert_eq!(unescape_html_entities("Tom &amp; Jerry"), "Tom & Jerry");
        assert_eq!(unescape_html_entities("&#39;quoted&#39;"), "'quoted'");
    }

    #[test]
    fn expand_tilde_bare() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn expand_tilde_with_subpath() {
        let home = std::env::var("HOME").unwrap();
        assert_eq!(expand_tilde("~/Downloads"), format!("{home}/Downloads"));
    }

    #[test]
    fn expand_tilde_leaves_absolute_paths_alone() {
        assert_eq!(expand_tilde("/mnt/videos"), "/mnt/videos");
    }

    #[test]
    fn build_output_template_joins_dir_and_pattern() {
        assert_eq!(
            build_output_template("/mnt/videos"),
            "/mnt/videos/%(title)s.%(ext)s"
        );
    }

    #[test]
    fn build_height_selector_zero_means_best_available() {
        assert_eq!(build_height_selector(0), MP4_FORMAT_SELECTOR);
    }

    #[test]
    fn build_height_selector_caps_at_given_height() {
        let selector = build_height_selector(1080);
        assert!(selector.contains("height<=1080"));
        assert!(!selector.contains("height<=0"));
    }

    #[test]
    fn validate_keyword_result_count_rejects_zero() {
        assert!(validate_keyword_result_count(0).is_err());
    }

    #[test]
    fn validate_keyword_result_count_accepts_positive() {
        assert_eq!(validate_keyword_result_count(5), Ok(5));
    }

    #[test]
    fn validate_http_url_accepts_http_and_https() {
        assert!(validate_http_url("https://youtube.com/watch?v=x").is_ok());
        assert!(validate_http_url("http://example.com").is_ok());
    }

    #[test]
    fn validate_http_url_rejects_other_schemes() {
        assert!(validate_http_url("ftp://example.com").is_err());
        assert!(validate_http_url("not a url").is_err());
    }

    #[test]
    fn validate_http_url_rejects_too_short() {
        // "http://" alone (7 chars) has no host after the scheme.
        assert!(validate_http_url("http://").is_err());
    }

    #[test]
    fn translate_ytdlp_error_explains_unsupported_url() {
        let msg = translate_ytdlp_error(
            "ERROR: Unsupported URL: https://example.com/",
            "fallback",
        );
        assert!(msg.contains("supported site"));
        assert!(!msg.contains("example.com"));
    }

    #[test]
    fn translate_ytdlp_error_uses_last_error_line_after_retries() {
        let stderr = "WARNING: retry 1 of 15\nERROR: [generic] some.site: first probe failed\nWARNING: retry 2 of 15\nERROR: Unsupported URL: https://some.site/";
        let msg = translate_ytdlp_error(stderr, "fallback");
        assert!(msg.contains("supported site"));
    }

    #[test]
    fn translate_ytdlp_error_passes_through_other_errors() {
        let msg = translate_ytdlp_error(
            "ERROR: [vimeo] 123: This video is private",
            "fallback",
        );
        assert_eq!(msg, "[vimeo] 123: This video is private");
    }

    #[test]
    fn translate_ytdlp_error_uses_fallback_when_empty() {
        assert_eq!(translate_ytdlp_error("   ", "fallback message"), "fallback message");
    }

    #[test]
    fn escape_regex_escapes_metacharacters() {
        assert_eq!(escape_regex("a.b*c"), "a\\.b\\*c");
        assert_eq!(escape_regex("(x)[y]"), "\\(x\\)\\[y\\]");
    }

    #[test]
    fn escape_regex_leaves_plain_text_alone() {
        assert_eq!(escape_regex("hello world"), "hello world");
    }

    #[test]
    fn build_match_filter_escapes_apostrophes() {
        // Regression test for the bug where a query like "don't" broke
        // yt-dlp's --match-filters quoted-string parsing because the
        // literal "'" wasn't escaped before being embedded in the
        // single-quoted filter string.
        assert_eq!(build_match_filter("don't"), "title ~= 'don\\'t'");
    }

    #[test]
    fn build_match_filter_wraps_plain_query() {
        assert_eq!(build_match_filter("live performance"), "title ~= 'live performance'");
    }

    #[test]
    fn extract_source_info_detects_playlist() {
        let json = serde_json::json!({
            "_type": "playlist",
            "playlist_title": "My Playlist",
            "playlist_count": 12,
        });
        let (title, source_type, count) = extract_source_info(&json);
        assert_eq!(title, "My Playlist");
        assert_eq!(source_type, "playlist");
        assert_eq!(count, Some(12));
    }

    #[test]
    fn extract_source_info_detects_single_video() {
        let json = serde_json::json!({ "title": "A Video" });
        let (title, source_type, count) = extract_source_info(&json);
        assert_eq!(title, "A Video");
        assert_eq!(source_type, "video");
        assert_eq!(count, Some(1));
    }

    #[test]
    fn extract_source_info_falls_back_to_channel_name() {
        let json = serde_json::json!({ "channel": "Some Channel" });
        let (title, _, _) = extract_source_info(&json);
        assert_eq!(title, "Some Channel");
    }

    #[test]
    fn extract_source_info_falls_back_to_unknown() {
        let json = serde_json::json!({});
        let (title, _, _) = extract_source_info(&json);
        assert_eq!(title, "Unknown source");
    }

    #[test]
    fn sanitize_extension_url_accepts_plain_http_and_https() {
        assert_eq!(
            sanitize_extension_url("https://example.com/video").unwrap(),
            "https://example.com/video"
        );
        assert_eq!(
            sanitize_extension_url("  http://example.com/video  ").unwrap(),
            "http://example.com/video"
        );
    }

    #[test]
    fn sanitize_extension_url_rejects_leading_dash() {
        // The realistic attack: a "url" value crafted to look like a yt-dlp
        // flag (e.g. --exec=...) rather than a real link.
        assert!(sanitize_extension_url("--exec=touch /tmp/pwned").is_err());
        assert!(sanitize_extension_url("-x").is_err());
    }

    #[test]
    fn sanitize_extension_url_rejects_non_http_schemes() {
        assert!(sanitize_extension_url("file:///etc/passwd").is_err());
        assert!(sanitize_extension_url("javascript:alert(1)").is_err());
        assert!(sanitize_extension_url("not a url at all").is_err());
    }
}
