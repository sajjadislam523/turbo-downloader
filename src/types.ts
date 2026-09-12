export type AppMode = "single" | "batch" | "keyword";

export interface Format {
    id: string;
    label: string;
}

export interface VideoMeta {
    title: string;
    formats: Format[];
}

export interface KeywordValidation {
    valid: boolean;
    source_title: string;
    source_type: string;
    entry_count?: number;
    match_count: number;
    sample_titles: string[];
    message: string;
    can_download: boolean;
}

export interface ToastMessage {
    id: number;
    text: string;
    kind: "error" | "success";
}
