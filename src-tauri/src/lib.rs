use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::path::Path;
use tauri::{AppHandle, Emitter};
use rfd::FileDialog;

const RETRY_ARGS: &[&str] = &[
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
fn expand_tilde(path: &str) -> String {
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

fn build_output_template(target_dir: &str) -> String {
    let expanded = expand_tilde(target_dir);
    Path::new(&expanded)
        .join("%(title)s.%(ext)s")
        .to_string_lossy()
        .to_string()
}

fn build_height_selector(max_height: u64) -> String {
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

fn validate_http_url(url: &str) -> Result<(), String> {
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

fn build_match_filter(query: &str) -> String {
    let escaped = escape_regex(query.trim());
    format!("title ~= '{escaped}'")
}

const KEYWORD_SCAN_LIMIT: u64 = 200;

fn extract_source_info(json: &serde_json::Value) -> (String, String, Option<u64>) {
    if let Some(entries) = json.as_array() {
        let first = entries.first().cloned().unwrap_or_else(|| json.clone());
        let title = first["playlist_title"]
            .as_str()
            .or_else(|| first["title"].as_str())
            .or_else(|| first["channel"].as_str())
            .unwrap_or("Unknown source")
            .to_string();
        let count = first["playlist_count"]
            .as_u64()
            .or_else(|| Some(entries.len() as u64));
        return (title, "playlist".to_string(), count);
    }

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

fn validate_keyword_source_blocking(
    source_url: String,
    query: String,
    result_count: u64,
) -> Result<serde_json::Value, String> {
    validate_http_url(&source_url)?;
    if query.trim().is_empty() {
        return Err("Keyword cannot be empty.".to_string());
    }
    let validated_count = validate_keyword_result_count(result_count)?;

    // Probe: can yt-dlp read this URL at all?
    let probe = Command::new("yt-dlp")
        .args([
            "-j",
            "--flat-playlist",
            "--yes-playlist",
            "--playlist-end",
            "1",
            "--no-warnings",
        ])
        .args(RETRY_ARGS)
        .arg(source_url.trim())
        .output()
        .map_err(|e| format!("yt-dlp not found – is it installed? ({e})"))?;

    if !probe.status.success() {
        let err = String::from_utf8_lossy(&probe.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "Could not access this URL. Use a video, playlist, or channel page supported by yt-dlp.".to_string()
        } else {
            err
        });
    }

    let probe_json: serde_json::Value = serde_json::from_slice(&probe.stdout)
        .map_err(|e| format!("Could not parse source metadata: {e}"))?;

    let (source_title, source_type, entry_count) = extract_source_info(&probe_json);

    // Simulate matching titles without downloading.
    let match_filter = build_match_filter(&query);
    let scan_limit = KEYWORD_SCAN_LIMIT.to_string();
    let preview_limit = validated_count.min(KEYWORD_SCAN_LIMIT).to_string();

    let sim = Command::new("yt-dlp")
        .args([
            "--flat-playlist",
            "--yes-playlist",
            "--simulate",
            "--no-warnings",
            "--print",
            "%(title)s",
            "--match-filters",
            &match_filter,
            "--max-downloads",
            &preview_limit,
            "--playlist-end",
            &scan_limit,
        ])
        .args(RETRY_ARGS)
        .arg(source_url.trim())
        .output()
        .map_err(|e| format!("Could not run keyword validation: {e}"))?;

    if !sim.status.success() {
        let err = String::from_utf8_lossy(&sim.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "Keyword validation failed for this URL.".to_string()
        } else {
            err
        });
    }

    let titles: Vec<String> = String::from_utf8_lossy(&sim.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    let match_count = titles.len() as u64;
    let can_download = match_count > 0;

    let message = if !can_download {
        format!(
            "No videos matching \"{}\" found at this URL. Use a channel or playlist page, or try a different keyword.",
            query.trim()
        )
    } else if source_type == "video" {
        format!(
            "Single video matches \"{}\" — ready to download.",
            query.trim()
        )
    } else {
        format!(
            "Found {} match(es) for \"{}\" (scanned up to {} entries). Up to {} will be downloaded.",
            match_count,
            query.trim(),
            KEYWORD_SCAN_LIMIT,
            validated_count
        )
    };

    Ok(serde_json::json!({
        "valid": true,
        "source_title": source_title,
        "source_type": source_type,
        "entry_count": entry_count,
        "match_count": match_count,
        "sample_titles": titles.iter().take(3).collect::<Vec<_>>(),
        "message": message,
        "can_download": can_download,
    }))
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

// ─── Stream yt-dlp stdout back to the frontend as events ─────────────────────
fn spawn_and_stream(app: AppHandle, mut child: std::process::Child) -> Result<(), String> {
    let stdout = child.stdout.take().ok_or("stdout pipe missing")?;
    let stderr = child.stderr.take().ok_or("stderr pipe missing")?;

    // Drain stderr on its own thread, concurrently with stdout. yt-dlp can
    // emit a lot of warnings on stderr; if nobody reads that pipe until
    // child.wait() returns, the OS pipe buffer fills up, yt-dlp blocks on
    // write(), and child.wait() then never returns — a silent hang.
    let stderr_handle = std::thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .flatten()
            .collect::<Vec<String>>()
    });

    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            let _ = app.emit("download-progress", &line);
        }
        match child.wait() {
            Ok(s) if s.success() => {
                let _ = app.emit("download-complete", "success");
            }
            Ok(_) => {
                let err_lines = stderr_handle.join().unwrap_or_default();
                let _ = app.emit("download-error", err_lines.join("\n").trim().to_string());
            }
            Err(e) => {
                let _ = app.emit("download-error", e.to_string());
            }
        }
    });
    Ok(())
}

