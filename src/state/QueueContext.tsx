import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React, {
    createContext,
    useCallback,
    useContext,
    useEffect,
    useReducer,
} from "react";

// ─────────────────────────────────────────────────────────────────────────────
// Mirrors src-tauri/src/queue.rs's QueueItem/QueueItemStatus/QueueSource —
// field names here are plain serde output (snake_case), not run through
// Tauri's camelCase-arg conversion (that conversion only applies to command
// *arguments*, not to data returned from a command or carried by an event).
// ─────────────────────────────────────────────────────────────────────────────

export type QueueItemStatus =
    | "pending"
    | "running"
    | "paused"
    | "completed"
    | "failed"
    | "cancelled";

export type QueueSource = "single" | "batch" | "keyword" | "extension";

export interface QueueItem {
    id: string;
    url: string;
    source: QueueSource;
    status: QueueItemStatus;
    format_id?: string | null;
    resolution?: number | null;
    save_path: string;
    title?: string | null;
    error?: string | null;
    host: string;
}

export interface ItemProgress {
    percent: number;
    speed: string;
    eta: string;
    size: string;
    lastLine: string;
}

const EMPTY_PROGRESS: ItemProgress = {
    percent: 0,
    speed: "--",
    eta: "--",
    size: "--",
    lastLine: "",
};

