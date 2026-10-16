import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { useAppStore } from "./store";
import { MODULES } from "./modules";
import ErrorBoundary from "./components/ErrorBoundary";
import { HardDriveDownload } from "lucide-react";

export default function App() {
  const { active, setActive, theme } = useAppStore();
  // 首次激活才渲染（懒加载：未访问的模块不执行 useEffect），
  // 访问过则常驻挂载（保留扫描结果，切走再切回不丢状态）
  const [visited, setVisited] = useState<Set<string>>(() => new Set([active]));

  // 就绪信号：通知 Rust 前端已挂载（Rust 侧记录日志；窗口已默认
  // 可见，Rust 的 show 为幂等 no-op）。启动页已移除（用户指定）。
  useEffect(() => {
    invoke("window_ready").catch(() => {});
  }, []);

  useEffect(() => {
    setVisited((v) => {
      if (v.has(active)) return v;
      const n = new Set(v);
      n.add(active);
      return n;
    });
  }, [active]);

  return (
    <div
      className="flex h-screen w-screen overflow-hidden bg-[var(--color-bg)] text-[var(--color-text)]"
      data-theme={theme}
      style={{ position: "relative" }}
    >
      {/* 左侧导航 · 玻璃栏 */}
      <nav className="relative z-10 flex w-56 shrink-0 flex-col border-r border-[var(--color-line)] bg-[var(--color-bg-2)] px-3 py-4">
        {/* Logo · CDS 陶土品牌区 */}
        <div className="mb-6 flex items-center gap-3 px-2 pt-1">
          <div
            className="flex h-10 w-10 shrink-0 items-center justify-center rounded-[12px]"
            style={{
              background: "linear-gradient(140deg, #e08264 0%, var(--color-primary) 45%, var(--color-primary-strong) 100%)",
              boxShadow: "0 2px 10px rgba(217, 119, 87, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.35), inset 0 0 0 1px rgba(255, 255, 255, 0.12)",
              color: "#ffffff",
            }}
          >
            <HardDriveDownload size={20} strokeWidth={2} />
          </div>
          <div className="leading-tight">
            <div className="font-display text-[15px] font-bold" style={{ letterSpacing: "-0.02em" }}>
              CDisk <span style={{ color: "var(--color-primary)" }}>Savior</span>
            </div>
            <div className="mt-0.5 text-[11px]" style={{ color: "var(--color-text-dim)", letterSpacing: "0.04em" }}>
              磁盘与内存维护助手
            </div>
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
          v0.1.0 · C 盘救星
        </div>
      </nav>

      {/* 主区域 · 首次激活才渲染、之后常驻挂载；display 切换 + 淡入淡出 */}
      <main
        className="relative z-10 flex-1 overflow-y-auto [scrollbar-gutter:stable]"
      >
        {MODULES.map((m) => {
          const show = active === m.id;
          const mounted = visited.has(m.id);
          return (
            <motion.div
              key={m.id}
              className={show ? "block h-full" : "hidden"}
              initial={false}
              animate={{ opacity: show ? 1 : 0 }}
              transition={{ duration: 0.13, ease: "easeOut" }}
            >
              {mounted ? (
                <ErrorBoundary>
                  <m.page />
                </ErrorBoundary>
              ) : null}
            </motion.div>
          );
        })}
      </main>
    </div>
  );
}
