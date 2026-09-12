// ─── Concurrent download queue ────────────────────────────────────────────────
// Replaces the old single-slot DownloadState (one busy flag, one tracked PID)
// with a bounded worker pool: any number of QueueItems can sit `Pending`, and
// a background dispatcher admits up to `max_concurrency` of them to run at
// once, re-reading that limit on every tick so a live change (via
// set_max_concurrency) takes effect immediately without a restart.
//
// A per-host cap (one in-flight item per host, independent of
// max_concurrency) and a per-host cooldown after each finish replace the old
// global 4-8s inter-file sleep — same rate-limit protection, but scoped to
// the host that actually needs it instead of pausing unrelated hosts too.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const DEFAULT_MAX_CONCURRENCY: usize = 3;
const DEFAULT_SAVE_PATH: &str = "~/Downloads";
const MIN_CONCURRENCY: usize = 1;
const MAX_CONCURRENCY: usize = 8;
const DISPATCH_TICK: Duration = Duration::from_millis(250);
const DISK_SPACE_FLOOR_BYTES: u64 = 500 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueItemStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

fn status_str(s: QueueItemStatus) -> &'static str {
    match s {
        QueueItemStatus::Pending => "pending",
        QueueItemStatus::Running => "running",
        QueueItemStatus::Paused => "paused",
        QueueItemStatus::Completed => "completed",
        QueueItemStatus::Failed => "failed",
        QueueItemStatus::Cancelled => "cancelled",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueSource {
    Single,
    Batch,
    Keyword,
    Extension,
}

impl QueueSource {
    fn from_str_or_default(s: &str) -> Self {
        match s {
            "batch" => QueueSource::Batch,
            "keyword" => QueueSource::Keyword,
            "extension" => QueueSource::Extension,
            _ => QueueSource::Single,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct QueueItem {
    pub id: String,
    pub url: String,
    pub source: QueueSource,
    pub status: QueueItemStatus,
    /// Explicit yt-dlp format selector — set for the rich single-URL path
    /// (a specific format the user picked after a metadata probe).
    pub format_id: Option<String>,
    /// Shared max-height cap used by the bulk path (batch/keyword/multi-URL
    /// paste), mutually exclusive in practice with `format_id`.
    pub resolution: Option<u64>,
    pub save_path: String,
    pub title: Option<String>,
    pub error: Option<String>,
    /// Derived once at enqueue time via the `url` crate — drives the
    /// per-host concurrency cap and cooldown below.
    pub host: String,
}

/// What a caller wants enqueued, before an id/host/status are assigned.
pub struct NewItemSpec {
    pub url: String,
    pub title: Option<String>,
    pub source: QueueSource,
    pub format_id: Option<String>,
    pub resolution: Option<u64>,
    pub save_path: String,
}

pub struct QueueStateInner {
    items: Mutex<Vec<QueueItem>>,
    running_pids: Mutex<HashMap<String, u32>>,
    max_concurrency: AtomicUsize,
    active_count: AtomicUsize,
    per_host_active: Mutex<HashMap<String, usize>>,
    host_cooldown_until: Mutex<HashMap<String, Instant>>,
    cancelled_ids: Mutex<HashSet<String>>,
    paused_ids: Mutex<HashSet<String>>,
    shutdown: AtomicBool,
    /// The frontend's current Save-To folder, mirrored here so a deep link
    /// arriving straight from the OS (no frontend call involved at all) has
    /// somewhere correct to land instead of a hardcoded default. Kept in
    /// sync by the frontend calling set_default_save_path on every change
    /// (see App.tsx) — single/batch/keyword downloads don't need this since
    /// the frontend already passes its current save path explicitly with
    /// every enqueue call.
    default_save_path: Mutex<String>,
}

#[derive(Clone)]
pub struct QueueState {
    pub inner: Arc<QueueStateInner>,
}

impl QueueState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(QueueStateInner {
                items: Mutex::new(Vec::new()),
                running_pids: Mutex::new(HashMap::new()),
                max_concurrency: AtomicUsize::new(DEFAULT_MAX_CONCURRENCY),
                active_count: AtomicUsize::new(0),
                per_host_active: Mutex::new(HashMap::new()),
                host_cooldown_until: Mutex::new(HashMap::new()),
                cancelled_ids: Mutex::new(HashSet::new()),
                paused_ids: Mutex::new(HashSet::new()),
                shutdown: AtomicBool::new(false),
                default_save_path: Mutex::new(DEFAULT_SAVE_PATH.to_string()),
            }),
        }
    }

    /// Used by window-close cleanup to kill every currently-running child,
    /// generalizing the old single-PID kill to however many are active.
    pub fn snapshot_running_pids(&self) -> Vec<u32> {
        lk(&self.inner.running_pids).values().copied().collect()
    }

    pub fn request_shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
    }

    /// The save path a deep link should use — see the field doc comment.
    pub fn default_save_path(&self) -> String {
        lk(&self.inner.default_save_path).clone()
    }
}