// ─── Single-URL download ──────────────────────────────────────────────────────
#[tauri::command]
fn start_turbo_download(
    app: AppHandle,
    url: String,
    format_id: String,   // actually a selector string now, not a numeric ID
    target_dir: String,
) -> Result<String, String> {
    let out_template = build_output_template(&target_dir);

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-f",  &format_id,          // height-based selector, works on any site
        "-o",  &out_template,
        "--merge-output-format", "mp4",
        // --merge-output-format only applies when yt-dlp merges two separate
        // streams (video+audio). Sites that serve a single HLS stream (no
        // merge happens) would otherwise be left in their native .ts
        // container. --remux-video does a lossless (stream-copy) container
        // remux to mp4 in that case too.
        "--remux-video", "mp4",
        "--concurrent-fragments","5",
        "--http-chunk-size",     "10485760",
        "--newline",
        "--no-playlist",
    ]);
    cmd.args(RETRY_ARGS);
    cmd.arg(&url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("Failed to launch yt-dlp: {e}"))?;
    spawn_and_stream(app, child)?;
    Ok("Download started".to_string())
}

// ─── Keyword source validation (pre-flight check) ───────────────────────────
#[tauri::command]
async fn validate_keyword_source(
    source_url: String,
    query: String,
    result_count: u64,
) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        validate_keyword_source_blocking(source_url, query, result_count)
    })
    .await
    .map_err(|e| e.to_string())?
}

// ─── Keyword search download ─────────────────────────────────────────────────
#[tauri::command]
fn start_keyword_download(
    app: AppHandle,
    source_url: String,
    query: String,
    target_dir: String,
    max_height: u64,
    result_count: u64,
) -> Result<String, String> {
    if source_url.trim().is_empty() {
        return Err("Source URL is required for keyword search.".to_string());
    }

    if query.trim().is_empty() {
        return Err("Search query cannot be empty.".to_string());
    }

    let validated_count = validate_keyword_result_count(result_count)?;

    // Reject downloads when the pre-flight check finds zero matches.
    let validation =
        validate_keyword_source_blocking(source_url.clone(), query.clone(), validated_count)?;
    let can_download = validation["can_download"].as_bool().unwrap_or(false);
    if !can_download {
        return Err(validation["message"]
            .as_str()
            .unwrap_or("No matching videos found.")
            .to_string());
    }

    let match_count = validation["match_count"].as_u64().unwrap_or(validated_count);
    let download_total = validated_count.min(match_count);
    let out_template = build_output_template(&target_dir);
    let format_selector = build_height_selector(max_height);
    let match_filter = build_match_filter(&query);

    let _ = app.emit("batch-total", download_total);

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-f", &format_selector,
        "-o", &out_template,
        "--merge-output-format", "mp4",
        "--remux-video", "mp4",
        "--concurrent-fragments", "5",
        "--http-chunk-size", "10485760",
        "--newline",
        // Required for channel/playlist URLs — without this yt-dlp only
        // processes the first video and keyword filtering silently fails.
        "--yes-playlist",
        "--sleep-interval", "4",
        "--max-sleep-interval", "8",
        "--sleep-requests", "1",
        "--ignore-errors",
        "--match-filters", &match_filter,
        "--max-downloads", &validated_count.to_string(),
    ]);
    cmd.args(RETRY_ARGS);
    cmd.arg(source_url.trim());
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("Failed to launch yt-dlp: {e}"))?;
    spawn_and_stream(app, child)?;
    Ok(format!(
        "Keyword download started — up to {} match(es) for '{}'",
        download_total, query.trim()
    ))
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

