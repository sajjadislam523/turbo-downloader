import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React, { useCallback, useEffect, useRef, useState } from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

interface Format {
    id: string;
    label: string;
}

interface VideoMeta {
    title: string;
    formats: Format[];
}

interface DownloadMetrics {
    speed: string;
    eta: string;
    size: string;
}

interface KeywordValidation {
    valid: boolean;
    source_title: string;
    source_type: string;
    entry_count?: number;
    match_count: number;
    sample_titles: string[];
    message: string;
    can_download: boolean;
}

type AppMode = "single" | "batch" | "keyword";
type AppStatus =
    | "idle"
    | "fetching"
    | "ready"
    | "downloading"
    | "done"
    | "error";

// ─────────────────────────────────────────────────────────────────────────────
// Reusable micro-components
// ─────────────────────────────────────────────────────────────────────────────

function StatusDot({ active }: { active: boolean }) {
    return (
        <span
            className={`inline-block w-2 h-2 rounded-full mr-2 shrink-0 ${
                active
                    ? "bg-[#c8ff00] shadow-[0_0_6px_#c8ff00] blink"
                    : "bg-[#3a3a3a]"
            }`}
        />
    );
}

function FieldLabel({
    children,
    className = "",
}: {
    children: React.ReactNode;
    className?: string;
}) {
    return (
        <span
            className={`block text-[10px] font-mono font-medium tracking-[0.2em] uppercase text-[#555] mb-1.5 ${className}`}
        >
            {children}
        </span>
    );
}

function ProgressBar({ percent }: { percent: number }) {
    return (
        <div className="progress-track">
            <div
                className="progress-fill"
                style={{ width: `${Math.min(percent, 100)}%` }}
            />
        </div>
    );
}