// Same parsing rules as the old top-level parseProgressLine in App.tsx, minus
// the batch-item-counter match — that concept doesn't apply once each queued
// video is already its own row instead of one shared "file X of Y" counter.
function parseProgressLine(raw: string) {
    const line = raw.replace(/\x1b\[[0-9;]*m/g, "");
    const result: { percent?: number; speed?: string; eta?: string; size?: string } = {};

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

interface QueueReducerState {
    items: QueueItem[];
    progress: Record<string, ItemProgress>;
    maxConcurrency: number;
}

type Action =
    | { type: "snapshot"; items: QueueItem[] }
    | { type: "added"; item: QueueItem }
    | { type: "status"; id: string; status: QueueItemStatus; error?: string | null }
    | { type: "progress"; id: string; line: string }
    | { type: "removed"; id: string }
    | { type: "maxConcurrency"; value: number }
    | { type: "clearTerminal" };

function reducer(state: QueueReducerState, action: Action): QueueReducerState {
    switch (action.type) {
        case "snapshot":
            return { ...state, items: action.items };
        case "added":
            // Guard against a duplicate add (e.g. a snapshot fetched after
            // the item was already appended via its own event).
            if (state.items.some((it) => it.id === action.item.id)) return state;
            return { ...state, items: [...state.items, action.item] };
        case "status":
            return {
                ...state,
                items: state.items.map((it) =>
                    it.id === action.id
                        ? { ...it, status: action.status, error: action.error ?? null }
                        : it,
                ),
            };
        case "progress": {
            const parsed = parseProgressLine(action.line);
            const prev = state.progress[action.id] ?? EMPTY_PROGRESS;
            const cleanLine = action.line.replace(/\x1b\[[0-9;]*m/g, "").trim();
            return {
                ...state,
                progress: {
                    ...state.progress,
                    [action.id]: {
                        percent: parsed.percent ?? prev.percent,
                        speed: parsed.speed ?? prev.speed,
                        eta: parsed.eta ?? prev.eta,
                        size: parsed.size ?? prev.size,
                        lastLine: cleanLine || prev.lastLine,
                    },
                },
            };
        }
        case "removed": {
            const progress = { ...state.progress };
            delete progress[action.id];
            return {
                ...state,
                items: state.items.filter((it) => it.id !== action.id),
                progress,
            };
        }
        case "maxConcurrency":
            return { ...state, maxConcurrency: action.value };
        case "clearTerminal":
            return {
                ...state,
                items: state.items.filter(
                    (it) => it.status === "pending" || it.status === "running" || it.status === "paused",
                ),
            };
        default:
            return state;
    }
}

interface QueueContextValue extends QueueReducerState {
    enqueueDownload: (url: string, formatId: string, savePath: string) => Promise<string>;
    enqueueUrls: (
        urls: string[],
        savePath: string,
        resolution: number,
        source: QueueSource,
    ) => Promise<string[]>;
    enqueueBatchFile: (filePath: string, savePath: string, resolution: number) => Promise<string[]>;
    cancelItem: (id: string) => Promise<void>;
    cancelAll: () => Promise<void>;
    pauseItem: (id: string) => Promise<void>;
    resumeItem: (id: string) => Promise<void>;
    retryItem: (id: string) => Promise<void>;
    setMaxConcurrency: (value: number) => Promise<number>;
    clearCompleted: () => Promise<void>;
    removeItem: (id: string) => Promise<void>;
}

const QueueCtx = createContext<QueueContextValue | null>(null);

export function QueueProvider({ children }: { children: React.ReactNode }) {
    const [state, dispatch] = useReducer(reducer, {
        items: [],
        progress: {},
        maxConcurrency: 3,
    });

    useEffect(() => {
        let alive = true;
        const subs: Array<() => void> = [];

        (async () => {
            try {
                const snapshot = await invoke<QueueItem[]>("get_queue_snapshot");
                if (alive) dispatch({ type: "snapshot", items: snapshot });
            } catch {
                /* backend not ready yet — the live events below will still populate it */
            }

            subs.push(
                await listen<QueueItem>("queue-item-added", (ev) =>
                    dispatch({ type: "added", item: ev.payload }),
                ),
            );
            subs.push(
                await listen<{ id: string; status: QueueItemStatus; error: string | null }>(
                    "queue-item-status-changed",
                    (ev) =>
                        dispatch({
                            type: "status",
                            id: ev.payload.id,
                            status: ev.payload.status,
                            error: ev.payload.error,
                        }),
                ),
            );
            subs.push(
                await listen<{ id: string; line: string }>("queue-item-progress", (ev) =>
                    dispatch({ type: "progress", id: ev.payload.id, line: ev.payload.line }),
                ),
            );
        })();

        return () => {
            alive = false;
            subs.forEach((unsub) => unsub());
        };
    }, []);

    const enqueueDownload = useCallback(
        (url: string, formatId: string, savePath: string) =>
            invoke<string>("enqueue_download", { url, formatId, savePath }),
        [],
    );

    const enqueueUrls = useCallback(
        (urls: string[], savePath: string, resolution: number, source: QueueSource) =>
            invoke<string[]>("enqueue_urls", { urls, savePath, resolution, source }),
        [],
    );

    const enqueueBatchFile = useCallback(
        (filePath: string, savePath: string, resolution: number) =>
            invoke<string[]>("enqueue_batch_file", { filePath, savePath, resolution }),
        [],
    );

    const cancelItem = useCallback(
        (id: string) => invoke<string>("cancel_queue_item", { id }).then(() => undefined),
        [],
    );

    const cancelAll = useCallback(
        () => invoke<string>("cancel_all_queue_items").then(() => undefined),
        [],
    );

    const pauseItem = useCallback(
        (id: string) => invoke<string>("pause_queue_item", { id }).then(() => undefined),
        [],
    );

    const resumeItem = useCallback(
        (id: string) => invoke<string>("resume_queue_item", { id }).then(() => undefined),
        [],
    );

    const retryItem = useCallback(
        (id: string) => invoke<string>("retry_queue_item", { id }).then(() => undefined),
        [],
    );

    const setMaxConcurrency = useCallback(async (value: number) => {
        const applied = await invoke<number>("set_max_concurrency", { value });
        dispatch({ type: "maxConcurrency", value: applied });
        return applied;
    }, []);

    const clearCompleted = useCallback(async () => {
        await invoke<number>("clear_completed");
        dispatch({ type: "clearTerminal" });
    }, []);

    const removeItem = useCallback(async (id: string) => {
        await invoke<string>("remove_completed_item", { id });
        dispatch({ type: "removed", id });
    }, []);

    const value: QueueContextValue = {
        ...state,
        enqueueDownload,
        enqueueUrls,
        enqueueBatchFile,
        cancelItem,
        cancelAll,
        pauseItem,
        resumeItem,
        retryItem,
        setMaxConcurrency,
        clearCompleted,
        removeItem,
    };

    return <QueueCtx.Provider value={value}>{children}</QueueCtx.Provider>;
}

export function useQueue(): QueueContextValue {
    const ctx = useContext(QueueCtx);
    if (!ctx) throw new Error("useQueue must be used within a QueueProvider");
    return ctx;
}
