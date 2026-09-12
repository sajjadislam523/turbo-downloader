import { useMemo } from "react";
import { useQueue } from "../state/QueueContext";
import QueueItemRow from "./QueueItemRow";

const CONCURRENCY_OPTIONS = [1, 2, 3, 4, 5, 6, 8];

export default function QueueList() {
    const { items, progress, maxConcurrency, setMaxConcurrency, cancelAll, clearCompleted } =
        useQueue();

    const counts = useMemo(() => {
        const c = { pending: 0, running: 0, paused: 0, completed: 0, failed: 0, cancelled: 0 };
        for (const it of items) c[it.status]++;
        return c;
    }, [items]);

    const hasActive = counts.pending > 0 || counts.running > 0 || counts.paused > 0;
    const hasTerminal = counts.completed > 0 || counts.failed > 0 || counts.cancelled > 0;

    // Newest-first within each bucket isn't necessary — insertion order (the
    // order items were enqueued in) is what the dispatcher itself admits in,
    // so keeping that order here is what lets the row list double as "what
    // will start next."
    const sorted = useMemo(
        () =>
            [...items].sort((a, b) => {
                const rank = (s: string) =>
                    s === "running" ? 0 : s === "pending" ? 1 : s === "paused" ? 2 : 3;
                return rank(a.status) - rank(b.status);
            }),
        [items],
    );

    return (
        <div className="flex-1 min-h-0 flex flex-col border-t border-[#161616] pt-1.5">
            <div className="flex items-center justify-between mb-1 gap-2 flex-wrap">
                <span className="font-mono text-[9px] font-medium tracking-[0.15em] uppercase text-[#555]">
                    Queue · {counts.running}▶ {counts.pending}⏳
                    {counts.paused > 0 ? ` ${counts.paused}⏸` : ""}
                    {hasTerminal ? ` — ${counts.completed} done, ${counts.failed} failed` : ""}
                </span>
                <div className="flex items-center gap-1.5 shrink-0">
                    <span className="font-mono text-[8px] uppercase tracking-widest text-[#444]">Max</span>
                    <select
                        value={maxConcurrency}
                        onChange={(e) => setMaxConcurrency(Number(e.target.value))}
                        title="Max concurrent downloads"
                        className="acid-focus h-5 bg-[#111] border border-[#242424] rounded px-1 text-[9px] font-mono text-[#aaa] focus:border-[#c8ff00]/30 transition-all appearance-none cursor-pointer"
                        style={{ backgroundImage: "none" }}
                    >
                        {CONCURRENCY_OPTIONS.map((n) => (
                            <option key={n} value={n}>
                                {n}
                            </option>
                        ))}
                    </select>
                    <button
                        onClick={() => cancelAll()}
                        disabled={!hasActive}
                        title="Cancel all"
                        className="font-mono text-[9px] tracking-widest uppercase text-[#ff4455] hover:text-[#ff7788] disabled:opacity-30 disabled:hover:text-[#ff4455] transition-colors"
                    >
                        Cancel All
                    </button>
                    <button
                        onClick={() => clearCompleted()}
                        disabled={!hasTerminal}
                        title="Clear completed"
                        className="font-mono text-[9px] tracking-widest uppercase text-[#555] hover:text-[#888] disabled:opacity-30 disabled:hover:text-[#555] transition-colors"
                    >
                        Clear
                    </button>
                </div>
            </div>

            <div className="flex-1 min-h-0 overflow-y-auto flex flex-col gap-1 pr-0.5">
                {sorted.length === 0 ? (
                    <div className="h-10 flex items-center justify-center border border-dashed border-[#1e1e1e] rounded-md">
                        <span className="font-mono text-[9px] text-[#333] tracking-wide">
                            Queue is empty — add a download above
                        </span>
                    </div>
                ) : (
                    sorted.map((item) => (
                        <QueueItemRow key={item.id} item={item} progress={progress[item.id]} />
                    ))
                )}
            </div>
        </div>
    );
}