// Same poison-tolerant pattern as lib.rs's `pid_lock`, generalized to every
// Mutex<T> this module introduces — a poisoned lock here must not cascade
// into a full app abort (the crate builds with `panic = "abort"`), and the
// critical sections around these locks are short, plain data mutations, so
// recovering the inner value from a poisoned lock is safe.
fn lk<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn next_id() -> String {
    static COUNTER: AtomicUsize = AtomicUsize::new(1);
    format!("q{}", COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Best-effort free-space check for the filesystem holding `dir`, via `df`
/// (consistent with this codebase's existing pattern of shelling out to a
/// system tool — curl, kill — rather than adding a crate for one syscall).
/// Returns None when `df` itself can't be read, in which case callers should
/// fail open (allow the download) rather than block on an unrelated `df`
/// quirk.
fn free_space_bytes(dir: &str) -> Option<u64> {
    let expanded = crate::expand_tilde(dir);
    let output = Command::new("df")
        .args(["--output=avail", "-B1", &expanded])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()?
        .trim()
        .parse::<u64>()
        .ok()
}

// How many items from the SAME host may run at once. A flat cap of 1 (the
// original design) meant that queuing several videos from one site — the
// overwhelmingly common case, e.g. every keyword-search result from one
// channel — could only ever run one at a time no matter how high
// max_concurrency was set, which defeats the point of a concurrent queue for
// exactly the workload it exists for. Scaling this with max_concurrency
// instead (roughly half, rounded up, floor 1) keeps the original rate-limit
// protection — same-host bursts are still capped well below max_concurrency
// — while actually delivering parallelism for same-host batches.
fn per_host_cap(max_concurrency: usize) -> usize {
    max_concurrency.div_ceil(2).max(1)
}

fn jitter_cooldown_secs() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    4 + (nanos % 5)
}

/// Builds the yt-dlp Command shared by every queue worker — the args that
/// used to be duplicated across start_turbo_download, start_keyword_download,
/// and spawn_sequential_downloads now live in exactly one place.
fn build_ytdlp_command(url: &str, format_selector: &str, out_template: &str) -> Command {
    let mut cmd = Command::new("yt-dlp");
    cmd.args([
        "-f", format_selector,
        "-o", out_template,
        "--merge-output-format", "mp4",
        "--remux-video", "mp4",
        "--concurrent-fragments", "5",
        "--http-chunk-size", "10485760",
        "--newline",
        "--no-playlist",
        "--sleep-requests", "1",
    ]);
    cmd.args(crate::RETRY_ARGS);
    cmd.arg(url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    // Own process group so cancelling kills any ffmpeg merge child too, not
    // just yt-dlp itself (see kill_process_group).
    cmd.process_group(0);
    cmd
}

/// Pushes new items onto the queue and emits `queue-item-added` for each —
/// the shared primitive behind enqueue_download, enqueue_urls,
/// enqueue_batch_file, and start_keyword_download's match resolution, so
/// "bulk" is a property of how many specs are passed in, not of any one
/// caller.
pub fn push_items(app: &AppHandle, inner: &Arc<QueueStateInner>, specs: Vec<NewItemSpec>) -> Vec<String> {
    let mut ids = Vec::with_capacity(specs.len());
    let mut new_items = Vec::with_capacity(specs.len());
    for spec in specs {
        let id = next_id();
        let host = crate::url_host(&spec.url).unwrap_or_else(|| id.clone());
        new_items.push(QueueItem {
            id: id.clone(),
            url: spec.url,
            source: spec.source,
            status: QueueItemStatus::Pending,
            format_id: spec.format_id,
            resolution: spec.resolution,
            save_path: spec.save_path,
            title: spec.title,
            error: None,
            host,
        });
        ids.push(id);
    }

    lk(&inner.items).extend(new_items.clone());
    for item in &new_items {
        let _ = app.emit("queue-item-added", item);
    }
    ids
}

fn build_url_specs(
    urls: Vec<String>,
    save_path: &str,
    resolution: u64,
    source: QueueSource,
) -> Result<Vec<NewItemSpec>, String> {
    let cleaned: Vec<String> = urls
        .into_iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty() && !u.starts_with('#') && crate::validate_http_url(u).is_ok())
        .collect();

    if cleaned.is_empty() {
        return Err("No valid http(s) URLs found.".to_string());
    }

    Ok(cleaned
        .into_iter()
        .map(|url| NewItemSpec {
            url,
            title: None,
            source,
            format_id: None,
            resolution: Some(resolution),
            save_path: save_path.to_string(),
        })
        .collect())
}

// ─── Commands ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn enqueue_download(
    app: AppHandle,
    state: tauri::State<'_, QueueState>,
    url: String,
    format_id: String,
    save_path: String,
) -> Result<String, String> {
    crate::validate_http_url(&url)?;
    let spec = NewItemSpec {
        url: url.trim().to_string(),
        title: None,
        source: QueueSource::Single,
        format_id: Some(format_id),
        resolution: None,
        save_path,
    };
    let ids = push_items(&app, &state.inner, vec![spec]);
    Ok(ids.into_iter().next().unwrap_or_default())
}

