use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
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
fn spawn_and_stream(app: AppHandle, mut child: std::process::Child) {
    let stdout = child.stdout.take().expect("stdout pipe missing");
    let stderr = child.stderr.take().expect("stderr pipe missing");

    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().flatten() {
            let _ = app.emit("download-progress", &line);
        }
        match child.wait() {
            Ok(s) if s.success() => { let _ = app.emit("download-complete", "success"); }
            Ok(_) => {
                let err: String = BufReader::new(stderr)
                    .lines().flatten()
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = app.emit("download-error", err.trim().to_string());
            }
            Err(e) => { let _ = app.emit("download-error", e.to_string()); }
        }
    });
}

// ─── Single-URL download ──────────────────────────────────────────────────────
#[tauri::command]
fn start_turbo_download(
    app: AppHandle,
    url: String,
    format_id: String,   // actually a selector string now, not a numeric ID
    target_dir: String,
) -> Result<String, String> {
    let out_template = format!("{}/%(title)s.%(ext)s", target_dir);

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-f",  &format_id,          // height-based selector, works on any site
        "-o",  &out_template,
        "--merge-output-format", "mp4",
        "--concurrent-fragments","5",
        "--http-chunk-size",     "10485760",
        "--newline",
        "--no-playlist",
    ]);
    cmd.args(RETRY_ARGS);
    cmd.arg(&url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("Failed to launch yt-dlp: {e}"))?;
    spawn_and_stream(app, child);
    Ok("Download started".to_string())
}

// ─── Batch download ───────────────────────────────────────────────────────────
// `max_height`  →  0 means "best available", any other value caps the height.
#[tauri::command]
fn start_batch_download(
    app: AppHandle,
    file_path: String,
    target_dir: String,
    max_height: u64,
) -> Result<String, String> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Cannot read batch file: {e}"))?;

    let url_count = content.lines().filter(|l| {
        let t = l.trim();
        !t.is_empty() && !t.starts_with('#')
            && (t.starts_with("http://") || t.starts_with("https://"))
    }).count();

    if url_count == 0 {
        return Err("The file contains no valid URLs (one per line).".to_string());
    }

    let _ = app.emit("batch-total", url_count);
    let out_template = format!("{}/%(title)s.%(ext)s", target_dir);

    // Build the format selector based on the requested max height.
    // 0 → no cap, pick the absolute best MP4 available.
    let format_selector: String = if max_height == 0 {
        MP4_FORMAT_SELECTOR.to_string()
    } else {
        format!(
            "bestvideo[height<={max_height}][ext=mp4]+bestaudio[ext=m4a]             /bestvideo[height<={max_height}][ext=mp4]+bestaudio             /best[height<={max_height}][ext=mp4]             /best[ext=mp4]"
        )
    };

    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "--batch-file",          &file_path,
        "-f",                    &format_selector,
        "-o",                    &out_template,
        "--merge-output-format", "mp4",
        "--concurrent-fragments","5",
        "--http-chunk-size",     "10485760",
        "--newline",
        "--no-playlist",
        "--no-abort-on-error",
        // Wait 4–8 seconds between each video in the batch.
        // This prevents the site from rate-limiting / dropping the SSL
        // handshake on rapid back-to-back connections.
        "--sleep-interval",      "4",
        "--max-sleep-interval",  "8",
        // Also add a small delay between individual fragment requests
        // to avoid hammering the CDN on each file.
        "--sleep-requests",      "1",

    ]);
    cmd.args(RETRY_ARGS);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd.spawn().map_err(|e| format!("Failed to launch yt-dlp: {e}"))?;
    spawn_and_stream(app, child);
    Ok(format!("Batch started — {url_count} URLs queued"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            open_directory_dialog,
            open_file_dialog,
            fetch_video_meta,
            start_turbo_download,
            start_batch_download,
        ])
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}
