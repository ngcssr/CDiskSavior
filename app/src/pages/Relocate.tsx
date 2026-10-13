import { useEffect, useState, useCallback, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api_relocate, api_settings } from "../api/commands";
import type { RelocateTarget, RelocateConfig, MigrationRecord } from "../api/commands";
import {
  HardDriveDownload, RefreshCw, Play, Pause, X, RotateCcw,
  Folder, AlertTriangle, CheckCircle, Trash2, FolderInput, Plus,
} from "lucide-react";

function fmt(bytes: number): string {
  if (!bytes) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(2)} ${u[i]}`;
}

function fmtTime(ts: number): string {
  try {
    return new Date(ts * 1000).toLocaleString("zh-CN", { hour12: false });
  } catch {
    return String(ts);
  }
}

/** 剥掉 long_path 的 \\?\ 前缀（历史记录存的是转换后路径，展示需还原） */
function stripUnc(p: string): string {
  return p.startsWith("\\\\?\\") ? p.slice(4) : p;
}

type ProgressEvent = {
  type: string;
  source?: string;
  freed?: number;
  totalFreed?: number;
  bytesDone?: number;
  bytesTotal?: number;
  speedBps?: number;
  reason?: string;
  message?: string;
};

type Phase = "idle" | "scanning" | "ready" | "migrating" | "error";

export default function Relocate() {
  const [tab, setTab] = useState<"scan" | "history">("scan");
  const [config, setConfig] = useState<RelocateConfig | null>(null);
  const [targets, setTargets] = useState<RelocateTarget[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [history, setHistory] = useState<MigrationRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [targetFree, setTargetFree] = useState<number | null>(null);
  const [autoKill, setAutoKill] = useState(false);
  const [concurrency, setConcurrency] = useState(6);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const addLog = (msg: string) => setLogs((l) => [...l.slice(-50), msg]);

  const queryTargetFree = async (root: string) => {
    try { setTargetFree(await api_relocate.get_target_drive_free(root)); }
    catch (e: any) { setError(`查询目标盘剩余空间失败: ${e?.message ?? e}`); }
  };

  // 初始加载：配置 + 历史
  useEffect(() => {
    (async () => {
      try {
        const [cfg, hist] = await Promise.all([
          api_relocate.get_relocate_config(),
          api_relocate.list_migration_history(),
        ]);
        setConfig(cfg);
        setHistory(hist);
        if (cfg.targetRoot) {
          try { setTargetFree(await api_relocate.get_target_drive_free(cfg.targetRoot)); }
          catch (e: any) { setError(`查询目标盘剩余空间失败: ${e?.message ?? e}`); }
        }
      } catch (e: any) {
        setError(e?.message ?? String(e));
      }
    })();

    listen<ProgressEvent>("migration-progress", (e) => {
      const p = e.payload;
      setProgress(p);
      switch (p.type) {
        case "progress":
          break;
        case "item-done":
          addLog(`✓ ${p.source} → 释放 ${fmt(p.freed ?? 0)}`);
          break;
        case "item-skipped":
          addLog(`⊘ 跳过 ${p.source}：${p.reason ?? ""}`);
          break;
        case "error":
          addLog(`✗ ${p.message ?? "迁移出错"}`);
          break;
        case "cancelled":
          addLog("已取消");
          break;
        case "done":
          // 双兼容：新字段 totalFreed，旧字段 freed（防版本错配显示 0）
          addLog(`✓ 迁移完成，共释放 ${fmt(p.totalFreed ?? p.freed ?? 0)}`);
          break;
      }
    }).then((un) => { unlistenRef.current = un; });

    return () => { unlistenRef.current?.(); };
  }, []);

  // 扫描
  const doScan = useCallback(async () => {
    setPhase("scanning");
    setError(null);
    try {
      const result = await api_relocate.scan_relocate_targets();
      setTargets(result);
      setSelected(new Set());
      setPhase("ready");
      addLog(`扫描完成：${result.length} 个可迁移目录`);
    } catch (e: any) {
      setError(e?.message ?? String(e));
      setPhase("error");
    }
  }, []);

  const toggleSelect = (path: string) => {
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  // 更换目标盘
  const pickTarget = async () => {
    try {
      const dir = await api_settings.pick_folder_dialog("选择迁移目标盘根目录");
      if (!dir) return;
      const newCfg = { ...config!, targetRoot: dir };
      setConfig(newCfg);
      await api_relocate.save_relocate_config(newCfg);
      await queryTargetFree(dir);
      addLog(`目标盘已更换为 ${dir}`);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  // 开始迁移（勾选"自动结束占用进程"时先查杀）
  const startMigration = async () => {
    if (!config?.targetRoot || selected.size === 0) return;
    setPhase("migrating");
    setError(null);
    setLogs([]);
    try {
      if (autoKill) {
        addLog("自动结束占用进程…");
        let killed = 0;
        for (const path of selected) {
          try {
            const procs = await api_relocate.list_locking_processes(path);
            for (const p of procs ?? []) {
              try {
                await api_relocate.kill_locking_process(p.pid);
                addLog(`已结束 ${p.name} (PID ${p.pid})`);
                killed++;
              } catch {}
            }
          } catch {}
        }
        if (killed === 0) addLog("未发现占用进程");
      }
      await api_relocate.start_migration(
        Array.from(selected),
        config.targetRoot,
        [],
        concurrency,
      );
      setHistory(await api_relocate.list_migration_history());
      setPhase("ready"); // 迁移完成复位状态（否则卡在"迁移中 0%"）
    } catch (e: any) {
      setError(e?.message ?? String(e));
      setPhase("error");
    }
  };

  const pause = () => api_relocate.pause_migration().catch(() => {});
  const resume = () => api_relocate.resume_migration().catch(() => {});
  const cancel = () => api_relocate.cancel_migration().catch(() => {});

  const restore = async (id: string) => {
    try {
      await api_relocate.restore_migration(id);
      addLog(`✓ 已回滚：${id}`);
      setHistory(await api_relocate.list_migration_history());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  const removeRecord = async (id: string) => {
    try {
      await api_relocate.remove_migration_record(id);
      setHistory(await api_relocate.list_migration_history());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  const totalSelected = Array.from(selected).reduce(
    (sum, path) => sum + (targets.find((t) => t.path === path)?.size ?? 0),
    0,
  );

  const migrating = phase === "migrating";
  const progressPct = progress?.bytesTotal && progress.bytesTotal > 0
    ? Math.min((progress.bytesDone ?? 0) / progress.bytesTotal * 100, 100)
    : 0;

  // 底部状态栏文案
  const statusText = (() => {
    if (phase === "scanning") return "扫描中…";
    if (phase === "error") return "出错（见上方提示）";
    if (migrating) {
      if (progress?.bytesTotal === 0) return `正在分析 ${progress?.source ?? ""}…`;
      if (progress?.bytesTotal && progress.bytesTotal > 0)
        return `迁移中 ${progressPct.toFixed(0)}% · ${fmt(progress.bytesDone ?? 0)}/${fmt(progress.bytesTotal)}${progress.speedBps ? ` · ${fmt(progress.speedBps)}/s` : ""}`;
      return "迁移中…";
    }
    if (phase === "ready" || targets.length > 0)
      return `准备就绪：${targets.length} 个可迁移，已选 ${selected.size} 个（${fmt(totalSelected)}）`;
    return "准备就绪";
  })();

  return (
    <div className="flex h-full flex-col">
      {/* 头部 + tab */}
      <div className="border-b border-[var(--color-line)] px-6 py-3">
        <div className="flex items-center gap-2">
          <HardDriveDownload size={18} className="text-[var(--color-primary)]" />
          <h1 className="text-lg font-bold">空间腾挪</h1>
          <div className="ml-4 flex gap-1">
            <button
              onClick={() => setTab("scan")}
              className={`rounded-lg px-3 py-1 text-sm font-medium ${tab === "scan" ? "bg-[var(--color-primary)] text-white" : "text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"}`}
            >
              扫描与迁移
            </button>
            <button
              onClick={() => setTab("history")}
              className={`rounded-lg px-3 py-1 text-sm font-medium ${tab === "history" ? "bg-[var(--color-primary)] text-white" : "text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"}`}
            >
              迁移记录
            </button>
          </div>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-3 flex items-center gap-2 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-2.5 text-sm text-[var(--color-danger-text)]">
          <AlertTriangle size={14} />
          {error}
          <button onClick={() => setError(null)} className="ml-auto text-xs opacity-60 hover:opacity-100">✕</button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-6">
        {tab === "scan" ? (
          <>
            {/* 三个大按钮一行 */}
            <div className="mb-2 flex items-center gap-3">
              <button
                onClick={doScan}
                disabled={migrating}
                className="flex items-center gap-2 rounded-lg border-2 border-[var(--color-primary)] px-5 py-2.5 text-base font-bold text-[var(--color-primary)] disabled:opacity-50"
              >
                <RefreshCw size={16} className={phase === "scanning" ? "animate-spin" : ""} />
                扫描大文件夹
              </button>
              <button
                onClick={startMigration}
                disabled={migrating || selected.size === 0}
                className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-5 py-2.5 text-base font-bold text-white disabled:opacity-40"
              >
                <HardDriveDownload size={16} />
                执行迁移
              </button>
            </div>
            {/* 使用步骤一行字 */}
            <div className="mb-4 text-xs text-[var(--color-text-secondary)]">
              使用步骤：1 扫描大文件夹 → 2 勾选目录 → 3 执行迁移
            </div>

            {/* 目标根目录单行 */}
            <div className="mb-3 flex items-center gap-2 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
              <span className="shrink-0 text-sm font-semibold">目标根目录:</span>
              <span className="font-mono text-sm text-[var(--color-primary)]">
                {config?.targetRoot ?? "未设置"}
              </span>
              {targetFree !== null && (
                <span className="text-xs text-[var(--color-text-secondary)]">
                  （剩余 {fmt(targetFree)}
                  {totalSelected > 0 && targetFree < totalSelected && (
                    <span className="text-[var(--color-danger-text)]">，⚠ 空间不足需 {fmt(totalSelected)}</span>
                  )}）
                </span>
              )}
              <button
                onClick={pickTarget}
                className="ml-auto shrink-0 rounded border border-[var(--color-line)] px-2 py-0.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
              >
                选择…
              </button>
            </div>

            {/* 扫描源紧凑列表 */}
            {config && (
              <div className="mb-4">
                <div className="mb-1 flex items-center gap-2">
                  <span className="shrink-0 text-sm font-semibold">扫描目录:</span>
                  <button
                    onClick={async () => {
                      try {
                        const dir = await api_settings.pick_folder_dialog("选择要迁移的目录");
                        if (!dir) return;
                        const name = dir.split(/[\\/]/).filter(Boolean).pop() ?? "自定义";
                        const next = { ...config, scanSources: [...config.scanSources, { enabled: true, label: name, path: dir, targetSubdir: name }] };
                        setConfig(next);
                        await api_relocate.save_relocate_config(next);
                      } catch (e: any) {
                        setError(e?.message ?? String(e));
                      }
                    }}
                    className="flex items-center gap-1 rounded border border-[var(--color-line)] px-2 py-0.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                  >
                    <Plus size={10} /> 添加...
                  </button>
                </div>
                <div className="space-y-0.5">
                  {config.scanSources.map((s, i) => (
                    <div key={s.path} className="flex items-center gap-2 rounded px-2 py-1 hover:bg-[var(--color-surface-hover)]">
                      <input
                        type="checkbox"
                        checked={s.enabled}
                        onChange={(e) => {
                          const next = [...config.scanSources];
                          next[i] = { ...s, enabled: e.target.checked };
                          setConfig({ ...config, scanSources: next });
                        }}
                        className="h-3.5 w-3.5 accent-[var(--color-primary)]"
                      />
                      <span className="w-36 shrink-0 truncate text-sm font-medium">{s.label}</span>
                      <span className="flex-1 truncate font-mono text-xs text-[var(--color-text-secondary)]" title={s.path}>
                        {s.path}
                      </span>
                      <button
                        onClick={async () => {
                          const next = { ...config, scanSources: config.scanSources.filter((x) => x.path !== s.path) };
                          setConfig(next);
                          await api_relocate.save_relocate_config(next);
                        }}
                        className="shrink-0 rounded p-0.5 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-danger-bg-soft)] hover:text-[var(--color-danger-text)]"
                      >
                        <X size={12} />
                      </button>
                    </div>
                  ))}
                </div>
                {/* 选项行：自动结束占用进程 + 并发传输 */}
                <div className="mt-2 flex items-center gap-4 px-2 py-1 text-sm">
                  <label className="flex cursor-pointer items-center gap-2">
                    <input
                      type="checkbox"
                      checked={autoKill}
                      onChange={(e) => setAutoKill(e.target.checked)}
                      className="h-3.5 w-3.5 accent-[var(--color-primary)]"
                    />
                    自动结束占用进程（慎用）
                  </label>
                  <label className="flex items-center gap-2 text-[var(--color-text-secondary)]">
                    并发传输
                    <input
                      type="range"
                      min={1}
                      max={16}
                      value={concurrency}
                      onChange={(e) => setConcurrency(Number(e.target.value))}
                      className="w-24 accent-[var(--color-primary)]"
                    />
                    <span className="w-6 tabular-nums">{concurrency}</span>
                  </label>
                </div>
              </div>
            )}

            {/* 扫描结果列表（格式对齐 AppData Mover：名称 (大小) [来源] 路径） */}
            {targets.length > 0 && (
              <div className="space-y-0.5">
                {targets.map((t) => {
                  const folderName = t.path.split(/[\\/]/).filter(Boolean).pop() ?? t.label;
                  return (
                    <label
                      key={t.path}
                      className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 hover:bg-[var(--color-surface-hover)] ${selected.has(t.path) ? "bg-[var(--color-primary-soft)]" : ""}`}
                    >
                      <input
                        type="checkbox"
                        checked={selected.has(t.path)}
                        onChange={() => toggleSelect(t.path)}
                        disabled={migrating || t.isJunction}
                        className="h-3.5 w-3.5 accent-[var(--color-primary)]"
                      />
                      {t.isJunction
                        ? <FolderInput size={14} className="shrink-0 text-[var(--color-caution)]" />
                        : <Folder size={14} className="shrink-0 text-[var(--color-primary)]" />}
                      <span className="shrink-0 font-medium text-[var(--color-primary)]">
                        {folderName} ({fmt(t.size)})
                      </span>
                      <span className="shrink-0 rounded bg-[var(--color-surface-hover)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                        [{t.label}]
                      </span>
                      <span className="flex-1 truncate font-mono text-xs text-[var(--color-text-secondary)]" title={t.path}>{t.path}</span>
                      {t.isJunction && <span className="shrink-0 text-xs text-[var(--color-caution)]">已是链接</span>}
                    </label>
                  );
                })}
              </div>
            )}

            {/* 迁移中进度条 */}
            {migrating && (
              <div className="mt-4 rounded-lg border border-[var(--color-primary)]/30 bg-[var(--color-primary-soft)]/30 p-3">
                <div className="mb-1 flex justify-between text-xs">
                  <span className="truncate">
                    {progress?.source
                      ? (progress.bytesTotal === 0 ? `正在分析 ${progress.source}…` : progress.source)
                      : "迁移中…"}
                  </span>
                  <span className="tabular-nums">
                    {progress?.bytesTotal === 0 ? "…" : `${progressPct.toFixed(0)}%`}
                  </span>
                </div>
                <div className="h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                  {progress?.bytesTotal === 0 ? (
                    <div className="h-full w-1/3 animate-pulse rounded-full bg-[var(--color-primary)]" />
                  ) : (
                    <div className="h-full rounded-full bg-[var(--color-primary)] transition-all" style={{ width: `${progressPct}%` }} />
                  )}
                </div>
                <div className="mt-2 flex gap-2">
                  <button onClick={pause} className="flex items-center gap-1 rounded bg-[var(--color-surface-hover)] px-2.5 py-1 text-xs">
                    <Pause size={12} /> 暂停
                  </button>
                  <button onClick={resume} className="flex items-center gap-1 rounded bg-[var(--color-surface-hover)] px-2.5 py-1 text-xs">
                    <Play size={12} /> 继续
                  </button>
                  <button onClick={cancel} className="flex items-center gap-1 rounded bg-[var(--color-danger-bg-soft)] px-2.5 py-1 text-xs text-[var(--color-danger-text)]">
                    <X size={12} /> 取消
                  </button>
                </div>
              </div>
            )}

            {/* 迁移日志 */}
            {logs.length > 0 && (
              <div className="mt-4 max-h-36 overflow-y-auto rounded-lg bg-[var(--color-surface)] p-3 font-mono text-xs">
                {logs.map((line, i) => (
                  <div key={i} className="py-0.5">{line}</div>
                ))}
              </div>
            )}
          </>
        ) : (
          /* 迁移记录 tab */
          <div className="space-y-1">
            {history.length === 0 && (
              <div className="py-8 text-center text-sm text-[var(--color-text-secondary)]">暂无迁移记录</div>
            )}
            {history.map((r) => (
              <div
                key={r.id}
                className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2"
              >
                <CheckCircle size={16} className="shrink-0 text-[var(--color-safe)]" />
                <div className="flex-1">
                  <div className="text-sm font-medium">{r.name}</div>
                  <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">
                    {stripUnc(r.sourcePath)} → {stripUnc(r.targetPath)}
                  </div>
                </div>
                <span className="shrink-0 text-xs tabular-nums text-[var(--color-text-secondary)]">
                  {fmt(r.sizeBytes)} · {fmtTime(r.timestamp)}
                </span>
                <button
                  onClick={() => restore(r.id)}
                  className="flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-xs text-[var(--color-primary)] hover:bg-[var(--color-primary-soft)]"
                >
                  <RotateCcw size={12} /> 回滚
                </button>
                <button
                  onClick={() => removeRecord(r.id)}
                  className="shrink-0 rounded-lg p-1 text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* 底部状态栏 */}
      <div className="border-t border-[var(--color-line)] bg-[var(--color-surface)] px-6 py-1.5">
        <span className={`text-xs ${phase === "error" ? "text-[var(--color-danger-text)]" : "text-[var(--color-text-secondary)]"}`}>
          {statusText}
        </span>
      </div>
    </div>
  );
}
