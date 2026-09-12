import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import React, { useCallback, useEffect, useRef, useState } from "react";
import BatchModePanel from "./components/panels/BatchModePanel";
import KeywordModePanel from "./components/panels/KeywordModePanel";
import SingleModePanel from "./components/panels/SingleModePanel";
import QueueList from "./components/QueueList";
import { ModeToggle, Toast } from "./components/ui";
import { QueueProvider } from "./state/QueueContext";
import type { AppMode, ToastMessage } from "./types";

// ─────────────────────────────────────────────────────────────────────────────
// Settings persistence — save path and last-used mode survive an app restart
// via localStorage. Per-mode resolution/limit choices now live inside each
// mode's own panel component instead of here.
// ─────────────────────────────────────────────────────────────────────────────

const SETTINGS_KEY = "turbodl.settings.v1";

interface PersistedSettings {
    mode: AppMode;
    savePath: string;
}

function loadSettings(): Partial<PersistedSettings> {
    try {
        const raw = localStorage.getItem(SETTINGS_KEY);
        return raw ? (JSON.parse(raw) as Partial<PersistedSettings>) : {};
    } catch {
        return {};
    }
}

const savedSettings = loadSettings();

function AppShell(): React.JSX.Element {
    const [mode, setMode] = useState<AppMode>(savedSettings.mode ?? "single");
    const [savePath, setSavePath] = useState<string>(savedSettings.savePath ?? "~/Downloads");
    const [toast, setToast] = useState<ToastMessage | null>(null);
    const [updatingEngine, setUpdatingEngine] = useState<boolean>(false);

    const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

    const pushToast = useCallback((text: string, kind: ToastMessage["kind"] = "error") => {
        setToast({ id: Date.now(), text, kind });
        if (toastTimer.current) clearTimeout(toastTimer.current);
        toastTimer.current = setTimeout(() => setToast(null), 9000);
    }, []);

    const dismissToast = useCallback(() => {
        if (toastTimer.current) clearTimeout(toastTimer.current);
        setToast(null);
    }, []);

    useEffect(() => {
        try {
            const settings: PersistedSettings = { mode, savePath };
            localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
        } catch {
            /* storage unavailable — settings just won't persist this run */
        }
    }, [mode, savePath]);

    // Mirrors the current Save-To folder into the Rust backend so a link
    // arriving via the browser extension's turbodl:// deep link — which has
    // no frontend call of its own to carry this along with it, unlike
    // single/batch/keyword downloads — lands in the same place everything
    // else does instead of a hardcoded default. Runs on mount too, so a
    // deep link arriving right after launch still gets whatever path was
    // restored from localStorage rather than the app's built-in default.
    useEffect(() => {
        invoke("set_default_save_path", { path: savePath }).catch(() => {
            /* backend not ready yet — a later change will still sync */
        });
    }, [savePath]);

    // Surfaces the browser-extension deep-link bridge's outcome — a
    // turbodl://add(-batch) link that Rust already validated and enqueued
    // (or rejected) before either event fires. See handle_incoming_deep_link
    // in src-tauri/src/lib.rs.
    useEffect(() => {
        let alive = true;
        const subs: Array<() => void> = [];
        (async () => {
            subs.push(
                await listen<number>("deep-link-added", (ev) => {
                    if (!alive) return;
                    pushToast(
                        ev.payload === 1
                            ? "Added 1 link from the browser extension"
                            : `Added ${ev.payload} links from the browser extension`,
                        "success",
                    );
                }),
            );
            subs.push(
                await listen<string>("deep-link-rejected", (ev) => {
                    if (!alive) return;
                    pushToast(ev.payload, "error");
                }),
            );
        })();
        return () => {
            alive = false;
            subs.forEach((unsub) => unsub());
        };
    }, [pushToast]);

    const chooseSaveDir = useCallback(async () => {
        try {
            const chosen = await invoke<string>("open_directory_dialog");
            if (chosen) setSavePath(chosen);
        } catch {
            /* cancelled */
        }
    }, []);

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

    const displaySavePath = savePath.length > 44 ? "…" + savePath.slice(-42) : savePath;

    return (
        <div className="relative w-full h-full bg-[#0a0a0a] flex flex-col overflow-hidden scanlines">
            <div
                className="pointer-events-none absolute inset-0 opacity-[0.03]"
                style={{
                    backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")`,
                    backgroundSize: "128px 128px",
                }}
            />

            <Toast toast={toast} onDismiss={dismissToast} />

            {/* Compact header: title + save-to + update-engine all on one row so the
                rest of the window height goes to the mode panel and the queue. */}
            <header className="flex items-center justify-between gap-2 px-4 pt-2.5 pb-2 shrink-0">
                <h1 className="font-display text-[15px] font-extrabold tracking-tight leading-none text-white shrink-0">
                    TURBO<span className="text-[#c8ff00]">DL</span>
                </h1>

                <div
                    onClick={chooseSaveDir}
                    title="Change save folder"
                    className="flex-1 min-w-0 flex items-center gap-1.5 bg-[#111] border border-[#1e1e1e] rounded-md px-2.5 py-1 cursor-pointer hover:border-[#3a3a3a] transition-colors group"
                >
                    <span className="text-[#c8ff00] text-[10px] shrink-0">📁</span>
                    <span className="font-mono text-[10px] text-[#555] group-hover:text-[#888] transition-colors truncate">
                        {displaySavePath}
                    </span>
                </div>

                <button
                    onClick={updateEngine}
                    disabled={updatingEngine}
                    title="Update yt-dlp"
                    className="shrink-0 flex items-center justify-center w-7 h-7 bg-[#111] border border-[#1e1e1e] hover:border-[#333] rounded-md transition-colors disabled:opacity-50"
                >
                    <span className={`text-[#888] text-[11px] ${updatingEngine ? "animate-spin" : ""}`}>⟳</span>
                </button>
            </header>

            <main className="flex-1 min-h-0 flex flex-col gap-2 px-4 pb-2.5 overflow-hidden">
                <ModeToggle mode={mode} onChange={setMode} />

                {/* All three panels stay mounted so switching tabs never loses what
                    you've typed or the state of an in-flight keyword search/validation
                    — only the active one is visible. Conditionally mounting/unmounting
                    them here used to reset every field (including the Results count)
                    back to its default the moment you switched away and back. */}
                <div className="shrink-0" hidden={mode !== "single"}>
                    <SingleModePanel savePath={savePath} pushToast={pushToast} />
                </div>
                <div className="shrink-0" hidden={mode !== "batch"}>
                    <BatchModePanel savePath={savePath} pushToast={pushToast} />
                </div>
                <div className="shrink-0" hidden={mode !== "keyword"}>
                    <KeywordModePanel savePath={savePath} pushToast={pushToast} />
                </div>

                <QueueList />
            </main>
        </div>
    );
}

export default function App(): React.JSX.Element {
    return (
        <QueueProvider>
            <AppShell />
        </QueueProvider>
    );
}
