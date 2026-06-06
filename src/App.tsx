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

type AppMode = "single" | "batch";
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

function FieldLabel({ children }: { children: React.ReactNode }) {
    return (
        <span className="block text-[10px] font-mono font-medium tracking-[0.2em] uppercase text-[#555] mb-1.5">
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
            {mode === "batch" && batchTotal > 0 ? (
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

function RawLogStrip({ status }: { status: AppStatus }) {
    const [lastLine, setLastLine] = useState<string>("");

    useEffect(() => {
        let unlisten: (() => void) | undefined;
        listen<string>("download-progress", (ev) => {
            const s = ev.payload.replace(/\x1b\[[0-9;]*m/g, "").trim();
            if (s) setLastLine(s);
        }).then((u) => {
            unlisten = u;
        });
        return () => unlisten?.();
    }, []);

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

    // ── Refs ─────────────────────────────────────────────────────────────────────
    const lastFetchedUrl = useRef<string>("");
    const lastClipboard = useRef<string>("");
    const clipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    // Debounce timer — waits 600ms after the user stops typing before fetching
    const fetchDebounce = useRef<ReturnType<typeof setTimeout> | null>(null);
    // Generation counter — lets us discard results from superseded fetches
    const fetchGen = useRef<number>(0);

    // ── Reset shared download state when switching modes ────────────────────────
    const handleModeChange = useCallback((m: AppMode) => {
        setMode(m);
        setStatus("idle");
        setStatusMsg(
            m === "single"
                ? "Paste a video URL to begin"
                : "Select a .txt file with URLs",
        );
        setProgress(0);
        setMetrics({ speed: "--", eta: "--", size: "--" });
        setBatchCurrent(0);
        setBatchTotal(0);
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
                }),
            );
        };

        setup();
        return () => subs.forEach((u) => u());
    }, []);

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
        }
    }, []);

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
            }
        } else {
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
            }
        }
    }, [mode, url, selectedFmt, savePath, batchFile]);

    // ── Derived UI helpers ───────────────────────────────────────────────────────
    const isDownloading = status === "downloading";
    const isFetching = status === "fetching";

    const canDownload =
        !isDownloading &&
        (mode === "single"
            ? (status === "ready" || status === "done") && selectedFmt !== ""
            : batchFile !== "" &&
              (status === "ready" || status === "done" || status === "error"));

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

            {/* ── Header ── */}
            <header className="flex items-center justify-between px-7 pt-5 pb-0 shrink-0">
                <div>
                    <h1 className="font-display text-[22px] font-extrabold tracking-tight leading-none text-white">
                        TURBO<span className="text-[#c8ff00]">DL</span>
                        <span className="ml-2 text-[10px] font-mono font-normal tracking-[0.15em] text-[#3a3a3a] align-middle">
                            MP4 · ONLY
                        </span>
                    </h1>
                    <p className="font-mono text-[9px] tracking-[0.25em] uppercase text-[#333] mt-0.5">
                        Ultra · Parallel Chunk Engine · yt-dlp
                    </p>
                </div>

                {/* Clipboard badge — only relevant in single mode */}
                {mode === "single" && (
                    <div className="flex items-center gap-1.5 bg-[#111] border border-[#1e1e1e] rounded-full px-3 py-1">
                        <StatusDot
                            active={clipMsg !== "Auto-clipboard active"}
                        />
                        <span className="font-mono text-[9px] tracking-wider uppercase text-[#555]">
                            {clipMsg}
                        </span>
                    </div>
                )}
            </header>

            {/* Divider */}
            <div className="mx-7 mt-3.5 mb-0 h-px bg-linear-to-r from-transparent via-[#1e1e1e] to-transparent" />

            {/* ── Body ── */}
            <main className="flex-1 flex flex-col gap-3.5 px-7 pt-4 pb-3 overflow-hidden">
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
                        <div className="flex items-stretch gap-2">
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
                  shrink-0 bg-[#161616] border border-[#2a2a2a] hover:border-[#444]
                  text-[#888] hover:text-[#ccc] rounded-lg px-4 text-[11px]
                  font-mono tracking-wider uppercase transition-all
                "
                            >
                                Browse
                            </button>
                        </div>
                        {/* Batch resolution picker */}
                        <div className="mt-2.5 flex items-center gap-3">
                            <span className="font-mono text-[10px] uppercase tracking-[0.18em] text-[#444] shrink-0">
                                Max Resolution
                            </span>
                            <div className="flex gap-1.5 flex-wrap">
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

                {/* ── SAVE PATH + FORMAT (shared row) ── */}
                <div
                    className={`grid gap-3 ${mode === "single" && meta ? "grid-cols-2" : "grid-cols-1"}`}
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
                    {mode === "batch" && (
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
                            {mode === "batch" && batchTotal > 0
                                ? `Progress — file ${batchCurrent} of ${batchTotal}`
                                : "Download Progress"}
                        </FieldLabel>
                        <span
                            className="font-mono text-[10px] tracking-widest uppercase flex items-center"
                            style={{ color: statusColor[status] }}
                        >
                            <StatusDot active={isDownloading} />
                            {statusMsg}
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
                <RawLogStrip status={status} />

                {/* DOWNLOAD BUTTON */}
                <button
                    disabled={!canDownload}
                    onClick={startDownload}
                    className="
            w-full h-12 rounded-xl font-display font-bold text-[14px]
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
                            {mode === "batch"
                                ? batchTotal > 0
                                    ? `Downloading ${batchCurrent}/${batchTotal}…`
                                    : "Preparing batch queue…"
                                : "Chunk lanes active…"}
                        </span>
                    ) : mode === "batch" ? (
                        "⚡ Start Batch Download (MP4)"
                    ) : (
                        "⚡ Download via Parallel Chunks"
                    )}
                </button>
            </main>

            {/* ── Footer ── */}
            <footer className="shrink-0 flex items-center justify-between px-7 py-2 border-t border-[#111]">
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
