import { Component, type ReactNode, type ErrorInfo } from "react";
import { AlertTriangle, RefreshCw } from "lucide-react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
  error?: Error;
}

/** 全局错误边界：单页面崩溃时显示错误卡片，不白屏 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("页面错误:", error, info);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-full items-center justify-center p-10">
          <div className="max-w-lg rounded-2xl border border-[var(--color-danger)]/30 bg-[var(--color-surface)] p-6 text-center">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-xl bg-[var(--color-danger-bg-soft)]">
              <AlertTriangle size={28} className="text-[var(--color-danger)]" />
            </div>
            <h2 className="mb-2 text-lg font-bold">页面出现错误</h2>
            <p className="mb-4 text-sm text-[var(--color-text-secondary)]">
              {this.state.error?.message ?? "未知错误"}
            </p>
            <button
              onClick={() => this.setState({ hasError: false, error: undefined })}
              className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white"
            >
              <RefreshCw size={14} />
              重试
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
