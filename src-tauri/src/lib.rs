use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter, Manager};
use rfd::FileDialog;

// Shared state so the frontend can cancel an active download by PID.
//
// - `running_pid` holds the PID of the currently-alive yt-dlp child, and
//   only that — None whenever no child process actually exists right now
//   (including the gaps between files in a batch run).
// - `busy` covers the whole lifetime of a start_*_download call, gaps
//   included: true from the moment a download is accepted until it (or
//   the whole batch queue) fully finishes. This is what start_*_download
//   checks to reject an overlapping call — using running_pid for that
//   would let a second download start during a batch's inter-file sleep,
//   when running_pid is legitimately None but the batch is still active.
// - `cancelled` distinguishes a deliberate user cancellation from a real
//   failure: reset to false at the start of every start_*_download call
//   and set to true by stop_download, so background threads can tell
//   "this process exited because the user killed it" apart from "this
//   process exited because yt-dlp failed" and stay silent in the former
//   case instead of emitting a misleading download-error.
struct DownloadState {
    running_pid: Arc<Mutex<Option<u32>>>,
    busy: Arc<AtomicBool>,
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
fn kill_process_group(pid: u32) -> std::io::Result<std::process::Output> {
    Command::new("kill")
        .args(["-TERM", &format!("-{pid}")])
        .output()
}

// Atomically claims the "a download is active" flag — otherwise two
// start_*_download calls landing back-to-back (or one landing during a
// batch's inter-file gap) could both proceed and both launch a process,
// leaving one of them untracked and uncancelable.
fn reserve_download_slot(state: &DownloadState) -> Result<(), String> {
    if state
        .busy
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(
            "A download is already in progress. Cancel it before starting a new one."
                .to_string(),
        );
    }
    Ok(())
}

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
fn spawn_and_stream(
    app: AppHandle,
    mut child: std::process::Child,
    pid_guard: Arc<Mutex<Option<u32>>>,
    cancelled: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
) -> Result<(), String> {
    let stdout = child.stdout.take().ok_or("stdout pipe missing")?;
    let stderr = child.stderr.take().ok_or("stderr pipe missing")?;

    // Drain stderr on its own thread, concurrently with stdout. yt-dlp can
    // emit a lot of warnings on stderr; if nobody reads that pipe until
    // child.wait() returns, the OS pipe buffer fills up, yt-dlp blocks on
    // write(), and child.wait() then never returns — a silent hang.
    let stderr_handle = std::thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<String>>()
    });

    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let _ = app.emit("download-progress", &line);
        }
        match child.wait() {
            Ok(s) if s.success() => {
                let _ = app.emit("download-complete", "success");
            }
            Ok(_) => {
                // A non-zero exit after the user hit Cancel is expected
                // (that's what killing the process looks like) — don't
                // report it as an error, the frontend already knows.
                if !cancelled.load(Ordering::SeqCst) {
                    let err_lines = stderr_handle.join().unwrap_or_default();
                    let _ = app.emit("download-error", err_lines.join("\n").trim().to_string());
                }
            }
            Err(e) => {
                if !cancelled.load(Ordering::SeqCst) {
                    let _ = app.emit("download-error", e.to_string());
                }
            }
        }
        // Clear the PID and release the busy flag — the process has
        // exited (or was killed), so a new download can now be started.
        *pid_lock(&pid_guard) = None;
        busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}

