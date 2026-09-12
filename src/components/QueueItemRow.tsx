import type { ReactNode } from "react";
import { ItemProgress, QueueItem, useQueue } from "../state/QueueContext";
import { ProgressBar, Spinner, StatusDot, truncate } from "./ui";

const STATUS_COLOR: Record<QueueItem["status"], string> = {
    pending: "#555",
    running: "#c8ff00",
    paused: "#6699ff",
    completed: "#00ff99",
    failed: "#ff4455",
    cancelled: "#666",
};

const SOURCE_ICON: Record<QueueItem["source"], string> = {
    single: "⚡",
    batch: "📄",
    keyword: "⌕",
    extension: "🧩",
};

function displayName(item: QueueItem): string {
    return item.title || item.url;
}

function IconButton({
    onClick,
    title,
    color,
    hoverColor,
    children,
}: {
    onClick: () => void;
    title: string;
    color: string;
    hoverColor: string;
    children: ReactNode;
}) {
    return (
        <button
            onClick={onClick}
            title={title}
            aria-label={title}
            className="shrink-0 w-5 h-5 flex items-center justify-center rounded text-[10px] leading-none transition-colors"
            style={{ color }}
            onMouseEnter={(e) => (e.currentTarget.style.color = hoverColor)}
            onMouseLeave={(e) => (e.currentTarget.style.color = color)}
        >
            {children}
        </button>
    );
}

export default function QueueItemRow({
    item,
    progress,
}: {
    item: QueueItem;
    progress: ItemProgress | undefined;
}) {
    const { cancelItem, removeItem, pauseItem, resumeItem } = useQueue();
    const p = progress ?? { percent: 0, speed: "--", eta: "--", size: "--", lastLine: "" };
    const isRunning = item.status === "running";
    const isPending = item.status === "pending";
    const isPaused = item.status === "paused";
    const isTerminal =
        item.status === "completed" || item.status === "failed" || item.status === "cancelled";

    return (
        <div className="bg-[#111] border border-[#1e1e1e] rounded-md px-2.5 py-1.5">
            <div className="flex items-center justify-between gap-1.5">
                <div className="min-w-0 flex items-center gap-1.5">
                    <StatusDot active={isRunning} />
                    <span className="text-[10px] shrink-0" title={item.source}>
                        {SOURCE_ICON[item.source]}
                    </span>
                    <span className="text-[10px] text-[#ccc] font-body truncate">
                        {truncate(displayName(item), 56)}
                    </span>
                </div>
                <div className="flex items-center gap-1 shrink-0">
                    <span
                        className="font-mono text-[8px] tracking-widest uppercase mr-0.5"
                        style={{ color: STATUS_COLOR[item.status] }}
                    >
                        {isRunning ? <Spinner /> : item.status}
                    </span>
                    {isRunning && (
                        <IconButton onClick={() => pauseItem(item.id)} title="Pause" color="#6699ff" hoverColor="#88aaff">
                            ⏸
                        </IconButton>
                    )}
                    {isPaused && (
                        <IconButton onClick={() => resumeItem(item.id)} title="Resume" color="#c8ff00" hoverColor="#ddff55">
                            ▶
                        </IconButton>
                    )}
                    {(isRunning || isPending || isPaused) && (
                        <IconButton onClick={() => cancelItem(item.id)} title="Cancel" color="#ff4455" hoverColor="#ff7788">
                            ⏹
                        </IconButton>
                    )}
                    {isTerminal && (
                        <IconButton onClick={() => removeItem(item.id)} title="Remove" color="#555" hoverColor="#888">
                            ✕
                        </IconButton>
                    )}
                </div>
            </div>

            {(isRunning || isPaused) && (
                <div className="mt-1">
                    <ProgressBar percent={p.percent} />
                    <div className="flex items-center justify-between text-[9px] font-mono text-[#555] mt-0.5">
                        <span>
                            <span className="text-[#666]">SPD </span>
                            <span className="text-[#c8ff00]">{p.speed}</span>
                        </span>
                        <span>
                            <span className="text-[#666]">ETA </span>
                            <span className="text-[#aaa]">{p.eta}</span>
                        </span>
                        <span>
                            <span className="text-[#666]">SIZE </span>
                            <span className="text-[#aaa]">{p.size}</span>
                        </span>
                        <span className="text-[#c8ff00]">{p.percent.toFixed(1)}%</span>
                    </div>
                </div>
            )}

            {item.status === "failed" && item.error && (
                <p className="mt-1 text-[9px] font-mono text-[#ff4455]/80 truncate">{item.error}</p>
            )}
        </div>
    );
}
