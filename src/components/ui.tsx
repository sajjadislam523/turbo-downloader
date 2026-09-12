import React from "react";
import type { AppMode, ToastMessage } from "../types";

export function StatusDot({ active }: { active: boolean }) {
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

export function FieldLabel({
    children,
    className = "",
}: {
    children: React.ReactNode;
    className?: string;
}) {
    return (
        <span
            className={`block text-[9px] font-mono font-medium tracking-[0.15em] uppercase text-[#555] mb-1 ${className}`}
        >
            {children}
        </span>
    );
}

export function ProgressBar({ percent }: { percent: number }) {
    return (
        <div className="progress-track">
            <div
                className="progress-fill"
                style={{ width: `${Math.min(percent, 100)}%` }}
            />
        </div>
    );
}

export function Spinner() {
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

export function Toast({
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

export function ModeToggle({
    mode,
    onChange,
}: {
    mode: AppMode;
    onChange: (m: AppMode) => void;
}) {
    const tabs: { key: AppMode; label: string; icon: string }[] = [
        { key: "single", label: "Single", icon: "⚡" },
        { key: "batch", label: "Batch", icon: "📄" },
        { key: "keyword", label: "Keyword", icon: "⌕" },
    ];

    return (
        <div className="shrink-0 flex gap-0.5 bg-[#111] border border-[#1e1e1e] rounded-md p-0.5">
            {tabs.map((t) => (
                <button
                    key={t.key}
                    onClick={() => onChange(t.key)}
                    className={`flex-1 flex items-center justify-center gap-1 py-1 rounded text-[10px] font-mono font-medium tracking-wider uppercase transition-all ${
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

export const RESOLUTION_OPTIONS: { label: string; value: number }[] = [
    { label: "Best", value: 0 },
    { label: "4K", value: 2160 },
    { label: "1440", value: 1440 },
    { label: "1080", value: 1080 },
    { label: "720", value: 720 },
    { label: "480", value: 480 },
    { label: "360", value: 360 },
];

export function ResolutionPicker({
    value,
    onChange,
    options = RESOLUTION_OPTIONS,
}: {
    value: number;
    onChange: (v: number) => void;
    options?: { label: string; value: number }[];
}) {
    return (
        <div className="grid grid-cols-4 sm:flex gap-1 sm:flex-wrap">
            {options.map((opt) => (
                <button
                    key={opt.value}
                    onClick={() => onChange(opt.value)}
                    className={`px-2 py-0.5 rounded font-mono text-[9px] tracking-wider uppercase transition-all border ${
                        value === opt.value
                            ? "bg-[#c8ff00] text-[#0a0a0a] border-[#c8ff00] shadow-[0_0_8px_#c8ff0033]"
                            : "bg-transparent text-[#555] border-[#2a2a2a] hover:border-[#444] hover:text-[#888]"
                    }`}
                >
                    {opt.label}
                </button>
            ))}
        </div>
    );
}

export function truncate(s: string, n: number): string {
    return s.length > n ? s.slice(0, n) + "…" : s;
}
