import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion } from "framer-motion";
import { useAppStore } from "./store";
import { MODULES } from "./modules";
import ErrorBoundary from "./components/ErrorBoundary";
import { api_settings, type UpdateInfo } from "./api/commands";
import { HardDriveDownload, Download } from "lucide-react";

export default function App() {
  const { active, setActive, theme } = useAppStore();
  // 首次激活才渲染（懒加载：未访问的模块不执行 useEffect），
  // 访问过则常驻挂载（保留扫描结果，切走再切回不丢状态）
  const [visited, setVisited] = useState<Set<string>>(() => new Set([active]));
  // 普通更新（弹窗：是/否/跳过此版本）
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  // 强制更新拦截（本地版本低于 minimum_version 时全屏阻止使用）
  const [forceUpdate, setForceUpdate] = useState<UpdateInfo | null>(null);
  // 下载状态
  const [downloading, setDownloading] = useState(false);
  const [downloadPct, setDownloadPct] = useState(0);

  // 检查更新：去重（同一版本只提示一次）+ 跳过版本 + force 拦截
  const checkUpdate = useCallback(async () => {
    try {
      const info = await api_settings.check_update();
      if (!info.has_update) return;
      const skipped = localStorage.getItem("skip_version") === String(info.latest_version);
      if (info.force) {
        setForceUpdate(info);
      } else if (!skipped) {
        setUpdateInfo(info);
      }
    } catch {}
  }, []);

  // 下载并应用更新（优先自动下载，无 download_url 则引导手动下载）
  const doUpdate = useCallback(async (info: UpdateInfo) => {
    if (!info.download_url) {
      if (info.url) api_settings.open_file(info.url).catch(() => {});
      return;
    }
    setDownloading(true);
    setDownloadPct(0);
    try {
      const r = await api_settings.download_update(info.download_url, info.sha256 || "");
      if (r.ok) {
        await api_settings.apply_update(r.path);
        return;
      }
      // 校验失败：回退引导手动下载
      if (info.url) api_settings.open_file(info.url).catch(() => {});
    } catch {}
    setDownloading(false);
  }, []);

  // 就绪信号：通知 Rust 前端已挂载（Rust 侧记录日志；窗口已默认
  // 可见，Rust 的 show 为幂等 no-op）。启动页已移除（用户指定）。
  useEffect(() => {
    invoke("window_ready").catch(() => {});
  }, []);

  // 启动 5 秒后检测一次 + 每 1 小时周期检测（后台，不阻塞界面）
  useEffect(() => {
    const t = setTimeout(() => checkUpdate(), 5000);
    const iv = setInterval(() => checkUpdate(), 3600_000);
    return () => { clearTimeout(t); clearInterval(iv); };
  }, [checkUpdate]);

  // 监听下载进度事件
  useEffect(() => {
    const un = listen<{ percent: number }>("update:download-progress", (e) => {
      setDownloadPct(e.payload.percent);
    });
    return () => { un.then((f) => f()); };
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

      {/* 普通更新弹窗（是 / 稍后 / 跳过此版本） */}
      {updateInfo && !forceUpdate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--color-bg)]/60 backdrop-blur-sm">
          <div className="mx-6 w-full max-w-md rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-6 shadow-2xl">
            <div className="flex items-center gap-3">
              <div className="flex h-10 w-10 items-center justify-center rounded-xl" style={{ background: "var(--color-primary-soft)", color: "var(--color-primary)" }}>
                <HardDriveDownload size={20} />
              </div>
              <div>
                <h2 className="text-base font-bold">发现新版本 v{updateInfo.latest_version}</h2>
                <div className="text-xs text-[var(--color-text-secondary)]">当前 v{updateInfo.current_version}</div>
              </div>
            </div>

            {updateInfo.notes && (
              <div className="mt-3 max-h-40 overflow-y-auto rounded-lg bg-[var(--color-bg-2)] px-3 py-2 text-sm whitespace-pre-wrap">
                {updateInfo.notes}
              </div>
            )}

            {downloading && (
              <div className="mt-3">
                <div className="mb-1 flex justify-between text-xs text-[var(--color-text-secondary)]">
                  <span>正在下载更新…</span>
                  <span className="tabular-nums">{downloadPct}%</span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                  <div className="h-full rounded-full bg-[var(--color-primary)] transition-all" style={{ width: `${downloadPct}%` }} />
                </div>
              </div>
            )}

            <div className="mt-5 flex items-center justify-end gap-2">
              <button
                onClick={() => { localStorage.setItem("skip_version", String(updateInfo.latest_version)); setUpdateInfo(null); }}
                disabled={downloading}
                className="rounded-lg px-3 py-2 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
              >
                跳过此版本
              </button>
              <button
                onClick={() => setUpdateInfo(null)}
                disabled={downloading}
                className="rounded-lg px-4 py-2 text-sm text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
              >
                稍后
              </button>
              <button
                onClick={() => doUpdate(updateInfo)}
                disabled={downloading}
                className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
              >
                <Download size={15} />
                {downloading ? "下载中…" : "立即更新"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 强制更新全屏拦截（本地版本低于 minimum_version，不可跳过） */}
      {forceUpdate && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-[var(--color-bg)]/85 backdrop-blur-sm">
          <div className="mx-6 w-full max-w-md rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-8 text-center shadow-2xl">
            <HardDriveDownload size={40} className="mx-auto mb-4 text-[var(--color-primary)]" />
            <h2 className="text-lg font-bold">需要更新才能继续使用</h2>
            <p className="mt-2 text-sm text-[var(--color-text-secondary)]">
              当前版本 v{forceUpdate.current_version} 过低，存在安全或兼容性问题，
              必须升级到 v{forceUpdate.latest_version}。
            </p>
            {forceUpdate.notes && (
              <p className="mt-2 rounded-lg bg-[var(--color-primary-soft)] px-3 py-2 text-xs">{forceUpdate.notes}</p>
            )}
            {downloading && (
              <div className="mt-3">
                <div className="mb-1 flex justify-between text-xs text-[var(--color-text-secondary)]">
                  <span>正在下载更新…</span>
                  <span className="tabular-nums">{downloadPct}%</span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                  <div className="h-full rounded-full bg-[var(--color-primary)] transition-all" style={{ width: `${downloadPct}%` }} />
                </div>
              </div>
            )}
            <button
              onClick={() => doUpdate(forceUpdate)}
              disabled={downloading}
              className="mt-6 inline-flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-6 py-2.5 text-sm font-semibold text-white disabled:opacity-50"
            >
              <Download size={16} />
              {downloading ? "下载中…" : "立即更新"}
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
