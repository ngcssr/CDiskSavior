import { useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppStore } from "./store";
import { MODULES } from "./modules";
import ErrorBoundary from "./components/ErrorBoundary";
import { HardDriveDownload } from "lucide-react";

export default function App() {
  const { active, setActive, theme } = useAppStore();

  // 就绪信号：通知 Rust 前端已挂载（Rust 侧记录日志；窗口已默认
  // 可见，Rust 的 show 为幂等 no-op）。启动页已移除（用户指定）。
  useEffect(() => {
    invoke("window_ready").catch(() => {});
  }, []);

  return (
    <div
      className="flex h-screen w-screen overflow-hidden bg-[var(--color-bg)] text-[var(--color-text)]"
      data-theme={theme}
      style={{ position: "relative" }}
    >
      {/* 左侧导航 · 玻璃栏 */}
      <nav className="relative z-10 flex w-56 shrink-0 flex-col border-r border-[var(--color-line)] bg-[var(--color-bg-2)] px-3 py-4">
        {/* Logo */}
        <div className="mb-5 flex items-center gap-2.5 px-2">
          <div
            className="flex h-9 w-9 items-center justify-center rounded-xl"
            style={{
              background: "linear-gradient(135deg, var(--color-primary), var(--color-primary-strong))",
              boxShadow: "0 0 18px var(--color-primary-glow)",
              color: "#06222a",
            }}
          >
            <HardDriveDownload size={19} strokeWidth={2.2} />
          </div>
          <div className="leading-tight">
            <div className="font-display text-[14px] font-bold tracking-tight">CDisk Savior</div>
            <div className="text-[11px] text-[var(--color-text-dim)]">磁盘与内存维护助手</div>
          </div>
        </div>

        {/* 模块导航 */}
        <div className="flex flex-1 flex-col gap-0.5">
          {MODULES.map((m) => {
            const Icon = m.icon;
            const isActive = active === m.id;
            return (
              <button
                key={m.id}
                onClick={() => setActive(m.id)}
                className="group relative flex items-center gap-2.5 rounded-lg px-3 py-2 text-left text-[13px] transition-all"
                style={{
                  background: isActive ? "var(--color-primary-soft)" : "transparent",
                  color: isActive ? "var(--color-primary)" : "var(--color-text-secondary)",
                  fontWeight: isActive ? 600 : 500,
                }}
                onMouseEnter={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "var(--color-surface-hover)";
                    e.currentTarget.style.color = "var(--color-text)";
                  }
                }}
                onMouseLeave={(e) => {
                  if (!isActive) {
                    e.currentTarget.style.background = "transparent";
                    e.currentTarget.style.color = "var(--color-text-secondary)";
                  }
                }}
              >
                {isActive && (
                  <span
                    className="absolute left-0 top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-full"
                    style={{ background: "var(--color-primary)", boxShadow: "0 0 8px var(--color-primary-glow)" }}
                  />
                )}
                <Icon
                  size={16}
                  strokeWidth={isActive ? 2.2 : 1.8}
                  style={{ color: isActive ? "var(--color-primary)" : "inherit" }}
                />
                {m.label}
              </button>
            );
          })}
        </div>

        {/* 底部版本 */}
        <div className="border-t border-[var(--color-line)] px-2 pt-3 text-[11px] text-[var(--color-text-dim)]">
          v0.1.0 · Refined Industrial
        </div>
      </nav>

      {/* 主区域 */}
      <main className="relative z-10 flex-1 overflow-y-auto">
        {MODULES.map((m) => (
          <div key={m.id} className={active === m.id ? "block h-full" : "hidden"}>
            <ErrorBoundary>
              <m.page />
            </ErrorBoundary>
          </div>
        ))}
      </main>
    </div>
  );
}
