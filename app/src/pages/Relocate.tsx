import { useEffect, useState, useCallback, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api_relocate, api_settings } from "../api/commands";
import type { RelocateTarget, RelocateConfig, MigrationRecord } from "../api/commands";
import {
  HardDriveDownload, RefreshCw, Play, Pause, X, RotateCcw,
  Folder, AlertTriangle, CheckCircle, Trash2, FolderInput, Search,
} from "lucide-react";

type ProgressEvent = {
  type: string; source?: string; bytesDone?: number; bytesTotal?: number;
  speedBps?: number; freed?: number; reason?: string; totalFreed?: number;
  records?: MigrationRecord[]; message?: string; partialCleanup?: string;
};

function fmt(bytes: number): string {
  if (!bytes) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${u[i]}`;
}

function fmtTime(ts: number): string {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleString("zh-CN");
}

type Phase = "idle" | "scanning" | "ready" | "migrating" | "done" | "error";

export default function Relocate() {
  const [config, setConfig] = useState<RelocateConfig | null>(null);
  const [targets, setTargets] = useState<RelocateTarget[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [history, setHistory] = useState<MigrationRecord[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lockingProcs, setLockingProcs] = useState<any[]>([]);
  const [lockLoading, setLockLoading] = useState(false);
  const [killPids, setKillPids] = useState<Set<number>>(new Set());
  const [targetFree, setTargetFree] = useState<number | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const addLog = (msg: string) => setLogs((l) => [...l.slice(-50), msg]);

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
        // 查询目标盘剩余空间
        if (cfg.targetRoot) {
          try { setTargetFree(await api_relocate.get_target_drive_free(cfg.targetRoot)); }
          catch (e: any) { setError(`查询目标盘剩余空间失败: ${e?.message ?? e}`); }
        }
      } catch (e: any) {
        setError(e?.message ?? String(e));
      }
    })();

    // 监听迁移进度事件
    listen<ProgressEvent>("migration-progress", (e) => {
      const p = e.payload;
      setProgress(p);
      switch (p.type) {
        case "progress":
          // 不打日志（太频繁）
          break;
        case "item-done":
          addLog(`✓ ${p.source} → 释放 ${fmt(p.freed ?? 0)}`);
          break;
        case "item-skipped":
          addLog(`⊘ 跳过 ${p.source}：${p.reason}`);
          break;
        case "done":
          addLog(`✓ 迁移完成，共释放 ${fmt(p.totalFreed ?? 0)}`);
          setPhase("done");
          api_relocate.list_migration_history().then(setHistory).catch(() => {});
          break;
        case "error":
          addLog(`✗ 错误：${p.message}`);
          setPhase("error");
          setError(p.message ?? "未知错误");
          break;
        case "cancelled":
          addLog(`⊘ 已取消：${p.partialCleanup}`);
          setPhase("idle");
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

  // 切换选择
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
      try { setTargetFree(await api_relocate.get_target_drive_free(dir)); } catch (e: any) { setError(`查询目标盘剩余空间失败: ${e?.message ?? e}`); }
      addLog(`目标盘已更换为 ${dir}`);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  // 检查占用进程（对已选目录批量查）
  const checkLocking = async () => {
    setLockLoading(true);
    setError(null);
    try {
      const all: any[] = [];
      for (const path of selected) {
        const procs = await api_relocate.list_locking_processes(path);
        all.push(...(procs ?? []).map((p: any) => ({ ...p, sourcePath: path })));
      }
      setLockingProcs(all);
      setKillPids(new Set());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setLockLoading(false);
    }
  };
  const toggleKillPid = (pid: number) =>
    setKillPids(prev => { const n = new Set(prev); n.has(pid) ? n.delete(pid) : n.add(pid); return n; });
  const killSelected = async () => {
    for (const pid of killPids) {
      try { await api_relocate.kill_locking_process(pid); } catch {}
    }
    addLog(`已结束 ${killPids.size} 个占用进程`);
    setKillPids(new Set());
    await checkLocking();
  };

  // 开始迁移
  const startMigration = async () => {
    if (!config?.targetRoot || selected.size === 0) return;
    setPhase("migrating");
    setError(null);
    setLogs([]);
    try {
      await api_relocate.start_migration(
        Array.from(selected),
        config.targetRoot,
        Array.from(killPids), // 用户选中的占用进程（之前是空数组——stub）
      );
    } catch (e: any) {
      setError(e?.message ?? String(e));
      setPhase("error");
    }
  };

  // 控制按钮
  const pause = () => api_relocate.pause_migration().catch(() => {});
  const resume = () => api_relocate.resume_migration().catch(() => {});
  const cancel = () => api_relocate.cancel_migration().catch(() => {});

  // 回滚
  const restore = async (id: string) => {
    try {
      await api_relocate.restore_migration(id);
      addLog(`✓ 已回滚：${id}`);
      setHistory(await api_relocate.list_migration_history());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  // 删除记录
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

  return (
    <div className="flex h-full flex-col">
      {/* 头部 */}
      <div className="border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <HardDriveDownload size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">空间腾挪</h1>
        </div>
        <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
          将 AppData 大文件夹迁移到其他磁盘，原位留下 Junction，软件无感知
        </p>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {error && (
          <div className="mb-4 flex items-center gap-2 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">
            <AlertTriangle size={16} />
            {error}
          </div>
        )}

        {/* 步骤 1：选择目标盘 */}
        {config && (
          <section className="mb-6 rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
            <div className="mb-2 flex items-center gap-2">
              <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[var(--color-primary)] text-xs font-bold text-white">1</span>
              <span className="text-sm font-semibold">选择目标盘</span>
            </div>
            <div className="flex items-center justify-between">
              <div className="flex-1">
                <div className="font-mono text-sm text-[var(--color-primary)]">
                  {config.targetRoot ?? "未设置（点击右侧选择）"}
                </div>
                <div className="mt-1 text-xs">
                  剩余空间：
                  {targetFree === null
                    ? <span className="text-[var(--color-text-secondary)]">查询中…</span>
                    : <strong className={targetFree < totalSelected ? "text-[var(--color-danger-text)]" : "text-[var(--color-safe)]"}>{fmt(targetFree)}</strong>}
                  {totalSelected > 0 && targetFree !== null && (
                    <span className="ml-2 text-[var(--color-text-secondary)]">
                      · 需迁移 {fmt(totalSelected)}
                      {targetFree < totalSelected && (
                        <span className="ml-1 text-[var(--color-danger-text)]">⚠ 空间不足</span>
                      )}
                    </span>
                  )}
                </div>
              </div>
              <button
                onClick={pickTarget}
                className="flex items-center gap-1.5 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-1.5 text-xs font-medium hover:bg-[var(--color-surface-hover)]"
              >
                <FolderInput size={12} />
                {config.targetRoot ? "更换" : "选择"}
              </button>
            </div>
          </section>
        )}

        {/* 步骤 2：选择要迁移的目录 */}
        {config && (
          <section className="mb-6 rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
            <div className="mb-2 flex items-center gap-2">
              <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[var(--color-primary)] text-xs font-bold text-white">2</span>
              <span className="text-sm font-semibold">选择要迁移的目录</span>
              <span className="ml-auto text-xs text-[var(--color-text-secondary)]">
                扫描源（{config.scanSources.length} 个，勾选后参与扫描）
              </span>
              <div className="flex gap-2">
                <button
                  onClick={async () => {
                    try {
                      const dir = await api_settings.pick_folder_dialog("选择要迁移的目录");
                      if (!dir) return;
                      const name = dir.split(/[\\/]/).filter(Boolean).pop() ?? "自定义";
                      setConfig({
                        ...config,
                        scanSources: [
                          ...config.scanSources,
                          { enabled: true, label: name, path: dir, targetSubdir: name },
                        ],
                      });
                    } catch (e: any) {
                      setError(e?.message ?? String(e));
                    }
                  }}
                  className="rounded-lg border border-[var(--color-line)] px-3 py-1 text-xs font-medium hover:bg-[var(--color-surface-hover)]"
                >
                  + 添加自定义目录
                </button>
                <button
                  onClick={async () => {
                    try {
                      await api_relocate.save_relocate_config(config);
                      setError(null);
                    } catch (e: any) {
                      setError(e?.message ?? String(e));
                    }
                  }}
                  className="rounded-lg bg-[var(--color-primary)] px-3 py-1 text-xs font-medium text-white"
                >
                  保存
                </button>
              </div>
            </div>
            <div className="space-y-1">
              {config.scanSources.map((s, i) => (
                <div
                  key={s.path}
                  className="flex items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-[var(--color-surface-hover)]"
                >
                  <input
                    type="checkbox"
                    checked={s.enabled}
                    onChange={(e) => {
                      const next = [...config.scanSources];
                      next[i] = { ...s, enabled: e.target.checked };
                      setConfig({ ...config, scanSources: next });
                    }}
                    className="h-4 w-4 accent-[var(--color-primary)]"
                  />
                  <span className="w-36 shrink-0 truncate text-sm font-medium">{s.label}</span>
                  <span
                    className="flex-1 truncate font-mono text-xs text-[var(--color-text-secondary)]"
                    title={s.path}
                  >
                    {s.path}
                  </span>
                  <span className="shrink-0 rounded bg-[var(--color-surface-hover)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--color-text-secondary)]">
                    → {s.targetSubdir}
                  </span>
                  {i >= 5 && (
                    <button
                      onClick={() =>
                        setConfig({
                          ...config,
                          scanSources: config.scanSources.filter((x) => x.path !== s.path),
                        })
                      }
                      className="shrink-0 text-xs text-[var(--color-danger-text)] hover:underline"
                    >
                      移除
                    </button>
                  )}
                </div>
              ))}
            </div>
            <p className="mt-2 text-xs text-[var(--color-text-secondary)]">
              Program Files / ProgramData 迁移影响面大，默认关闭；开启前请确认目标盘空间并先备份。
            </p>
          </section>
        )}

        {/* 扫描 + 列表 */}
        <section className="mb-6">
          <div className="mb-3 flex items-center gap-3">
            <button
              onClick={doScan}
              disabled={migrating}
              className="flex items-center gap-1.5 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
            >
              <RefreshCw size={14} className={phase === "scanning" ? "animate-spin" : ""} />
              扫描可迁移目录
            </button>
            {targets.length > 0 && (
              <span className="text-sm text-[var(--color-text-secondary)]">
                {targets.length} 个目录，已选 {selected.size} 个（共 {fmt(totalSelected)}）
              </span>
            )}
          </div>

          {targets.length > 0 && (
            <div className="space-y-1">
              {targets.map((t) => (
                <label
                  key={t.path}
                  className={`flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 hover:bg-[var(--color-surface-hover)] ${
                    selected.has(t.path) ? "bg-[var(--color-primary-soft)]" : ""
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={selected.has(t.path)}
                    onChange={() => toggleSelect(t.path)}
                    disabled={migrating || t.isJunction}
                    className="h-4 w-4 accent-[var(--color-primary)]"
                  />
                  {t.isJunction ? (
                    <FolderInput size={16} className="text-[var(--color-caution)]" />
                  ) : (
                    <Folder size={16} className="text-[var(--color-primary)]" />
                  )}
                  <div className="flex-1">
                    <div className="text-sm font-medium">{t.label}</div>
                    <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{t.path}</div>
                  </div>
                  {t.isJunction ? (
                    <span className="text-xs text-[var(--color-caution)]">已是链接</span>
                  ) : (
                    <span className="text-sm tabular-nums text-[var(--color-text-secondary)]">{fmt(t.size)}</span>
                  )}
                </label>
              ))}
            </div>
          )}
        </section>

        {/* 步骤 3：开始迁移 */}
        {selected.size > 0 && (
          <section className="mb-6 rounded-2xl border border-[var(--color-primary)]/30 bg-[var(--color-primary-soft)]/30 p-4">
            <div className="mb-2 flex items-center gap-2">
              <span className="flex h-5 w-5 items-center justify-center rounded-full bg-[var(--color-primary)] text-xs font-bold text-white">3</span>
              <span className="text-sm font-semibold">开始迁移</span>
            </div>
            {migrating ? (
              <div className="space-y-3">
                {/* 进度条 */}
                <div>
                  <div className="mb-1 flex justify-between text-xs">
                    <span className="truncate">
                      {progress?.source
                        ? (progress.bytesTotal === 0
                            ? `正在分析 ${progress.source}…`
                            : progress.source)
                        : "迁移中…"}
                    </span>
                    <span className="tabular-nums">
                      {progress?.bytesTotal === 0 ? "…" : `${progressPct.toFixed(0)}%`}
                    </span>
                  </div>
                  <div className="h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                    {progress?.bytesTotal === 0 ? (
                      <div className="h-full w-1/3 rounded-full bg-[var(--color-primary)] animate-pulse" />
                    ) : (
                      <div
                        className="h-full rounded-full bg-[var(--color-primary)] transition-all"
                        style={{ width: `${progressPct}%` }}
                      />
                    )}
                  </div>
                  {progress?.bytesTotal && progress.bytesTotal > 0 && (
                    <div className="mt-1 text-xs text-[var(--color-text-secondary)]">
                      {fmt(progress.bytesDone ?? 0)} / {fmt(progress.bytesTotal)}
                      {progress.speedBps ? ` · ${fmt(progress.speedBps)}/s` : ""}
                    </div>
                  )}
                </div>
                {/* 控制按钮 */}
                <div className="flex gap-2">
                  <button onClick={pause} className="flex items-center gap-1 rounded-lg bg-[var(--color-surface-hover)] px-3 py-1.5 text-sm">
                    <Pause size={14} /> 暂停
                  </button>
                  <button onClick={resume} className="flex items-center gap-1 rounded-lg bg-[var(--color-surface-hover)] px-3 py-1.5 text-sm">
                    <Play size={14} /> 继续
                  </button>
                  <button onClick={cancel} className="flex items-center gap-1 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)]">
                    <X size={14} /> 取消
                  </button>
                </div>
              </div>
            ) : (
              <div className="space-y-3">
                {/* 占用进程检查 */}
                <div className="flex items-center gap-2">
                  <button
                    onClick={checkLocking}
                    disabled={lockLoading}
                    className="flex items-center gap-1.5 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-1.5 text-xs text-[var(--color-text-secondary)] disabled:opacity-50"
                  >
                    <Search size={12} />
                    {lockLoading ? "检查中…" : "检查占用进程"}
                  </button>
                  {lockingProcs.length > 0 && (
                    <span className="text-xs text-[var(--color-caution)]">
                      {lockingProcs.length} 个进程占用
                    </span>
                  )}
                </div>
                {lockingProcs.length > 0 && (
                  <div className="space-y-1 rounded-lg bg-[var(--color-surface-hover)] p-2">
                    {lockingProcs.map((p, i) => (
                      <label key={i} className="flex cursor-pointer items-center gap-2 px-1 py-0.5 text-xs">
                        <input
                          type="checkbox"
                          checked={killPids.has(p.pid)}
                          onChange={() => toggleKillPid(p.pid)}
                          className="accent-[var(--color-danger)]"
                        />
                        <span className="font-medium">{p.name}</span>
                        <span className="text-[var(--color-text-secondary)]">PID: {p.pid}</span>
                        <span className="truncate text-[var(--color-text-secondary)]" title={p.sourcePath}>{p.sourcePath}</span>
                      </label>
                    ))}
                    <button
                      onClick={killSelected}
                      disabled={killPids.size === 0}
                      className="mt-1 flex items-center gap-1 rounded bg-[var(--color-danger)] px-2 py-1 text-xs text-white disabled:opacity-40"
                    >
                      <X size={10} /> 结束选中进程（{killPids.size}）
                    </button>
                  </div>
                )}
                <button
                  onClick={startMigration}
                  className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white"
                >
                  <HardDriveDownload size={16} />
                  开始迁移 {selected.size} 个目录（{fmt(totalSelected)}）
                </button>
              </div>
            )}
          </section>
        )}

        {/* 日志 */}
        {logs.length > 0 && (
          <section className="mb-6">
            <h3 className="mb-2 text-xs font-semibold text-[var(--color-text-secondary)]">迁移日志</h3>
            <div className="max-h-40 overflow-y-auto rounded-xl bg-[var(--color-surface)] p-3 font-mono text-xs">
              {logs.map((line, i) => (
                <div key={i} className="py-0.5">{line}</div>
              ))}
            </div>
          </section>
        )}

        {/* 历史 */}
        {history.length > 0 && (
          <section>
            <h3 className="mb-2 text-xs font-semibold text-[var(--color-text-secondary)]">迁移历史</h3>
            <div className="space-y-1">
              {history.map((r) => (
                <div
                  key={r.id}
                  className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2"
                >
                  <CheckCircle size={16} className="text-[var(--color-safe)]" />
                  <div className="flex-1">
                    <div className="text-sm font-medium">{r.name}</div>
                    <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">
                      {r.sourcePath} → {r.targetPath}
                    </div>
                  </div>
                  <span className="text-xs tabular-nums text-[var(--color-text-secondary)]">
                    {fmt(r.sizeBytes)} · {fmtTime(r.timestamp)}
                  </span>
                  <button
                    onClick={() => restore(r.id)}
                    className="flex items-center gap-1 rounded-lg px-2 py-1 text-xs text-[var(--color-primary)] hover:bg-[var(--color-primary-soft)]"
                  >
                    <RotateCcw size={12} /> 回滚
                  </button>
                  <button
                    onClick={() => removeRecord(r.id)}
                    className="rounded-lg p-1 text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