#[tauri::command]
pub fn enqueue_urls(
    app: AppHandle,
    state: tauri::State<'_, QueueState>,
    urls: Vec<String>,
    save_path: String,
    resolution: u64,
    source: String,
) -> Result<Vec<String>, String> {
    let specs = build_url_specs(urls, &save_path, resolution, QueueSource::from_str_or_default(&source))?;
    Ok(push_items(&app, &state.inner, specs))
}

#[tauri::command]
pub fn enqueue_batch_file(
    app: AppHandle,
    state: tauri::State<'_, QueueState>,
    file_path: String,
    save_path: String,
    resolution: u64,
) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Cannot read batch file: {e}"))?;
    let urls: Vec<String> = content.lines().map(|l| l.to_string()).collect();
    let specs = build_url_specs(urls, &save_path, resolution, QueueSource::Batch)?;
    Ok(push_items(&app, &state.inner, specs))
}

#[tauri::command]
pub fn cancel_queue_item(app: AppHandle, state: tauri::State<'_, QueueState>, id: String) -> Result<String, String> {
    let pid = lk(&state.inner.running_pids).get(&id).copied();
    if let Some(pid) = pid {
        lk(&state.inner.cancelled_ids).insert(id.clone());
        crate::kill_process_group(pid).map_err(|e| format!("Failed to run kill: {e}"))?;
        return Ok("Cancelling".to_string());
    }

    let mut items = lk(&state.inner.items);
    if let Some(it) = items.iter_mut().find(|it| {
        it.id == id && matches!(it.status, QueueItemStatus::Pending | QueueItemStatus::Paused)
    }) {
        it.status = QueueItemStatus::Cancelled;
        drop(items);
        let _ = app.emit(
            "queue-item-status-changed",
            serde_json::json!({ "id": id, "status": "cancelled", "error": Option::<String>::None }),
        );
        Ok("Cancelled".to_string())
    } else {
        Err("No matching pending, paused, or running item to cancel".to_string())
    }
}