// ─── Stream a single child's output and block until it exits ────────────────
// Used by the batch loop, which needs to know exactly when one file's yt-dlp
// process finished before moving on to emit the "next file starting" event.
// Same stderr-deadlock protection as spawn_and_stream: drain stderr on its
// own thread concurrently with stdout.
fn stream_and_wait(app: &AppHandle, mut child: std::process::Child) -> Result<(), String> {
    let stdout = child.stdout.take().ok_or("stdout pipe missing")?;
    let stderr = child.stderr.take().ok_or("stderr pipe missing")?;

    let stderr_handle = std::thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .flatten()
            .collect::<Vec<String>>()
    });

    for line in BufReader::new(stdout).lines().flatten() {
        let _ = app.emit("download-progress", &line);
    }

    match child.wait() {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => {
            let err_lines = stderr_handle.join().unwrap_or_default();
            Err(err_lines.join("\n").trim().to_string())
        }
        Err(e) => Err(e.to_string()),
    }
}

// ─── Batch download ───────────────────────────────────────────────────────────
// `max_height`  →  0 means "best available", any other value caps the height.
//
// Previously this ran ONE yt-dlp process with --batch-file for the whole
// list. That process never prints "Downloading item X of Y" for a plain URL
// list (that message only appears for real playlists/channels, where yt-dlp
// knows the total in advance) — so the frontend's X/Y file counter had no
// signal to update on and stayed stuck at 0/N for the entire run.
//
// Now we launch yt-dlp once per URL, sequentially, and emit "batch-item-start"
// ourselves right before each one — a signal the frontend can always rely on,
// regardless of what any given site's extractor happens to print.
#[tauri::command]
fn start_batch_download(
    app: AppHandle,
    file_path: String,
    target_dir: String,
    max_height: u64,
) -> Result<String, String> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Cannot read batch file: {e}"))?;

    let urls: Vec<String> = content
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|t| {
            !t.is_empty()
                && !t.starts_with('#')
                && (t.starts_with("http://") || t.starts_with("https://"))
        })
        .collect();

    if urls.is_empty() {
        return Err("The file contains no valid URLs (one per line).".to_string());
    }

    let total = urls.len();
    let _ = app.emit("batch-total", total);

    let out_template = build_output_template(&target_dir);
    let format_selector = build_height_selector(max_height);

    std::thread::spawn(move || {
        let mut failures: Vec<String> = Vec::new();

        for (index, url) in urls.iter().enumerate() {
            let _ = app.emit("batch-item-start", (index + 1) as u64);

            let mut cmd = Command::new("yt-dlp");
            cmd.args([
                "-f", &format_selector,
                "-o", &out_template,
                "--merge-output-format", "mp4",
                "--remux-video", "mp4",
                "--concurrent-fragments", "5",
                "--http-chunk-size", "10485760",
                "--newline",
                "--no-playlist",
                // Small delay between individual fragment requests, to avoid
                // hammering the CDN within a single file's download.
                "--sleep-requests", "1",
            ]);
            cmd.args(RETRY_ARGS);
            cmd.arg(url);
            cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

            match cmd.spawn() {
                Ok(child) => {
                    if let Err(e) = stream_and_wait(&app, child) {
                        failures.push(format!("{url} — {e}"));
                    }
                }
                Err(e) => failures.push(format!("{url} — failed to launch yt-dlp: {e}")),
            }

            // Wait 4–8s between files (varies a little instead of a fixed
            // cadence) — same rate-limiting protection the old
            // --sleep-interval/--max-sleep-interval flags gave, just applied
            // by us now that each file is its own process.
            if index + 1 < total {
                let delay_secs = 4 + ((index as u64) % 5);
                std::thread::sleep(std::time::Duration::from_secs(delay_secs));
            }
        }

        if failures.is_empty() {
            let _ = app.emit("download-complete", "success");
        } else {
            let _ = app.emit("download-error", failures.join("\n"));
        }
    });

    Ok(format!("Batch started — {total} URLs queued"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Tauri application and registers the download commands.
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_directory_dialog,
            open_file_dialog,
            fetch_video_meta,
            validate_keyword_source,
            start_turbo_download,
            start_keyword_download,
            start_batch_download,
            update_yt_dlp,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