// ─── Single-URL download ──────────────────────────────────────────────────────
#[tauri::command]
fn start_turbo_download(
    app: AppHandle,
    state: tauri::State<'_, DownloadState>,
    url: String,
    format_id: String,   // actually a selector string now, not a numeric ID
    target_dir: String,
) -> Result<String, String> {
    reserve_download_slot(&state)?;
    state.cancelled.store(false, Ordering::SeqCst);
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
    // Own process group so cancelling kills any ffmpeg merge child too,
    // not just yt-dlp itself (see kill_process_group).
    cmd.process_group(0);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            state.busy.store(false, Ordering::SeqCst); // nothing will run in the background to release this
            return Err(format!("Failed to launch yt-dlp: {e}"));
        }
    };
    let pid = child.id();
    *pid_lock(&state.running_pid) = Some(pid);
    let pid_guard = state.running_pid.clone();
    spawn_and_stream(app, child, pid_guard, state.cancelled.clone(), state.busy.clone())?;
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
    state: tauri::State<'_, DownloadState>,
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

    reserve_download_slot(&state)?;
    state.cancelled.store(false, Ordering::SeqCst);

    // The frontend already ran the full pre-flight validation; skip the
    // expensive yt-dlp probes here and just validate our inputs.
    let download_total = validated_count;
    let out_template = build_output_template(&target_dir);
    let format_selector = build_height_selector(max_height);
    let match_filter = build_match_filter(&query);
    // Same scan depth the pre-flight validation used (KEYWORD_SCAN_LIMIT)
    // — without this bound the real download could crawl arbitrarily deep
    // into a large channel/playlist looking for matches, taking far longer
    // than what the validation step told the user to expect.
    let scan_limit = KEYWORD_SCAN_LIMIT.to_string();

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
        "--playlist-end", &scan_limit,
    ]);
    cmd.args(RETRY_ARGS);
    cmd.arg(source_url.trim());
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.process_group(0);

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            state.busy.store(false, Ordering::SeqCst);
            return Err(format!("Failed to launch yt-dlp: {e}"));
        }
    };
    let pid = child.id();
    *pid_lock(&state.running_pid) = Some(pid);
    let pid_guard = state.running_pid.clone();
    spawn_and_stream(app, child, pid_guard, state.cancelled.clone(), state.busy.clone())?;
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
fn stream_and_wait(
    app: &AppHandle,
    mut child: std::process::Child,
    pid_guard: Arc<Mutex<Option<u32>>>,
) -> Result<(), String> {
    let stdout = child.stdout.take().ok_or("stdout pipe missing")?;
    let stderr = child.stderr.take().ok_or("stderr pipe missing")?;

    let stderr_handle = std::thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<String>>()
    });

    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let _ = app.emit("download-progress", &line);
    }

    let result = match child.wait() {
        Ok(s) if s.success() => Ok(()),
        Ok(_) => {
            let err_lines = stderr_handle.join().unwrap_or_default();
            Err(err_lines.join("\n").trim().to_string())
        }
        Err(e) => Err(e.to_string()),
    };

    // Clear the PID now that the process has exited.
    *pid_lock(&pid_guard) = None;

    result
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
    state: tauri::State<'_, DownloadState>,
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

    reserve_download_slot(&state)?;
    state.cancelled.store(false, Ordering::SeqCst);

    let total = urls.len();
    let _ = app.emit("batch-total", total);

    let out_template = build_output_template(&target_dir);
    let format_selector = build_height_selector(max_height);

    // Clone shared state before spawning the thread (State refs can't move
    // into a 'static thread).
    let pid_guard = state.running_pid.clone();
    let cancelled = state.cancelled.clone();
    let busy = state.busy.clone();

    std::thread::spawn(move || {
        let mut failures: Vec<String> = Vec::new();
        let mut succeeded: u64 = 0;

        for (index, url) in urls.iter().enumerate() {
            // Cancel was hit while the previous item was between downloads
            // (e.g. during the inter-file sleep) — stop the whole queue
            // instead of starting another file.
            if cancelled.load(Ordering::SeqCst) {
                break;
            }

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
            // Own process group so cancelling kills any ffmpeg merge child
            // too, not just yt-dlp itself (see kill_process_group).
            cmd.process_group(0);

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id();
                    *pid_lock(&pid_guard) = Some(pid);
                    let child_pid_guard = pid_guard.clone();
                    match stream_and_wait(&app, child, child_pid_guard) {
                        Ok(()) => succeeded += 1,
                        Err(e) => {
                            // A cancelled item's process exits non-zero
                            // because it was killed, not because it failed
                            // — don't record it as a failure, and stop the
                            // queue here rather than moving on to the next
                            // URL.
                            if cancelled.load(Ordering::SeqCst) {
                                break;
                            }
                            failures.push(format!("{url} — {e}"));
                        }
                    }
                }
                Err(e) => failures.push(format!("{url} — failed to launch yt-dlp: {e}")),
            }

            if cancelled.load(Ordering::SeqCst) {
                break;
            }

            // Wait 4–8s between files (varies a little instead of a fixed
            // cadence) — same rate-limiting protection the old
            // --sleep-interval/--max-sleep-interval flags gave, just applied
            // by us now that each file is its own process. Slept in short
            // increments so a cancel during this gap takes effect almost
            // immediately instead of waiting out the full delay.
            if index + 1 < total {
                let delay_secs = 4 + ((index as u64) % 5);
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_secs(delay_secs);
                while std::time::Instant::now() < deadline {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            }
        }

        // The whole batch is done now — release the slot so a new
        // download can be started.
        busy.store(false, Ordering::SeqCst);

        // A deliberate cancel already has its own frontend state (set the
        // moment stop_download returns) — don't overwrite it with a
        // trailing complete/error/partial event for a queue the user chose
        // to stop.
        if cancelled.load(Ordering::SeqCst) {
            // no-op
        } else if failures.is_empty() {
            let _ = app.emit("download-complete", "success");
        } else if succeeded > 0 {
            // Some files failed but at least one succeeded — a plain
            // download-error would read as "the whole batch failed",
            // which isn't true, so this gets its own event with counts.
            let _ = app.emit(
                "download-partial",
                serde_json::json!({
                    "succeeded": succeeded,
                    "failed": failures.len(),
                    "errors": failures,
                }),
            );
        } else {
            let _ = app.emit("download-error", failures.join("\n"));
        }
    });

    Ok(format!("Batch started — {total} URLs queued"))
}