// ─── Pause / resume ───────────────────────────────────────────────────────
// A "paused" item is killed (or, if it hadn't started yet, simply held back)
// and left in the queue rather than removed. Resuming just flips it back to
// Pending so the dispatcher re-admits it — the resulting yt-dlp invocation is
// identical (same url/format/output path) to the one that was killed, and
// yt-dlp resumes a partially-downloaded file from its .part data by default,
// so no special "resume" logic is needed beyond re-running the same command.
#[tauri::command]
pub fn pause_queue_item(app: AppHandle, state: tauri::State<'_, QueueState>, id: String) -> Result<String, String> {
    let pid = lk(&state.inner.running_pids).get(&id).copied();
    if let Some(pid) = pid {
        lk(&state.inner.paused_ids).insert(id.clone());
        crate::kill_process_group(pid).map_err(|e| format!("Failed to run kill: {e}"))?;
        return Ok("Pausing".to_string());
    }

    let mut items = lk(&state.inner.items);
    if let Some(it) = items
        .iter_mut()
        .find(|it| it.id == id && it.status == QueueItemStatus::Pending)
    {
        it.status = QueueItemStatus::Paused;
        drop(items);
        let _ = app.emit(
            "queue-item-status-changed",
            serde_json::json!({ "id": id, "status": "paused", "error": Option::<String>::None }),
        );
        Ok("Paused".to_string())
    } else {
        Err("No matching pending or running item to pause".to_string())
    }
}

#[tauri::command]
pub fn resume_queue_item(app: AppHandle, state: tauri::State<'_, QueueState>, id: String) -> Result<String, String> {
    let mut items = lk(&state.inner.items);
    if let Some(it) = items
        .iter_mut()
        .find(|it| it.id == id && it.status == QueueItemStatus::Paused)
    {
        it.status = QueueItemStatus::Pending;
        drop(items);
        let _ = app.emit(
            "queue-item-status-changed",
            serde_json::json!({ "id": id, "status": "pending", "error": Option::<String>::None }),
        );
        Ok("Resumed".to_string())
    } else {
        Err("No matching paused item to resume".to_string())
    }
}

#[tauri::command]
pub fn cancel_all_queue_items(app: AppHandle, state: tauri::State<'_, QueueState>) -> Result<String, String> {
    let pids: Vec<(String, u32)> = lk(&state.inner.running_pids)
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    for (id, pid) in &pids {
        lk(&state.inner.cancelled_ids).insert(id.clone());
        let _ = crate::kill_process_group(*pid);
    }

    let mut cancelled_pending: Vec<String> = Vec::new();
    {
        let mut items = lk(&state.inner.items);
        for it in items.iter_mut() {
            if matches!(it.status, QueueItemStatus::Pending | QueueItemStatus::Paused) {
                it.status = QueueItemStatus::Cancelled;
                cancelled_pending.push(it.id.clone());
            }
        }
    }
    // Running items' own status-changed events fire from finish_item once
    // their process actually exits — emitting again here would just be a
    // premature duplicate.
    for id in &cancelled_pending {
        let _ = app.emit(
            "queue-item-status-changed",
            serde_json::json!({ "id": id, "status": "cancelled", "error": Option::<String>::None }),
        );
    }
    Ok(format!(
        "Cancelling {} running and {} pending item(s)",
        pids.len(),
        cancelled_pending.len()
    ))
}

#[tauri::command]
pub fn set_max_concurrency(state: tauri::State<'_, QueueState>, value: usize) -> Result<usize, String> {
    let clamped = value.clamp(MIN_CONCURRENCY, MAX_CONCURRENCY);
    state.inner.max_concurrency.store(clamped, Ordering::SeqCst);
    Ok(clamped)
}

