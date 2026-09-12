import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import type { KeywordValidation, ToastMessage } from "../../types";
import { FieldLabel, ResolutionPicker, Spinner } from "../ui";

const KEYWORD_RESOLUTION_OPTIONS = [
    { label: "Best", value: 0 },
    { label: "1080", value: 1080 },
    { label: "720", value: 720 },
    { label: "480", value: 480 },
];

// NOTE: keyword mode still auto-queues its top N matches rather than
// offering a full checkbox-driven multi-select list (that full match-list
// UI is tracked as a follow-up in project-docs/plans — see
// parallel-download-queue-and-browser-extension.md Phase 2). Every match it
// does queue lands as its own row in the shared queue and downloads
// concurrently with everything else, same as single/batch mode.
export default function KeywordModePanel({
    savePath,
    pushToast,
}: {
    savePath: string;
    pushToast: (text: string, kind: ToastMessage["kind"]) => void;
}) {
    const [sourceUrl, setSourceUrl] = useState<string>("");
    const [query, setQuery] = useState<string>("");
    const [limit, setLimit] = useState<number>(5);
    const [resolution, setResolution] = useState<number>(1080);
    const [validation, setValidation] = useState<KeywordValidation | null>(null);
    const [validating, setValidating] = useState<boolean>(false);
    const [scanChecked, setScanChecked] = useState<number>(0);
    const [scanMatched, setScanMatched] = useState<number>(0);
    const [queuing, setQueuing] = useState<boolean>(false);

    const validateGen = useRef<number>(0);
    const validateDebounce = useRef<ReturnType<typeof setTimeout> | null>(null);

    useEffect(() => {
        let unsub: (() => void) | undefined;
        listen<{ checked: number; matched: number }>("keyword-scan-progress", (ev) => {
            setScanChecked(ev.payload.checked);
            setScanMatched(ev.payload.matched);
        }).then((u) => {
            unsub = u;
        });
        return () => unsub?.();
    }, []);

    const runValidation = useCallback(
        async (source: string, q: string, count: number) => {
            const myGen = ++validateGen.current;
            setValidating(true);
            setValidation(null);
            setScanChecked(0);
            setScanMatched(0);
            try {
                const result = await invoke<KeywordValidation>("validate_keyword_source", {
                    sourceUrl: source.trim(),
                    query: q.trim(),
                    resultCount: count,
                });
                if (myGen !== validateGen.current) return;
                setValidation(result);
            } catch (err) {
                if (myGen !== validateGen.current) return;
                setValidation(null);
                pushToast(`Validation failed: ${String(err)}`, "error");
            } finally {
                if (myGen === validateGen.current) setValidating(false);
            }
        },
        [pushToast],
    );

    useEffect(() => {
        if (validateDebounce.current) clearTimeout(validateDebounce.current);
        const source = sourceUrl.trim();
        const q = query.trim();

        if (!(source.startsWith("http://") || source.startsWith("https://")) || q === "") {
            setValidation(null);
            setValidating(false);
            return;
        }

        validateDebounce.current = setTimeout(() => {
            runValidation(source, q, limit);
        }, 800);

        return () => {
            if (validateDebounce.current) clearTimeout(validateDebounce.current);
        };
    }, [sourceUrl, query, limit, runValidation]);

    const checkNow = useCallback(() => {
        if (validateDebounce.current) clearTimeout(validateDebounce.current);
        const source = sourceUrl.trim();
        const q = query.trim();
        if (!(source.startsWith("http://") || source.startsWith("https://"))) {
            pushToast("Enter a channel, playlist, or video URL first", "error");
            return;
        }
        if (q === "") {
            pushToast("Enter a keyword to filter video titles first", "error");
            return;
        }
        runValidation(source, q, limit);
    }, [sourceUrl, query, limit, runValidation, pushToast]);

    const stopValidation = useCallback(async () => {
        validateGen.current++;
        if (validateDebounce.current) clearTimeout(validateDebounce.current);
        try {
            await invoke("stop_keyword_validation");
        } catch {
            /* nothing was running */
        }
        setValidating(false);
        setValidation(null);
    }, []);

    const canQueue =
        !queuing &&
        !validating &&
        sourceUrl.trim() !== "" &&
        query.trim() !== "" &&
        validation?.can_download === true;

    const addToQueue = useCallback(async () => {
        setQueuing(true);
        try {
            await invoke("start_keyword_download", {
                sourceUrl: sourceUrl.trim(),
                query: query.trim(),
                targetDir: savePath,
                maxHeight: resolution,
                resultCount: limit,
            });
            pushToast("Resolving matches — they'll appear in the queue shortly", "success");
        } catch (err) {
            pushToast(`Keyword search failed: ${String(err)}`, "error");
        } finally {
            setQueuing(false);
        }
    }, [sourceUrl, query, savePath, resolution, limit, pushToast]);

    return (
        <div className="flex flex-col gap-1.5">
            <div>
                <FieldLabel>Source URL</FieldLabel>
                <div className="flex items-center gap-1.5">
                    <div className="relative flex-1 min-w-0">
                        <input
                            type="text"
                            value={sourceUrl}
                            onChange={(e) => setSourceUrl(e.target.value)}
                            placeholder="https://youtube.com/@channel/videos or playlist URL"
                            className="acid-focus w-full bg-[#111] border border-[#242424] rounded-md px-3 py-1.5 pr-7 text-[12px] font-mono text-[#ddd] placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30"
                        />
                        <span className="absolute right-2.5 top-1/2 -translate-y-1/2">
                            {validating ? (
                                <span className="text-[#6699ff]">
                                    <Spinner />
                                </span>
                            ) : validation?.can_download ? (
                                <span className="text-[#c8ff00] text-xs">✓</span>
                            ) : validation && !validation.can_download ? (
                                <span className="text-[#ff4455] text-xs">✕</span>
                            ) : (
                                <span className="text-[#2a2a2a] text-xs">🔗</span>
                            )}
                        </span>
                    </div>
                    <button
                        type="button"
                        onClick={checkNow}
                        title="Check now"
                        disabled={validating || sourceUrl.trim() === "" || query.trim() === ""}
                        className="shrink-0 w-7 h-7 flex items-center justify-center rounded-md border border-[#242424] bg-[#111] text-[11px] text-[#888] transition-all hover:border-[#c8ff00]/40 hover:text-[#c8ff00] disabled:opacity-30 disabled:cursor-not-allowed"
                    >
                        ⟳
                    </button>
                    <button
                        type="button"
                        onClick={stopValidation}
                        title="Stop check"
                        disabled={!validating}
                        className="shrink-0 w-7 h-7 flex items-center justify-center rounded-md border border-[#242424] bg-[#111] text-[11px] text-[#888] transition-all hover:border-[#ff4455]/40 hover:text-[#ff4455] disabled:opacity-30 disabled:cursor-not-allowed"
                    >
                        ⏹
                    </button>
                </div>
                {validation && (
                    <div
                        className={`slide-in mt-1 rounded-md border px-2.5 py-1 ${
                            validation.can_download
                                ? "bg-[#111] border-[#1e1e1e]"
                                : "bg-[#1a0505] border-[#ff4455]/30"
                        }`}
                    >
                        <p className="text-[10px] font-mono text-[#aaa] leading-snug line-clamp-2">
                            {validation.message}
                        </p>
                        {validation.sample_titles.length > 0 && (
                            <p className="mt-0.5 text-[9px] font-mono text-[#666] truncate">
                                {validation.sample_titles.slice(0, 3).join("  ·  ")}
                            </p>
                        )}
                    </div>
                )}
                {validating && scanChecked > 0 && (
                    <p className="mt-1 font-mono text-[9px] text-[#6699ff]">
                        Verified {scanMatched}/{scanChecked}…
                    </p>
                )}
            </div>

            <div>
                <FieldLabel>Search Keyword</FieldLabel>
                <div className="relative">
                    <input
                        type="text"
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                        placeholder="artist name live performance"
                        className="acid-focus w-full bg-[#111] border border-[#242424] rounded-md px-3 py-1.5 pr-7 text-[12px] font-mono text-[#ddd] placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30"
                    />
                    <span className="absolute right-2.5 top-1/2 -translate-y-1/2 text-[#2a2a2a] text-xs">⌕</span>
                </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
                <div>
                    <FieldLabel>Results</FieldLabel>
                    <select
                        value={limit}
                        onChange={(e) => setLimit(Number(e.target.value))}
                        className="acid-focus w-full h-7 bg-[#111] border border-[#242424] rounded-md px-2 text-[10px] font-mono text-[#aaa] focus:border-[#c8ff00]/30 transition-all appearance-none cursor-pointer"
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
                    <FieldLabel>Max Res</FieldLabel>
                    <ResolutionPicker
                        value={resolution}
                        onChange={setResolution}
                        options={KEYWORD_RESOLUTION_OPTIONS}
                    />
                </div>
            </div>

            <button
                disabled={!canQueue}
                onClick={addToQueue}
                className="min-h-8 rounded-lg font-display font-bold text-[11px] tracking-widest uppercase transition-all disabled:opacity-25 disabled:cursor-not-allowed enabled:hover:scale-[1.01] enabled:active:scale-[0.99]"
                style={{
                    background: "linear-gradient(90deg, #3d4f00, #c8ff00 60%, #9cbf00)",
                    color: "#0a0a0a",
                    boxShadow: canQueue ? "0 0 20px #c8ff0033, 0 2px 0 #9cbf00" : "none",
                }}
            >
                {queuing
                    ? "Resolving matches…"
                    : validating
                      ? "Checking URL and keyword matches…"
                      : "⚡ Queue Matching Videos"}
            </button>
        </div>
    );
}