// ─── Cancel an active download by killing the tracked yt-dlp process ────────
#[tauri::command]
fn stop_download(state: tauri::State<'_, DownloadState>) -> Result<String, String> {
    let pid = pid_lock(&state.running_pid).take();

    // Nothing tracked and no download in flight at all — genuinely
    // nothing to cancel.
    if pid.is_none() && !state.busy.load(Ordering::SeqCst) {
        return Err("No active download to cancel".to_string());
    }

    // Mark this as a deliberate cancellation before killing anything, so
    // background threads watching a process don't report its resulting
    // non-zero exit as a download error.
    state.cancelled.store(true, Ordering::SeqCst);

    match pid {
        Some(pid) => match kill_process_group(pid) {
            Ok(output) if output.status.success() => Ok("Download cancelled".to_string()),
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Err(format!("Failed to kill process {pid}: {stderr}"))
            }
            Err(e) => Err(format!("Failed to run kill: {e}")),
        },
        // busy is true but no child is alive right now — e.g. a batch
        // download between files. Nothing to kill yet; the cancelled flag
        // alone stops the queue before it launches the next one.
        None => Ok("Download cancelled".to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Starts the Tauri application and registers the download commands.
pub fn run() {
    tauri::Builder::default()
        .manage(DownloadState {
            running_pid: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            open_directory_dialog,
            open_file_dialog,
            fetch_video_meta,
            validate_keyword_source,
            start_turbo_download,
            start_keyword_download,
            start_batch_download,
            update_yt_dlp,
            stop_download,
        ])
        // Without this, closing the window while a download is active
        // leaves yt-dlp (and any ffmpeg merge it handed off to) running as
        // an orphaned background process with no way to stop it short of
        // a terminal `pkill` — the app itself is already gone.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<DownloadState>();
                let pid = pid_lock(&state.running_pid).take();
                if let Some(pid) = pid {
                    state.cancelled.store(true, Ordering::SeqCst);
                    let _ = kill_process_group(pid);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Error while running Tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn reserve_download_slot_rejects_when_already_busy() {
        let state = DownloadState {
            running_pid: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        assert!(reserve_download_slot(&state).is_ok());
        assert!(reserve_download_slot(&state).is_err());
    }
}
