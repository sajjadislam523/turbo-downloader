import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
    children: ReactNode;
}

interface State {
    hasError: boolean;
    error: Error | null;
}

export default class ErrorBoundary extends Component<Props, State> {
    constructor(props: Props) {
        super(props);
        this.state = { hasError: false, error: null };
    }

    static getDerivedStateFromError(error: Error): State {
        return { hasError: true, error };
    }

    componentDidCatch(error: Error, info: ErrorInfo) {
        console.error("[ErrorBoundary]", error, info.componentStack);
    }

    handleRestart = () => {
        this.setState({ hasError: false, error: null });
    };

    render() {
        if (this.state.hasError) {
            return (
                <div className="relative w-full h-full bg-[#0a0a0a] flex flex-col items-center justify-center gap-4 px-6">
                    <div className="pointer-events-none absolute inset-0 opacity-[0.03]"
                        style={{
                            backgroundImage: `url("data:image/svg+xml,%3Csvg viewBox='0 0 256 256' xmlns='http://www.w3.org/2000/svg'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='4' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E")`,
                            backgroundSize: "128px 128px",
                        }}
                    />
                    <span className="text-[#ff4455] text-3xl">⚠</span>
                    <h1 className="font-display text-[20px] font-extrabold tracking-tight text-white">
                        Something went wrong
                    </h1>
                    <p className="font-mono text-[11px] text-[#555] text-center max-w-md leading-relaxed">
                        {this.state.error?.message || "An unexpected error occurred."}
                    </p>
                    <button
                        onClick={this.handleRestart}
                        className="mt-2 bg-[#c8ff00] text-[#0a0a0a] rounded-xl px-6 py-2.5 font-display font-bold text-[12px] tracking-widest uppercase hover:scale-[1.02] active:scale-[0.98] transition-all"
                    >
                        Restart
                    </button>
                </div>
            );
        }

        return this.props.children;
    }
}