// Keeps handle_incoming_deep_link's save path in sync with whatever the
// frontend currently has selected — see the default_save_path field doc
// comment on QueueStateInner. The frontend calls this on mount and on every
// change of its own savePath state (App.tsx).
#[tauri::command]
pub fn set_default_save_path(state: tauri::State<'_, QueueState>, path: String) -> Result<(), String> {
    *lk(&state.inner.default_save_path) = path;
    Ok(())
}

#[tauri::command]
pub fn get_queue_snapshot(state: tauri::State<'_, QueueState>) -> Result<Vec<QueueItem>, String> {
    Ok(lk(&state.inner.items).clone())
}

#[tauri::command]
pub fn remove_completed_item(state: tauri::State<'_, QueueState>, id: String) -> Result<String, String> {
    let mut items = lk(&state.inner.items);
    let before = items.len();
    items.retain(|it| {
        !(it.id == id
            && matches!(
                it.status,
                QueueItemStatus::Completed | QueueItemStatus::Failed | QueueItemStatus::Cancelled
            ))
    });
    if items.len() == before {
        Err("No matching terminal item to remove".to_string())
    } else {
        Ok(id)
    }
}

#[tauri::command]
pub fn clear_completed(state: tauri::State<'_, QueueState>) -> Result<usize, String> {
    let mut items = lk(&state.inner.items);
    let before = items.len();
    items.retain(|it| {
        matches!(
            it.status,
            QueueItemStatus::Pending | QueueItemStatus::Running | QueueItemStatus::Paused
        )
    });
    Ok(before - items.len())
}

// ─── Dispatcher ───────────────────────────────────────────────────────────

/// One long-lived background thread, spawned once at app startup, that
/// admits `Pending` items up to the live `max_concurrency` value. Polls on a
/// short fixed interval rather than a condvar wake-up — simpler to reason
/// about correctly at this scale (at most 8 concurrent workers), and cheap
/// enough that sub-second admission latency doesn't matter here.
pub fn start_dispatcher(app: AppHandle, inner: Arc<QueueStateInner>) {
    std::thread::spawn(move || loop {
        if inner.shutdown.load(Ordering::SeqCst) {
            break;
        }
        dispatch_tick(&app, &inner);
        std::thread::sleep(DISPATCH_TICK);
    });
}

fn dispatch_tick(app: &AppHandle, inner: &Arc<QueueStateInner>) {
    loop {
        let max = inner.max_concurrency.load(Ordering::SeqCst);
        let active = inner.active_count.load(Ordering::SeqCst);
        if active >= max {
            return;
        }

        let host_cap = per_host_cap(max);
        let candidate = {
            let items = lk(&inner.items);
            let host_active = lk(&inner.per_host_active);
            let host_cooldown = lk(&inner.host_cooldown_until);
            let now = Instant::now();
            items
                .iter()
                .find(|it| {
                    it.status == QueueItemStatus::Pending
                        && host_active.get(&it.host).copied().unwrap_or(0) < host_cap
                        && host_cooldown
                            .get(&it.host)
                            .map(|until| now >= *until)
                            .unwrap_or(true)
                })
                .cloned()
        };

        let Some(candidate) = candidate else {
            return;
        };

        if let Some(free) = free_space_bytes(&candidate.save_path) {
            if free < DISK_SPACE_FLOOR_BYTES {
                mark_item_failed(
                    app,
                    inner,
                    &candidate.id,
                    "Insufficient disk space to start this download.".to_string(),
                );
                continue;
            }
        }

        let admitted = {
            let mut items = lk(&inner.items);
            items.iter_mut().find(|it| it.id == candidate.id).map(|it| {
                it.status = QueueItemStatus::Running;
                it.clone()
            })
        };
        let Some(admitted) = admitted else {
            continue;
        };

        *lk(&inner.per_host_active).entry(admitted.host.clone()).or_insert(0) += 1;
        inner.active_count.fetch_add(1, Ordering::SeqCst);

        let _ = app.emit(
            "queue-item-status-changed",
            serde_json::json!({ "id": admitted.id, "status": "running", "error": Option::<String>::None }),
        );

        spawn_worker(app.clone(), inner.clone(), admitted);
    }
}