function Spinner() {
    return (
        <svg className="animate-spin w-4 h-4" viewBox="0 0 24 24" fill="none">
            <circle
                className="opacity-20"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                strokeWidth="3"
            />
            <path
                className="opacity-80"
                fill="currentColor"
                d="M4 12a8 8 0 018-8v8H4z"
            />
        </svg>
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Toast — readable, word-wrapping, dismissible notification.
//
// The compact status pill in the progress block truncates to a single line,
// which is fine for routine status text but hides anything actionable (like
// a yt-dlp update warning) unless the window is stretched wide. The toast
// gives long/important messages a proper place to be fully readable.
// ─────────────────────────────────────────────────────────────────────────────

interface ToastMessage {
    id: number;
    text: string;
    kind: "error" | "success";
}

function Toast({
    toast,
    onDismiss,
}: {
    toast: ToastMessage | null;
    onDismiss: () => void;
}) {
    if (!toast) return null;
    const isError = toast.kind === "error";

    return (
        <div
            key={toast.id}
            role="alert"
            className={`slide-in absolute bottom-4 left-4 right-4 sm:left-auto sm:right-4 sm:w-[380px] z-50 rounded-xl border px-4 py-3 shadow-2xl backdrop-blur-sm ${
                isError
                    ? "bg-[#1a0505]/95 border-[#ff4455]/40"
                    : "bg-[#051a10]/95 border-[#00ff99]/40"
            }`}
        >
            <div className="flex items-start gap-2.5">
                <span
                    className={`text-[13px] shrink-0 mt-0.5 ${isError ? "text-[#ff4455]" : "text-[#00ff99]"}`}
                >
                    {isError ? "⚠" : "✓"}
                </span>
                <p className="flex-1 min-w-0 text-[11px] font-mono leading-relaxed text-[#ccc] break-words whitespace-pre-wrap">
                    {toast.text}
                </p>
                <button
                    onClick={onDismiss}
                    className="shrink-0 text-[#555] hover:text-[#aaa] text-[13px] leading-none transition-colors"
                    aria-label="Dismiss notification"
                >
                    ✕
                </button>
            </div>
        </div>
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Mode toggle — Single URL / Batch File
// ─────────────────────────────────────────────────────────────────────────────

function ModeToggle({
    mode,
    onChange,
}: {
    mode: AppMode;
    onChange: (m: AppMode) => void;
}) {
    const tabs: { key: AppMode; label: string; icon: string }[] = [
        { key: "single", label: "Single URL", icon: "⚡" },
        { key: "batch", label: "Batch File", icon: "📄" },
        { key: "keyword", label: "Keyword", icon: "⌕" },
    ];

    return (
        <div className="flex gap-1 bg-[#111] border border-[#1e1e1e] rounded-lg p-1">
            {tabs.map((t) => (
                <button
                    key={t.key}
                    onClick={() => onChange(t.key)}
                    className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 rounded-md text-[11px] font-mono font-medium tracking-wider uppercase transition-all ${
                        mode === t.key
                            ? "bg-[#c8ff00] text-[#0a0a0a] shadow-[0_0_10px_#c8ff0033]"
                            : "text-[#555] hover:text-[#888]"
                    }`}
                >
                    <span>{t.icon}</span>
                    {t.label}
                </button>
            ))}
        </div>
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Metrics strip
// ─────────────────────────────────────────────────────────────────────────────

function MetricsStrip({
    metrics,
    percent,
    batchCurrent,
    batchTotal,
    mode,
}: {
    metrics: DownloadMetrics;
    percent: number;
    batchCurrent: number;
    batchTotal: number;
    mode: AppMode;
}) {
    return (
        <div className="flex items-center justify-between text-[11px] font-mono text-[#555] mt-1.5">
            <span>
                <span className="text-[#666]">SPD </span>
                <span className="text-[#c8ff00]">{metrics.speed}</span>
            </span>
            <span>
                <span className="text-[#666]">ETA </span>
                <span className="text-[#aaa]">{metrics.eta}</span>
            </span>
            <span>
                <span className="text-[#666]">SIZE </span>
                <span className="text-[#aaa]">{metrics.size}</span>
            </span>
            {(mode === "batch" || mode === "keyword") && batchTotal > 0 ? (
                <span>
                    <span className="text-[#666]">FILE </span>
                    <span className="text-[#c8ff00]">
                        {batchCurrent}/{batchTotal}
                    </span>
                </span>
            ) : (
                <span>
                    <span className="text-[#666]">DONE </span>
                    <span className="text-[#c8ff00]">
                        {percent.toFixed(1)}%
                    </span>
                </span>
            )}
        </div>
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Parse a yt-dlp stdout line for progress data
// ─────────────────────────────────────────────────────────────────────────────

interface ParsedLine {
    percent?: number;
    speed?: string;
    eta?: string;
    size?: string;
    /** e.g. [2, 5] from "Downloading item 2 of 5" */
    batchItem?: [number, number];
}

function parseProgressLine(raw: string): ParsedLine {
    const line = raw.replace(/\x1b\[[0-9;]*m/g, ""); // strip ANSI codes
    const result: ParsedLine = {};

    // Batch item counter: "[download] Downloading item 2 of 5"
    const batchMatch = line.match(/Downloading item\s+(\d+)\s+of\s+(\d+)/i);
    if (batchMatch) {
        result.batchItem = [
            parseInt(batchMatch[1], 10),
            parseInt(batchMatch[2], 10),
        ];
    }

    // Per-file percentage
    const pct = line.match(/(\d+\.?\d*)%/);
    if (pct) result.percent = parseFloat(pct[1]);

    const spd = line.match(/at\s+([\d.]+\s*\S*B\/s)/i);
    if (spd) result.speed = spd[1];

    const eta = line.match(/ETA\s+([\d:]+)/i);
    if (eta) result.eta = eta[1];

    const size = line.match(/of\s+([\d.]+\s*(?:GiB|MiB|KiB|B))/i);
    if (size) result.size = size[1];

    return result;
}

// ─────────────────────────────────────────────────────────────────────────────
// Raw log strip
// ─────────────────────────────────────────────────────────────────────────────

function RawLogStrip({
    status,
    lastLine,
}: {
    status: AppStatus;
    lastLine: string;
}) {
    const idle =
        status === "idle" || status === "fetching" || status === "ready";

    return (
        <div className="h-[26px] bg-[#0d0d0d] border border-[#181818] rounded-md px-3 flex items-center overflow-hidden">
            <span
                className={`font-mono text-[9px] tracking-wide truncate ${idle ? "text-[#2a2a2a]" : "text-[#444]"}`}
            >
                {idle
                    ? "yt-dlp engine output will stream here…"
                    : lastLine || "Initialising…"}
            </span>
        </div>
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Main App
// ─────────────────────────────────────────────────────────────────────────────

export default function App(): React.JSX.Element {
    // ── Mode ────────────────────────────────────────────────────────────────────
    const [mode, setMode] = useState<AppMode>("single");

    // ── Single-URL state ────────────────────────────────────────────────────────
    const [url, setUrl] = useState<string>("");
    const [meta, setMeta] = useState<VideoMeta | null>(null);
    const [selectedFmt, setSelectedFmt] = useState<string>("");
    const [clipMsg, setClipMsg] = useState<string>("Auto-clipboard active");

    // ── Batch state ──────────────────────────────────────────────────────────────
    const [batchFile, setBatchFile] = useState<string>("");
    const [batchTotal, setBatchTotal] = useState<number>(0);
    const [batchCurrent, setBatchCurrent] = useState<number>(0);
    // 0 = best available; any other value caps the height (e.g. 1080)
    const [batchResolution, setBatchResolution] = useState<number>(0);

    // ── Keyword search state ────────────────────────────────────────────────────
    const [keywordSourceUrl, setKeywordSourceUrl] = useState<string>("");
    const [keywordQuery, setKeywordQuery] = useState<string>("");
    const [keywordLimit, setKeywordLimit] = useState<number>(5);
    const [keywordResolution, setKeywordResolution] = useState<number>(1080);
    const [keywordValidation, setKeywordValidation] =
        useState<KeywordValidation | null>(null);
    const [keywordValidating, setKeywordValidating] = useState<boolean>(false);

    // ── Shared state ────────────────────────────────────────────────────────────
    const [savePath, setSavePath] = useState<string>("~/Downloads");
    const [status, setStatus] = useState<AppStatus>("idle");
    const [statusMsg, setStatusMsg] = useState<string>(
        "Paste a video URL to begin",
    );
    const [progress, setProgress] = useState<number>(0);
    const [metrics, setMetrics] = useState<DownloadMetrics>({
        speed: "--",
        eta: "--",
        size: "--",
    });
    const [rawLine, setRawLine] = useState<string>("");
    const [toast, setToast] = useState<ToastMessage | null>(null);
    const [updatingEngine, setUpdatingEngine] = useState<boolean>(false);

    // ── Refs ─────────────────────────────────────────────────────────────────────
    const lastFetchedUrl = useRef<string>("");
    const lastClipboard = useRef<string>("");
    const clipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    // Debounce timer — waits 600ms after the user stops typing before fetching
    const fetchDebounce = useRef<ReturnType<typeof setTimeout> | null>(null);
    // Generation counter — lets us discard results from superseded fetches
    const fetchGen = useRef<number>(0);
    const keywordValidateGen = useRef<number>(0);
    const keywordValidateDebounce = useRef<ReturnType<typeof setTimeout> | null>(
        null,
    );
    const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

    // Show a toast, replacing any currently-visible one, auto-dismissing
    // after 9s unless the user closes it first.
    const pushToast = useCallback(
        (text: string, kind: ToastMessage["kind"] = "error") => {
            setToast({ id: Date.now(), text, kind });
            if (toastTimer.current) clearTimeout(toastTimer.current);
            toastTimer.current = setTimeout(() => setToast(null), 9000);
        },
        [],
    );

    const dismissToast = useCallback(() => {
        if (toastTimer.current) clearTimeout(toastTimer.current);
        setToast(null);
    }, []);

    // ── Reset shared download state when switching modes ────────────────────────
    const handleModeChange = useCallback((m: AppMode) => {
        setMode(m);
        setStatus("idle");
        setStatusMsg(
            m === "single"
                ? "Paste a video URL to begin"
                : m === "batch"
                  ? "Select a .txt file with URLs"
                  : "Enter a channel, playlist, or video page URL",
        );
        setProgress(0);
        setMetrics({ speed: "--", eta: "--", size: "--" });
        setBatchCurrent(0);
        setBatchTotal(0);
        setKeywordValidation(null);
        setKeywordValidating(false);
    }, []);

    // ── Clipboard monitor ────────────────────────────────────────────────────────
    useEffect(() => {
        let alive = true;
        const poll = async () => {
            if (!alive) return;
            try {
                const text = await navigator.clipboard.readText();
                const t = text.trim();
                if (
                    t !== lastClipboard.current &&
                    (t.startsWith("http://") || t.startsWith("https://"))
                ) {
                    lastClipboard.current = t;
                    // Only auto-paste in single mode
                    if (mode === "single") {
                        setUrl(t);
                        setClipMsg("📋 Link auto-pasted from clipboard!");
                        if (clipTimer.current) clearTimeout(clipTimer.current);
                        clipTimer.current = setTimeout(
                            () => setClipMsg("Auto-clipboard active"),
                            3500,
                        );
                    }
                }
            } catch {
                /* permission denied – ignore */
            }
            if (alive) setTimeout(poll, 1500);
        };
        poll();
        return () => {
            alive = false;
        };
    }, [mode]);

    // ── Auto-fetch on URL change  (debounced 600 ms) ────────────────────────────
    // Without debouncing, pasting a URL fires one fetch per character of the
    // pasted string (React batches state but the effect still fires multiple
    // times while the clipboard auto-paste settles), causing N parallel yt-dlp
    // processes and the "Requested format not available" spam.
    useEffect(() => {
        if (fetchDebounce.current) clearTimeout(fetchDebounce.current);

        fetchDebounce.current = setTimeout(() => {
            const t = url.trim();
            if (
                (t.startsWith("http://") || t.startsWith("https://")) &&
                t !== lastFetchedUrl.current
            ) {
                lastFetchedUrl.current = t;
                triggerFetch(t);
            }
        }, 600);

        return () => {
            if (fetchDebounce.current) clearTimeout(fetchDebounce.current);
        };
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [url]);

    // ── Global event listeners ───────────────────────────────────────────────────
    useEffect(() => {
        const subs: Array<() => void> = [];

        const setup = async () => {
            // Per-file progress
            subs.push(
                await listen<string>("download-progress", (ev) => {
                    const cleanLine = ev.payload
                        .replace(/\x1b\[[0-9;]*m/g, "")
                        .trim();
                    if (cleanLine) setRawLine(cleanLine);

                    const p = parseProgressLine(ev.payload);
                    if (p.percent !== undefined) setProgress(p.percent);
                    if (p.speed !== undefined)
                        setMetrics((m) => ({ ...m, speed: p.speed! }));
                    if (p.eta !== undefined)
                        setMetrics((m) => ({ ...m, eta: p.eta! }));
                    if (p.size !== undefined)
                        setMetrics((m) => ({ ...m, size: p.size! }));
                    if (p.batchItem !== undefined) {
                        setBatchCurrent(p.batchItem[0]);
                        // Reset per-file progress bar when a new file starts
                        setProgress(0);
                        setMetrics({ speed: "--", eta: "--", size: "--" });
                    }
                }),
            );

            // Batch total (emitted by Rust before spawning yt-dlp)
            subs.push(
                await listen<number>("batch-total", (ev) => {
                    setBatchTotal(ev.payload);
                    setBatchCurrent(0);
                }),
            );

            // Batch mode: Rust launches yt-dlp once per URL and emits this
            // right before each one starts. Batch-file downloads never print
            // a "Downloading item X of Y" line for yt-dlp to parse (that only
            // happens for real playlists), so this is the reliable source of
            // truth for the X/Y counter in batch mode — the parseProgressLine
            // batchItem match above still covers keyword mode, where the
            // source really is a playlist/channel and yt-dlp does print it.
            subs.push(
                await listen<number>("batch-item-start", (ev) => {
                    setBatchCurrent(ev.payload);
                    setProgress(0);
                    setMetrics({ speed: "--", eta: "--", size: "--" });
                }),
            );

            subs.push(
                await listen("download-complete", () => {
                    setStatus("done");
                    setProgress(100);
                    setStatusMsg("All downloads complete — files saved!");
                }),
            );

            subs.push(
                await listen<string>("download-error", (ev) => {
                    setStatus("error");
                    setStatusMsg(`Error: ${ev.payload}`);
                    pushToast(ev.payload, "error");
                }),
            );
        };

        setup();
        return () => subs.forEach((u) => u());
    }, [pushToast]);

    const runKeywordValidation = useCallback(
        async (source: string, query: string, limit: number) => {
            const myGen = ++keywordValidateGen.current;
            setKeywordValidating(true);
            setKeywordValidation(null);

            try {
                const result = await invoke<KeywordValidation>(
                    "validate_keyword_source",
                    {
                        sourceUrl: source.trim(),
                        query: query.trim(),
                        resultCount: limit,
                    },
                );
                if (myGen !== keywordValidateGen.current) return;
                setKeywordValidation(result);
                setStatus(result.can_download ? "ready" : "error");
                setStatusMsg(result.message);
            } catch (err) {
                if (myGen !== keywordValidateGen.current) return;
                setKeywordValidation(null);
                setStatus("error");
                setStatusMsg(`Validation failed: ${String(err)}`);
                pushToast(`Validation failed: ${String(err)}`, "error");
            } finally {
                if (myGen === keywordValidateGen.current) {
                    setKeywordValidating(false);
                }
            }
        },
        [pushToast],
    );

    // Debounced keyword source + query validation (800 ms)
    useEffect(() => {
        if (mode !== "keyword") return;

        if (keywordValidateDebounce.current) {
            clearTimeout(keywordValidateDebounce.current);
        }

        const source = keywordSourceUrl.trim();
        const query = keywordQuery.trim();

        if (
            !source.startsWith("http://") &&
            !source.startsWith("https://")
        ) {
            setKeywordValidation(null);
            setKeywordValidating(false);
            setStatus("idle");
            setStatusMsg("Enter a channel, playlist, or video page URL");
            return;
        }

        if (query === "") {
            setKeywordValidation(null);
            setKeywordValidating(false);
            setStatus("idle");
            setStatusMsg("Enter a keyword to filter video titles");
            return;
        }

        setStatus("fetching");
        setStatusMsg("Checking source URL and keyword matches…");

        keywordValidateDebounce.current = setTimeout(() => {
            runKeywordValidation(source, query, keywordLimit);
        }, 800);

        return () => {
            if (keywordValidateDebounce.current) {
                clearTimeout(keywordValidateDebounce.current);
            }
        };
    }, [
        mode,
        keywordSourceUrl,
        keywordQuery,
        keywordLimit,
        runKeywordValidation,
    ]);

    // ── Fetch MP4 formats for a single URL ──────────────────────────────────────
    const triggerFetch = useCallback(async (targetUrl: string) => {
        // Stamp this fetch with a generation number.
        // If another fetch starts before this one finishes, its gen will be higher
        // and we silently discard the stale result instead of overwriting the UI.
        const myGen = ++fetchGen.current;

        setStatus("fetching");
        setStatusMsg("Analyzing video stream…");
        setMeta(null);
        setProgress(0);
        setMetrics({ speed: "--", eta: "--", size: "--" });

        try {
            const result = await invoke<VideoMeta>("fetch_video_meta", {
                url: targetUrl,
            });

            // Discard if a newer fetch has already started
            if (myGen !== fetchGen.current) return;

            setMeta(result);
            setSelectedFmt(result.formats[0]?.id ?? "");
            setStatus("ready");
            setStatusMsg(
                `${result.formats.length} MP4 quality option(s) found`,
            );
        } catch (err) {
            if (myGen !== fetchGen.current) return;
            setStatus("error");
            setStatusMsg(`Fetch failed: ${String(err)}`);
            pushToast(`Fetch failed: ${String(err)}`, "error");
        }
    }, [pushToast]);

    // ── Choose save folder ───────────────────────────────────────────────────────
    const chooseSaveDir = useCallback(async () => {
        try {
            const chosen = await invoke<string>("open_directory_dialog");
            if (chosen) setSavePath(chosen);
        } catch {
            /* cancelled */
        }
    }, []);

    // ── Pick a batch .txt file ───────────────────────────────────────────────────
    const chooseBatchFile = useCallback(async () => {
        try {
            const chosen = await invoke<string>("open_file_dialog");
            if (chosen) {
                setBatchFile(chosen);
                setStatus("ready");
                setStatusMsg("File loaded — ready to download");
                setBatchTotal(0);
                setBatchCurrent(0);
                setProgress(0);
            }
        } catch {
            /* cancelled */
        }
    }, []);

    const handleKeywordSourceChange = useCallback((value: string) => {
        setKeywordSourceUrl(value);
        setKeywordValidation(null);
        setProgress(0);
        setBatchCurrent(0);
        setBatchTotal(0);
    }, []);

    const handleKeywordChange = useCallback((value: string) => {
        setKeywordQuery(value);
        setKeywordValidation(null);
        setProgress(0);
        setBatchCurrent(0);
        setBatchTotal(0);
    }, []);

    // ── Start download (single or batch) ────────────────────────────────────────
    const startDownload = useCallback(async () => {
        setStatus("downloading");
        setProgress(0);
        setMetrics({ speed: "--", eta: "--", size: "--" });

        if (mode === "single") {
            setStatusMsg("Splitting into parallel chunk lanes…");
            try {
                await invoke("start_turbo_download", {
                    url: url.trim(),
                    formatId: selectedFmt,
                    targetDir: savePath,
                });
            } catch (err) {
                setStatus("error");
                setStatusMsg(`Launch failed: ${String(err)}`);
                pushToast(`Launch failed: ${String(err)}`, "error");
            }
        } else if (mode === "batch") {
            setStatusMsg("Reading batch file — starting queue…");
            setBatchCurrent(0);
            try {
                await invoke("start_batch_download", {
                    filePath: batchFile,
                    targetDir: savePath,
                    maxHeight: batchResolution,
                });
            } catch (err) {
                setStatus("error");
                setStatusMsg(`Batch failed: ${String(err)}`);
                pushToast(`Batch failed: ${String(err)}`, "error");
            }
        } else if (mode === "keyword") {
            setStatusMsg("Searching and queuing results…");
            setBatchCurrent(0);
            try {
                await invoke("start_keyword_download", {
                    sourceUrl: keywordSourceUrl.trim(),
                    query: keywordQuery.trim(),
                    targetDir: savePath,
                    maxHeight: keywordResolution,
                    resultCount: keywordLimit,
                });
            } catch (err) {
                setStatus("error");
                setStatusMsg(`Keyword search failed: ${String(err)}`);
                pushToast(`Keyword search failed: ${String(err)}`, "error");
            }
        }
    }, [
        mode,
        url,
        selectedFmt,
        savePath,
        batchFile,
        keywordSourceUrl,
        keywordQuery,
        keywordLimit,
        keywordResolution,
        pushToast,
    ]);

    const cancelDownload = useCallback(async () => {
        try {
            await invoke("stop_download");
            setStatus("idle");
            setStatusMsg("Download cancelled");
            setProgress(0);
            setMetrics({ speed: "--", eta: "--", size: "--" });
            pushToast("Download cancelled", "success");
        } catch (err) {
            pushToast(`Cancel failed: ${String(err)}`, "error");
        }
    }, [pushToast]);

    const updateEngine = useCallback(async () => {
        setUpdatingEngine(true);
        try {
            const result = await invoke<string>("update_yt_dlp");
            pushToast(result, "success");
        } catch (err) {
            pushToast(
                `yt-dlp update failed: ${String(err)}\n\nIf this is a permissions error, either run "sudo yt-dlp -U" in a terminal, or reinstall yt-dlp to a user-writable location like ~/.local/bin.`,
                "error",
            );
        } finally {
            setUpdatingEngine(false);
        }
    }, [pushToast]);

    // ── Derived UI helpers ───────────────────────────────────────────────────────
    const isDownloading = status === "downloading";
    const isFetching = status === "fetching";

    const canDownload =
        !isDownloading &&
        (mode === "single"
            ? (status === "ready" || status === "done") && selectedFmt !== ""
            : mode === "batch"
              ? batchFile !== "" &&
                (status === "ready" || status === "done" || status === "error")
              : mode === "keyword"
                ? keywordSourceUrl.trim() !== "" &&
                  keywordQuery.trim() !== "" &&
                  !keywordValidating &&
                  keywordValidation?.can_download === true &&
                  (status === "ready" ||
                      status === "done" ||
                      status === "error")
                : false);

    const canKeywordDownload =
        mode === "keyword" &&
        !isDownloading &&
        !keywordValidating &&
        keywordValidation?.can_download === true;

    const statusColor: Record<AppStatus, string> = {
        idle: "#3a3a3a",
        fetching: "#6699ff",
        ready: "#c8ff00",
        downloading: "#c8ff00",
        done: "#00ff99",
        error: "#ff4455",
    };

    const truncate = (s: string, n: number) =>
        s.length > n ? s.slice(0, n) + "…" : s;
    const displaySavePath =
        savePath.length > 40 ? "…" + savePath.slice(-38) : savePath;
    const displayBatchFile = batchFile
        ? (batchFile.split("/").pop() ?? batchFile)
        : "No file selected";

    // ─────────────────────────────────────────────────────────────────────────────
    // RENDER
    // ─────────────────────────────────────────────────────────────────────────────
    return (
        <div className="relative w-full h-full bg-[#0a0a0a] flex flex-col overflow-hidden scanlines">
            {/* Noise overlay */}
            <div
                className="pointer-events-none absolute inset-0 opacity-[0.03]"
                style={{
                    backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")`,
                    backgroundSize: "128px 128px",
                }}
            />

            {/* Error / success notifications — replaces the old cramped truncated status pill for anything that actually needs reading in full */}
            <Toast toast={toast} onDismiss={dismissToast} />

            {/* ── Header ── */}
            <header className="flex items-center justify-between gap-4 px-5 sm:px-7 pt-4 sm:pt-5 pb-0 shrink-0">
                <div>
                    <h1 className="font-display text-[20px] sm:text-[22px] font-extrabold tracking-tight leading-none text-white">
                        TURBO<span className="text-[#c8ff00]">DL</span>
                        <span className="ml-2 text-[10px] font-mono font-normal tracking-[0.15em] text-[#3a3a3a] align-middle">
                            MP4 · ONLY
                        </span>
                    </h1>
                    <p className="font-mono text-[8px] sm:text-[9px] tracking-[0.18em] sm:tracking-[0.25em] uppercase text-[#333] mt-0.5">
                        Ultra · Parallel Chunk Engine · yt-dlp
                    </p>
                </div>

                <div className="flex items-center gap-2">
                    {/* Clipboard badge — only relevant in single mode */}
                    {mode === "single" && (
                        <div className="hidden sm:flex items-center gap-1.5 bg-[#111] border border-[#1e1e1e] rounded-full px-3 py-1 min-w-0">
                            <StatusDot
                                active={clipMsg !== "Auto-clipboard active"}
                            />
                            <span className="font-mono text-[9px] tracking-wider uppercase text-[#555]">
                                {truncate(clipMsg, 30)}
                            </span>
                        </div>
                    )}

                    {/* Update Engine — runs `yt-dlp -U` and reports the result via toast */}
                    <button
                        onClick={updateEngine}
                        disabled={updatingEngine}
                        title="Update yt-dlp"
                        className="flex items-center gap-1.5 bg-[#111] border border-[#1e1e1e] hover:border-[#333] rounded-full px-3 py-1 transition-colors disabled:opacity-50"
                    >
                        <span
                            className={`text-[#888] text-[10px] ${updatingEngine ? "animate-spin" : ""}`}
                        >
                            ⟳
                        </span>
                        <span className="font-mono text-[9px] tracking-wider uppercase text-[#555]">
                            {updatingEngine ? "Updating…" : "Update Engine"}
                        </span>
                    </button>
                </div>
            </header>

            {/* Divider */}
            <div className="mx-5 sm:mx-7 mt-3 mb-0 h-px bg-linear-to-r from-transparent via-[#1e1e1e] to-transparent" />

            {/* ── Body ── */}
            <main className="flex-1 min-h-0 flex flex-col gap-3 px-5 sm:px-7 pt-3 sm:pt-4 pb-3 overflow-y-auto overflow-x-hidden">
                {/* MODE TOGGLE */}
                <ModeToggle mode={mode} onChange={handleModeChange} />

                {/* ── SINGLE MODE INPUT ── */}
                {mode === "single" && (
                    <>
                        <div>
                            <FieldLabel>Video URL</FieldLabel>
                            <div className="relative">
                                <input
                                    type="text"
                                    value={url}
                                    onChange={(e) => setUrl(e.target.value)}
                                    placeholder="https://youtube.com/watch?v=…"
                                    className="
                    acid-focus w-full bg-[#111] border border-[#242424] rounded-lg
                    px-4 py-3 pr-11 text-[13px] font-mono text-[#ddd]
                    placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30
                  "
                                />
                                <span className="absolute right-3.5 top-1/2 -translate-y-1/2">
                                    {isFetching ? (
                                        <span className="text-[#c8ff00]">
                                            <Spinner />
                                        </span>
                                    ) : status === "ready" ||
                                      status === "done" ? (
                                        <span className="text-[#c8ff00] text-sm">
                                            ✓
                                        </span>
                                    ) : (
                                        <span className="text-[#2a2a2a] text-sm">
                                            ⚡
                                        </span>
                                    )}
                                </span>
                            </div>
                        </div>

                        {/* Video title pill */}
                        {meta && (
                            <div className="slide-in bg-[#111] border border-[#1e1e1e] rounded-lg px-4 py-2 flex items-center gap-2.5">
                                <span className="text-[#c8ff00] text-[10px] font-mono tracking-widest uppercase shrink-0">
                                    TITLE
                                </span>
                                <span className="text-[12px] text-[#aaa] font-body truncate">
                                    {truncate(meta.title, 60)}
                                </span>
                            </div>
                        )}
                    </>
                )}

                {/* ── BATCH MODE INPUT ── */}
                {mode === "batch" && (
                    <div>
                        <FieldLabel>
                            Link List File (.txt — one URL per line)
                        </FieldLabel>
                        <div className="flex flex-col sm:flex-row sm:items-stretch gap-2">
                            {/* File display */}
                            <div className="flex-1 bg-[#111] border border-[#242424] rounded-lg px-4 py-3 flex items-center gap-2.5 min-w-0">
                                <span className="text-[#c8ff00] text-[13px] shrink-0">
                                    📄
                                </span>
                                <span className="font-mono text-[12px] text-[#666] truncate">
                                    {displayBatchFile}
                                </span>
                                {batchTotal > 0 && (
                                    <span className="ml-auto shrink-0 font-mono text-[10px] text-[#c8ff00] bg-[#c8ff0011] px-2 py-0.5 rounded-full border border-[#c8ff0033]">
                                        {batchTotal} URLs
                                    </span>
                                )}
                            </div>
                            {/* Browse button */}
                            <button
                                onClick={chooseBatchFile}
                                className="
                  shrink-0 min-h-[40px] bg-[#161616] border border-[#2a2a2a] hover:border-[#444]
                  text-[#888] hover:text-[#ccc] rounded-lg px-4 py-2 text-[11px]
                  font-mono tracking-wider uppercase transition-all
                "
                            >
                                Browse
                            </button>
                        </div>
                        {/* Batch resolution picker */}
                        <div className="mt-2.5 flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-3">
                            <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[#444] shrink-0">
                                Max Resolution
                            </span>
                            <div className="grid grid-cols-4 sm:flex gap-1.5 sm:flex-wrap">
                                {(
                                    [
                                        { label: "Best", value: 0 },
                                        { label: "4K", value: 2160 },
                                        { label: "1440", value: 1440 },
                                        { label: "1080", value: 1080 },
                                        { label: "720", value: 720 },
                                        { label: "480", value: 480 },
                                        { label: "360", value: 360 },
                                    ] as { label: string; value: number }[]
                                ).map((opt) => (
                                    <button
                                        key={opt.value}
                                        onClick={() =>
                                            setBatchResolution(opt.value)
                                        }
                                        className={`px-2.5 py-1 rounded-md font-mono text-[10px] tracking-wider uppercase transition-all border ${
                                            batchResolution === opt.value
                                                ? "bg-[#c8ff00] text-[#0a0a0a] border-[#c8ff00] shadow-[0_0_8px_#c8ff0033]"
                                                : "bg-transparent text-[#555] border-[#2a2a2a] hover:border-[#444] hover:text-[#888]"
                                        }`}
                                    >
                                        {opt.label}
                                    </button>
                                ))}
                            </div>
                        </div>
                        <p className="mt-2 font-mono text-[9px] text-[#333] tracking-wide">
                            Plain .txt file · one URL per line · lines starting
                            with # are skipped
                        </p>
                    </div>
                )}

                {/* ── KEYWORD MODE INPUT ── */}
                {mode === "keyword" && (
                    <div>
                        <FieldLabel>Source URL</FieldLabel>
                        <div className="relative">
                            <input
                                type="text"
                                value={keywordSourceUrl}
                                onChange={(event) =>
                                    handleKeywordSourceChange(
                                        event.target.value,
                                    )
                                }
                                placeholder="https://youtube.com/@channel/videos or playlist URL"
                                className="
                    acid-focus w-full bg-[#111] border border-[#242424] rounded-lg
                    px-4 py-3 pr-11 text-[13px] font-mono text-[#ddd]
                    placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30
                  "
                            />
                            <span className="absolute right-3.5 top-1/2 -translate-y-1/2">
                                {keywordValidating ? (
                                    <span className="text-[#6699ff]">
                                        <Spinner />
                                    </span>
                                ) : keywordValidation?.can_download ? (
                                    <span className="text-[#c8ff00] text-sm">
                                        ✓
                                    </span>
                                ) : keywordValidation &&
                                    !keywordValidation.can_download ? (
                                    <span className="text-[#ff4455] text-sm">
                                        ✕
                                    </span>
                                ) : (
                                    <span className="text-[#2a2a2a] text-sm">
                                        🔗
                                    </span>
                                )}
                            </span>
                        </div>
                        {keywordValidation && (
                            <div
                                className={`slide-in mt-2 rounded-lg border px-4 py-2 ${
                                    keywordValidation.can_download
                                        ? "bg-[#111] border-[#1e1e1e]"
                                        : "bg-[#1a0505] border-[#ff4455]/30"
                                }`}
                            >
                                <p className="text-[11px] font-mono text-[#aaa] leading-relaxed">
                                    {keywordValidation.message}
                                </p>
                                {keywordValidation.sample_titles.length > 0 && (
                                    <ul className="mt-1.5 space-y-0.5">
                                        {keywordValidation.sample_titles.map(
                                            (title) => (
                                                <li
                                                    key={title}
                                                    className="text-[10px] font-mono text-[#666] truncate"
                                                >
                                                    · {title}
                                                </li>
                                            ),
                                        )}
                                    </ul>
                                )}
                            </div>
                        )}
                        <p className="mt-2 font-mono text-[9px] text-[#333] tracking-wide">
                            Channel, playlist, or single video page · titles
                            are filtered by your keyword before download
                        </p>
                        <FieldLabel className="mt-4">Search Keyword</FieldLabel>
                        <div className="relative">
                            <input
                                type="text"
                                value={keywordQuery}
                                onChange={(event) =>
                                    handleKeywordChange(event.target.value)
                                }
                                placeholder="artist name live performance"
                                className="
                    acid-focus w-full bg-[#111] border border-[#242424] rounded-lg
                    px-4 py-3 pr-11 text-[13px] font-mono text-[#ddd]
                    placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30
                  "
                            />
                            <span className="absolute right-3.5 top-1/2 -translate-y-1/2 text-[#2a2a2a] text-sm">
                                ⌕
                            </span>
                        </div>
                        <div className="mt-2.5 grid grid-cols-1 sm:grid-cols-2 gap-3">
                            <div>
                                <FieldLabel>Results</FieldLabel>
                                <select
                                    value={keywordLimit}
                                    onChange={(event) =>
                                        setKeywordLimit(
                                            Number(event.target.value),
                                        )
                                    }
                                    className="
                      acid-focus w-full h-[40px] bg-[#111] border border-[#242424] rounded-lg
                      px-3 text-[11px] font-mono text-[#aaa] focus:border-[#c8ff00]/30
                      transition-all appearance-none cursor-pointer
                    "
                                    style={{ backgroundImage: "none" }}
                                >
                                    {[1, 3, 5, 10, 15, 20].map((count) => (
                                        <option key={count} value={count}>
                                            Top {count}
                                        </option>
                                    ))}
                                </select>
                            </div>
                            <div>
                                <FieldLabel>Max Resolution</FieldLabel>
                                <div className="grid grid-cols-4 gap-1.5">
                                    {(
                                        [
                                            { label: "Best", value: 0 },
                                            { label: "1080", value: 1080 },
                                            { label: "720", value: 720 },
                                            { label: "480", value: 480 },
                                        ] as {
                                            label: string;
                                            value: number;
                                        }[]
                                    ).map((option) => (
                                        <button
                                            key={option.value}
                                            onClick={() =>
                                                setKeywordResolution(
                                                    option.value,
                                                )
                                            }
                                            className={`px-2 py-2 rounded-md font-mono text-[10px] tracking-wider uppercase transition-all border ${
                                                keywordResolution ===
                                                option.value
                                                    ? "bg-[#c8ff00] text-[#0a0a0a] border-[#c8ff00] shadow-[0_0_8px_#c8ff0033]"
                                                    : "bg-transparent text-[#555] border-[#2a2a2a] hover:border-[#444] hover:text-[#888]"
                                            }`}
                                        >
                                            {option.label}
                                        </button>
                                    ))}
                                </div>
                            </div>
                        </div>
                    </div>
                )}

                {/* ── SAVE PATH + FORMAT (shared row) ── */}
                <div
                    className={`grid gap-3 ${mode === "single" && meta ? "grid-cols-1 sm:grid-cols-2" : "grid-cols-1"}`}
                >
                    {/* Save path */}
                    <div>
                        <FieldLabel>Save To</FieldLabel>
                        <div
                            onClick={chooseSaveDir}
                            className="
                flex items-center gap-2 bg-[#111] border border-[#242424] rounded-lg
                px-3 py-2.5 cursor-pointer hover:border-[#3a3a3a] transition-colors group h-[44px]
              "
                        >
                            <span className="text-[#c8ff00] text-[12px] shrink-0">
                                📁
                            </span>
                            <span className="font-mono text-[11px] text-[#555] group-hover:text-[#888] transition-colors truncate">
                                {displaySavePath}
                            </span>
                        </div>
                    </div>

                    {/* MP4 Format dropdown — single mode only, after fetch */}
                    {mode === "single" && meta && (
                        <div className="slide-in">
                            <FieldLabel>MP4 Quality</FieldLabel>
                            <select
                                disabled={isFetching}
                                value={selectedFmt}
                                onChange={(e) => setSelectedFmt(e.target.value)}
                                className="
                  acid-focus w-full h-[44px] bg-[#111] border border-[#242424] rounded-lg
                  px-3 text-[11px] font-mono text-[#aaa] disabled:opacity-40
                  disabled:cursor-not-allowed focus:border-[#c8ff00]/30 transition-all
                  appearance-none cursor-pointer
                "
                                style={{ backgroundImage: "none" }}
                            >
                                {meta.formats.map((f) => (
                                    <option key={f.id} value={f.id}>
                                        {f.label.length > 50
                                            ? f.label.slice(0, 50) + "…"
                                            : f.label}
                                    </option>
                                ))}
                            </select>
                        </div>
                    )}

                    {/* Batch mode: MP4 badge instead of dropdown */}
                    {(mode === "batch" || mode === "keyword") && (
                        <div>
                            <FieldLabel>Output Format</FieldLabel>
                            <div className="h-[44px] bg-[#111] border border-[#1e1e1e] rounded-lg px-3 flex items-center gap-2">
                                <span className="text-[#c8ff00] text-[11px]">
                                    ✦
                                </span>
                                <span className="font-mono text-[11px] text-[#666]">
                                    Best MP4 (auto-selected per video)
                                </span>
                            </div>
                        </div>
                    )}
                </div>

                {/* ── PROGRESS BLOCK ── */}
                <div>
                    <div className="flex items-center justify-between mb-1.5">
                        <FieldLabel>
                            {(mode === "batch" || mode === "keyword") &&
                            batchTotal > 0
                                ? `Progress — file ${batchCurrent} of ${batchTotal}`
                                : "Download Progress"}
                        </FieldLabel>
                        <span
                            className="min-w-0 max-w-[58%] text-right font-mono text-[10px] tracking-widest uppercase flex items-center justify-end"
                            style={{ color: statusColor[status] }}
                        >
                            <StatusDot active={isDownloading} />
                            <span className="truncate">{statusMsg}</span>
                        </span>
                    </div>
                    <ProgressBar percent={progress} />
                    <MetricsStrip
                        metrics={metrics}
                        percent={progress}
                        batchCurrent={batchCurrent}
                        batchTotal={batchTotal}
                        mode={mode}
                    />
                </div>

                {/* LOG STRIP */}
                <RawLogStrip status={status} lastLine={rawLine} />

                {/* DOWNLOAD / CANCEL BUTTONS */}
                <div className="flex gap-2">
                    <button
                        disabled={!canDownload}
                        onClick={startDownload}
                        className="
                flex-1 min-h-12 rounded-xl font-display font-bold text-[13px] sm:text-[14px]
                tracking-widest uppercase transition-all
                disabled:opacity-25 disabled:cursor-not-allowed
                enabled:hover:scale-[1.01] enabled:active:scale-[0.99]
              "
                        style={{
                            background: isDownloading
                                ? "linear-gradient(90deg, #1a2a00, #263800)"
                                : "linear-gradient(90deg, #3d4f00, #c8ff00 60%, #9cbf00)",
                            color: isDownloading ? "#c8ff00" : "#0a0a0a",
                            boxShadow:
                                canDownload && !isDownloading
                                    ? "0 0 20px #c8ff0033, 0 2px 0 #9cbf00"
                                    : "none",
                        }}
                    >
                        {isDownloading ? (
                            <span className="flex items-center justify-center gap-2">
                                <Spinner />
                                {mode === "batch" || mode === "keyword"
                                    ? batchTotal > 0
                                        ? `Downloading ${batchCurrent}/${batchTotal}…`
                                        : "Preparing queue…"
                                    : "Chunk lanes active…"}
                            </span>
                        ) : mode === "batch" ? (
                            "⚡ Start Batch Download (MP4)"
                        ) : mode === "keyword" ? (
                            canKeywordDownload ? (
                                "⚡ Start Keyword Search & Download (MP4)"
                            ) : keywordValidating ? (
                                "Checking URL and keyword matches…"
                            ) : keywordSourceUrl.trim() === "" ? (
                                "Enter Source URL to Begin"
                            ) : keywordQuery.trim() === "" ? (
                                "Enter Keyword to Validate"
                            ) : keywordValidation &&
                              !keywordValidation.can_download ? (
                                "No Matching Videos — Adjust URL or Keyword"
                            ) : (
                                "Waiting for URL validation…"
                            )
                        ) : (
                            "⚡ Download via Parallel Chunks"
                        )}
                    </button>

                    {/* Cancel button — only visible while a download is running */}
                    {isDownloading && (
                        <button
                            onClick={cancelDownload}
                            className="
                  shrink-0 min-h-12 px-4 rounded-xl font-mono font-medium text-[11px]
                  tracking-widest uppercase transition-all
                  bg-[#1a0505] border border-[#ff4455]/40 text-[#ff4455]
                  hover:bg-[#2a0505] hover:border-[#ff4455]/60
                  active:scale-[0.97]
                "
                        >
                            Cancel
                        </button>
                    )}
                </div>
            </main>

            {/* ── Footer ── */}
            <footer className="shrink-0 hidden sm:flex items-center justify-between px-7 py-2 border-t border-[#111]">
                <span className="font-mono text-[9px] uppercase tracking-[0.2em] text-[#2a2a2a]">
                    yt-dlp · ffmpeg · 5× fragment engine
                </span>
                <span className="font-mono text-[9px] uppercase tracking-[0.2em] text-[#2a2a2a]">
                    Tauri · React · Rust
                </span>
            </footer>
        </div>
    );
}
