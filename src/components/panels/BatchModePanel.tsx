import { invoke } from "@tauri-apps/api/core";
import { useCallback, useState } from "react";
import { useQueue } from "../../state/QueueContext";
import type { ToastMessage } from "../../types";
import { FieldLabel, ResolutionPicker } from "../ui";

type InputMode = "file" | "paste";

export default function BatchModePanel({
    savePath,
    pushToast,
}: {
    savePath: string;
    pushToast: (text: string, kind: ToastMessage["kind"]) => void;
}) {
    const { enqueueUrls, enqueueBatchFile } = useQueue();

    const [inputMode, setInputMode] = useState<InputMode>("file");
    const [batchFile, setBatchFile] = useState<string>("");
    const [pasted, setPasted] = useState<string>("");
    const [resolution, setResolution] = useState<number>(0);
    const [busy, setBusy] = useState<boolean>(false);

    const chooseBatchFile = useCallback(async () => {
        try {
            const chosen = await invoke<string>("open_file_dialog");
            if (chosen) setBatchFile(chosen);
        } catch {
            /* cancelled */
        }
    }, []);

    const pastedLines = pasted
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter((l) => l && !l.startsWith("#"));

    const canAdd =
        !busy && (inputMode === "file" ? batchFile !== "" : pastedLines.length > 0);

    const addToQueue = useCallback(async () => {
        setBusy(true);
        try {
            if (inputMode === "file") {
                const ids = await enqueueBatchFile(batchFile, savePath, resolution);
                pushToast(`Queued ${ids.length} URL(s) from file`, "success");
            } else {
                const ids = await enqueueUrls(pastedLines, savePath, resolution, "batch");
                pushToast(`Queued ${ids.length} URL(s)`, "success");
                setPasted("");
            }
        } catch (err) {
            pushToast(`Could not queue batch: ${String(err)}`, "error");
        } finally {
            setBusy(false);
        }
    }, [inputMode, batchFile, pastedLines, resolution, savePath, enqueueBatchFile, enqueueUrls, pushToast]);

    const displayBatchFile = batchFile ? (batchFile.split("/").pop() ?? batchFile) : "No file selected";

    return (
        <div className="flex flex-col gap-1.5">
            <div className="flex gap-0.5 bg-[#111] border border-[#1e1e1e] rounded-md p-0.5 w-fit">
                {(["file", "paste"] as InputMode[]).map((m) => (
                    <button
                        key={m}
                        onClick={() => setInputMode(m)}
                        className={`px-2.5 py-0.5 rounded text-[9px] font-mono font-medium tracking-wider uppercase transition-all ${
                            inputMode === m
                                ? "bg-[#c8ff00] text-[#0a0a0a]"
                                : "text-[#555] hover:text-[#888]"
                        }`}
                    >
                        {m === "file" ? "From File" : "Paste URLs"}
                    </button>
                ))}
            </div>

            {inputMode === "file" ? (
                <div>
                    <div className="flex items-stretch gap-1.5">
                        <div className="flex-1 bg-[#111] border border-[#242424] rounded-md px-2.5 py-1.5 flex items-center gap-2 min-w-0">
                            <span className="text-[#c8ff00] text-[11px] shrink-0">📄</span>
                            <span className="font-mono text-[11px] text-[#666] truncate">
                                {displayBatchFile}
                            </span>
                        </div>
                        <button
                            onClick={chooseBatchFile}
                            className="shrink-0 bg-[#161616] border border-[#2a2a2a] hover:border-[#444] text-[#888] hover:text-[#ccc] rounded-md px-3 text-[10px] font-mono tracking-wider uppercase transition-all"
                        >
                            Browse
                        </button>
                    </div>
                    <p className="mt-1 font-mono text-[8px] text-[#333] tracking-wide">
                        .txt file · one URL per line · # lines skipped
                    </p>
                </div>
            ) : (
                <div>
                    <FieldLabel>{pastedLines.length} valid link(s) detected</FieldLabel>
                    <textarea
                        value={pasted}
                        onChange={(e) => setPasted(e.target.value)}
                        placeholder={"https://example.com/video1\nhttps://example.com/video2\n# comment lines are skipped"}
                        rows={4}
                        className="acid-focus w-full bg-[#111] border border-[#242424] rounded-md px-3 py-1.5 text-[12px] font-mono text-[#ddd] placeholder-[#2a2a2a] transition-all focus:border-[#c8ff00]/30 resize-y"
                    />
                </div>
            )}

            <div className="flex items-center gap-2">
                <span className="font-mono text-[9px] uppercase tracking-[0.15em] text-[#444] shrink-0">
                    Max Res
                </span>
                <ResolutionPicker value={resolution} onChange={setResolution} />
            </div>

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
                ⚡ Add Batch to Queue
            </button>
        </div>
    );
}