fn mark_item_failed(app: &AppHandle, inner: &Arc<QueueStateInner>, id: &str, message: String) {
    {
        let mut items = lk(&inner.items);
        if let Some(it) = items.iter_mut().find(|it| it.id == id) {
            it.status = QueueItemStatus::Failed;
            it.error = Some(message.clone());
        }
    }
    let _ = app.emit(
        "queue-item-status-changed",
        serde_json::json!({ "id": id, "status": "failed", "error": message }),
    );
}

fn spawn_worker(app: AppHandle, inner: Arc<QueueStateInner>, item: QueueItem) {
    std::thread::spawn(move || {
        let format_selector = item
            .format_id
            .clone()
            .unwrap_or_else(|| crate::build_height_selector(item.resolution.unwrap_or(0)));
        let out_template = crate::build_output_template(&item.save_path);
        let mut cmd = build_ytdlp_command(&item.url, &format_selector, &out_template);

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                finish_item(&app, &inner, &item, Err(format!("Failed to launch yt-dlp: {e}")));
                return;
            }
        };
        let pid = child.id();
        lk(&inner.running_pids).insert(item.id.clone(), pid);

        let result = stream_and_wait_item(&app, &item.id, child);

        lk(&inner.running_pids).remove(&item.id);
        // Paused takes priority over cancelled — the two are never set
        // together in practice (pause_queue_item and cancel_queue_item each
        // only touch one set for a given id), but checking paused first
        // keeps that assumption from mattering.
        let was_paused = lk(&inner.paused_ids).remove(&item.id);
        let was_cancelled = lk(&inner.cancelled_ids).remove(&item.id);
        let final_result = if was_paused {
            Err("__paused__".to_string())
        } else if was_cancelled {
            Err("__cancelled__".to_string())
        } else {
            result
        };

        finish_item(&app, &inner, &item, final_result);
    });
}

