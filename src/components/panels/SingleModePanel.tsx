import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import { useQueue } from "../../state/QueueContext";
import type { ToastMessage, VideoMeta } from "../../types";
import { FieldLabel, ResolutionPicker, Spinner, StatusDot, truncate } from "../ui";

type FetchStatus = "idle" | "fetching" | "ready" | "error";

export default function SingleModePanel({
    savePath,
    pushToast,
}: {
    savePath: string;
    pushToast: (text: string, kind: ToastMessage["kind"]) => void;
}) {
    const { enqueueDownload, enqueueUrls } = useQueue();

    const [url, setUrl] = useState<string>("");
    const [meta, setMeta] = useState<VideoMeta | null>(null);
    const [selectedFmt, setSelectedFmt] = useState<string>("");
    const [fetchStatus, setFetchStatus] = useState<FetchStatus>("idle");
    const [bulkResolution, setBulkResolution] = useState<number>(0);
    const [clipMsg, setClipMsg] = useState<string>("Auto-clipboard active");
    const [busy, setBusy] = useState<boolean>(false);

    const lastFetchedUrl = useRef<string>("");
    const lastClipboard = useRef<string>("");
    const clipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
    const fetchGen = useRef<number>(0);

    // Keeping pushToast behind a ref (rather than in the fetch effect's own
    // dependency array) is what lets that effect depend on nothing but `url`
    // — see the effect below.
    const pushToastRef = useRef(pushToast);
    useEffect(() => {
        pushToastRef.current = pushToast;
    }, [pushToast]);

    const lines = url
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean);
    const isBulk = lines.length > 1;

    // ── Clipboard auto-paste — only relevant while this panel is mounted,
    // i.e. only in single mode, same as before. ─────────────────────────────
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
                    setUrl(t);
                    setClipMsg("📋 Link auto-pasted from clipboard!");
                    if (clipTimer.current) clearTimeout(clipTimer.current);
                    clipTimer.current = setTimeout(
                        () => setClipMsg("Auto-clipboard active"),
                        3500,
                    );
                }
            } catch {
                /* permission denied — ignore */
            }
            if (alive) setTimeout(poll, 1500);
        };
        poll();
        return () => {
            alive = false;
        };
    }, []);

    // ── Auto-fetch metadata for a single pasted URL (debounced 600ms) ──────
    // Only `url` is a dependency — pushToast is read via a ref above, and the
    // fetch itself is inlined here rather than called through an external
    // useCallback, so there's no unstable-function-identity retrigger risk.
    useEffect(() => {
        if (isBulk) return;
        const t = lines[0] ?? "";
        if (!(t.startsWith("http://") || t.startsWith("https://"))) {
            return;
        }
        if (t === lastFetchedUrl.current) return;

        const timer = setTimeout(async () => {
            lastFetchedUrl.current = t;
            const myGen = ++fetchGen.current;
            setFetchStatus("fetching");
            setMeta(null);
            try {
                const result = await invoke<VideoMeta>("fetch_video_meta", { url: t });
                if (myGen !== fetchGen.current) return;
                setMeta(result);
                setSelectedFmt(result.formats[0]?.id ?? "");
                setFetchStatus("ready");
            } catch (err) {
                if (myGen !== fetchGen.current) return;
                setFetchStatus("error");
                pushToastRef.current(`Fetch failed: ${String(err)}`, "error");
            }
        }, 600);

        return () => clearTimeout(timer);
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [url]);

    const addToQueue = useCallback(async () => {
        setBusy(true);
        try {
            if (isBulk) {
                await enqueueUrls(lines, savePath, bulkResolution, "single");
                pushToast(`Queued ${lines.length} link(s)`, "success");
                setUrl("");
            } else {
                await enqueueDownload(lines[0] ?? "", selectedFmt, savePath);
                pushToast("Added to queue", "success");
            }
        } catch (err) {
            pushToast(`Could not add to queue: ${String(err)}`, "error");
        } finally {
            setBusy(false);
        }
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [isBulk, lines, savePath, bulkResolution, selectedFmt, enqueueDownload, enqueueUrls, pushToast]);

    const canAdd = isBulk
        ? lines.length > 1 && !busy
        : lines.length === 1 && fetchStatus === "ready" && selectedFmt !== "" && !busy;

    return (
        <div className="flex flex-col gap-1.5">
            <div>
                <div className="flex items-center justify-between mb-1">
                    <FieldLabel className="mb-0">
                        {isBulk ? `${lines.length} link(s) detected` : "Video URL"}
                    </FieldLabel>
                    <span className="font-mono text-[8px] tracking-wider uppercase text-[#555]">
                        <StatusDot active={clipMsg !== "Auto-clipboard active"} />
                        {truncate(clipMsg, 24)}
                    </span>
                </div>
                <div className="relative">
                    <textarea
                        value={url}
                        onChange={(e) => setUrl(e.target.value)}
                        placeholder="https://youtube.com/watch?v=…  (paste multiple lines for several at once)"
                        rows={isBulk ? 3 : 1}
                        className="acid-focus w-full bg-[#111] border border-[#242424] rounded-md px-3 py-1.5 pr-8 text-[12px] font-mono text-[#ddd] placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30 resize-y"
                    />
                    <span className="absolute right-2.5 top-1.5">
                        {fetchStatus === "fetching" ? (
                            <span className="text-[#c8ff00]">
                                <Spinner />
                            </span>
                        ) : fetchStatus === "ready" ? (
                            <span className="text-[#c8ff00] text-sm">✓</span>
                        ) : (
                            <span className="text-[#2a2a2a] text-sm">⚡</span>
                        )}
                    </span>
                </div>
            </div>

            {!isBulk && meta && (
                <div className="slide-in flex items-center gap-2">
                    <div className="flex-1 min-w-0 bg-[#111] border border-[#1e1e1e] rounded-md px-2.5 py-1 flex items-center gap-2">
                        <span className="text-[#c8ff00] text-[9px] font-mono tracking-widest uppercase shrink-0">
                            TITLE
                        </span>
                        <span className="text-[11px] text-[#aaa] font-body truncate">
                            {truncate(meta.title, 40)}
                        </span>
                    </div>
                    <select
                        value={selectedFmt}
                        onChange={(e) => setSelectedFmt(e.target.value)}
                        className="acid-focus shrink-0 w-[110px] h-[30px] bg-[#111] border border-[#242424] rounded-md px-2 text-[10px] font-mono text-[#aaa] focus:border-[#c8ff00]/30 transition-all appearance-none cursor-pointer"
                        style={{ backgroundImage: "none" }}
                    >
                        {meta.formats.map((f) => (
                            <option key={f.id} value={f.id}>
                                {f.label.length > 30 ? f.label.slice(0, 30) + "…" : f.label}
                            </option>
                        ))}
                    </select>
                </div>
            )}

            {isBulk && (
                <div>
                    <FieldLabel>Max Resolution (all queued links)</FieldLabel>
                    <ResolutionPicker value={bulkResolution} onChange={setBulkResolution} />
                </div>
            )}

            <button
                disabled={!canAdd}
                onClick={addToQueue}
                className="min-h-8 rounded-lg font-display font-bold text-[11px] tracking-widest uppercase transition-all disabled:opacity-25 disabled:cursor-not-allowed enabled:hover:scale-[1.01] enabled:active:scale-[0.99]"
                style={{
                    background: "linear-gradient(90deg, #3d4f00, #c8ff00 60%, #9cbf00)",
                    color: "#0a0a0a",
                    boxShadow: canAdd ? "0 0 20px #c8ff0033, 0 2px 0 #9cbf00" : "none",
                }}
            >
                {isBulk ? `⚡ Add ${lines.length} Links to Queue` : "⚡ Add to Queue"}
            </button>
        </div>
    );
}