/// Streams one child's stdout as per-item progress lines and blocks until it
/// exits — the same stderr-drained-concurrently structure as lib.rs's old
/// stream_and_wait/spawn_and_stream (reading stderr only after exit can
/// deadlock: a full pipe buffer blocks yt-dlp's write() indefinitely).
fn stream_and_wait_item(app: &AppHandle, id: &str, mut child: Child) -> Result<(), String> {
    let stdout = child.stdout.take().ok_or("stdout pipe missing")?;
    let stderr = child.stderr.take().ok_or("stderr pipe missing")?;

    let stderr_handle = std::thread::spawn(move || {
        BufReader::new(stderr).lines().map_while(Result::ok).collect::<Vec<String>>()
    });

    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        let _ = app.emit("queue-item-progress", serde_json::json!({ "id": id, "line": line }));
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

fn finish_item(app: &AppHandle, inner: &Arc<QueueStateInner>, item: &QueueItem, result: Result<(), String>) {
    let (status, error) = match result {
        Ok(()) => (QueueItemStatus::Completed, None),
        Err(e) if e == "__cancelled__" => (QueueItemStatus::Cancelled, None),
        Err(e) if e == "__paused__" => (QueueItemStatus::Paused, None),
        Err(e) => (QueueItemStatus::Failed, Some(crate::translate_ytdlp_error(&e, "Download failed."))),
    };

    {
        let mut items = lk(&inner.items);
        if let Some(it) = items.iter_mut().find(|it| it.id == item.id) {
            it.status = status;
            it.error = error.clone();
        }
    }

    if let Some(c) = lk(&inner.per_host_active).get_mut(&item.host) {
        *c = c.saturating_sub(1);
    }
    // A deliberate pause shouldn't cost the item the usual same-host
    // cooldown — that delay exists to space out *finished* downloads
    // against the same host, not to punish the user for wanting to free up
    // a slot right now. Resuming re-admits it as soon as a slot and the
    // per-host cap allow.
    if status != QueueItemStatus::Paused {
        let cooldown = Instant::now() + Duration::from_secs(jitter_cooldown_secs());
        lk(&inner.host_cooldown_until).insert(item.host.clone(), cooldown);
    }

    inner.active_count.fetch_sub(1, Ordering::SeqCst);

    let _ = app.emit(
        "queue-item-status-changed",
        serde_json::json!({ "id": item.id, "status": status_str(status), "error": error }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_url_specs_filters_blank_comment_and_invalid_lines() {
        let urls = vec![
            "https://example.com/a".to_string(),
            "".to_string(),
            "# a comment".to_string(),
            "not a url".to_string(),
            "  http://example.com/b  ".to_string(),
        ];
        let specs = build_url_specs(urls, "/tmp", 1080, QueueSource::Batch).unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].url, "https://example.com/a");
        assert_eq!(specs[1].url, "http://example.com/b");
        assert_eq!(specs[0].resolution, Some(1080));
        assert!(matches!(specs[0].source, QueueSource::Batch));
    }

    #[test]
    fn build_url_specs_rejects_when_nothing_valid_remains() {
        let urls = vec!["".to_string(), "not a url".to_string()];
        assert!(build_url_specs(urls, "/tmp", 0, QueueSource::Single).is_err());
    }

    #[test]
    fn queue_source_from_str_maps_known_values_and_defaults_to_single() {
        assert!(matches!(QueueSource::from_str_or_default("batch"), QueueSource::Batch));
        assert!(matches!(QueueSource::from_str_or_default("keyword"), QueueSource::Keyword));
        assert!(matches!(QueueSource::from_str_or_default("extension"), QueueSource::Extension));
        assert!(matches!(QueueSource::from_str_or_default("single"), QueueSource::Single));
        assert!(matches!(QueueSource::from_str_or_default("garbage"), QueueSource::Single));
    }

    #[test]
    fn next_id_is_unique_and_monotonic() {
        let a = next_id();
        let b = next_id();
        assert_ne!(a, b);
    }

    #[test]
    fn jitter_cooldown_is_within_expected_range() {
        let secs = jitter_cooldown_secs();
        assert!((4..=8).contains(&secs));
    }

    #[test]
    fn status_str_matches_serde_rename() {
        assert_eq!(status_str(QueueItemStatus::Pending), "pending");
        assert_eq!(status_str(QueueItemStatus::Running), "running");
        assert_eq!(status_str(QueueItemStatus::Paused), "paused");
        assert_eq!(status_str(QueueItemStatus::Completed), "completed");
        assert_eq!(status_str(QueueItemStatus::Failed), "failed");
        assert_eq!(status_str(QueueItemStatus::Cancelled), "cancelled");
    }

    #[test]
    fn per_host_cap_scales_with_max_concurrency_but_never_drops_below_one() {
        // Regression test: a flat cap of 1 meant two same-host items queued
        // together (the common case — e.g. two results from one keyword
        // search) could never run at the same time no matter how high
        // max_concurrency was set.
        assert_eq!(per_host_cap(1), 1);
        assert_eq!(per_host_cap(2), 1);
        assert_eq!(per_host_cap(3), 2);
        assert_eq!(per_host_cap(4), 2);
        assert_eq!(per_host_cap(8), 4);
    }

    #[test]
    fn new_queue_state_starts_with_default_concurrency_and_no_items() {
        let state = QueueState::new();
        assert_eq!(state.inner.max_concurrency.load(Ordering::SeqCst), DEFAULT_MAX_CONCURRENCY);
        assert_eq!(lk(&state.inner.items).len(), 0);
        assert!(state.snapshot_running_pids().is_empty());
    }

    #[test]
    fn default_save_path_starts_at_the_fallback_and_reflects_updates() {
        let state = QueueState::new();
        assert_eq!(state.default_save_path(), DEFAULT_SAVE_PATH);

        *lk(&state.inner.default_save_path) = "/mnt/videos".to_string();
        assert_eq!(state.default_save_path(), "/mnt/videos");
    }
}
