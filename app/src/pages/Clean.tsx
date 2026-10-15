import { useEffect, useState, useCallback } from "react";
import { api_clean } from "../api/commands";
import { listen } from "@tauri-apps/api/event";
import {
  Trash2, RefreshCw, AlertTriangle, FolderX, Layers, X, MessageSquare,
  ShieldCheck, Power, PackageX, EyeOff, Eye, ExternalLink, Bomb,
} from "lucide-react";

type CleanupItem = {
  id: string; name: string; description: string; size: number;
  path: string; category: string; safety: string;
};
type CleanupStats = { totalFreed: number; totalRuns: number; lastAt: number };

function fmt(bytes: number): string {
  if (!bytes) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${u[i]}`;
}

function fmtTime(ts: number): string {
  if (!ts) return "从未";
  return new Date(ts * 1000).toLocaleString("zh-CN");
}

const SAFETY_STYLE: Record<string, { color: string; label: string }> = {
  safe: { color: "var(--color-safe)", label: "安全" },
  caution: { color: "var(--color-caution)", label: "需提权" },
  keep: { color: "var(--color-danger)", label: "保留" },
};

/** 类别展示顺序与名称（与后端 46 项清单对应） */
const CATEGORY_ORDER = ["system", "update", "browser", "dev", "app", "logs", "recycle"] as const;
const CATEGORY_NAMES: Record<string, string> = {
  system: "系统与错误报告",
  update: "更新与组件",
  browser: "浏览器缓存",
  dev: "开发工具缓存",
  app: "应用缓存",
  logs: "日志与杂项",
  recycle: "回收站",
};

export default function Clean() {
  const [items, setItems] = useState<CleanupItem[]>([]);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [stats, setStats] = useState<CleanupStats | null>(null);
  // 卸载残留（置信度评分引擎）
  const [leftovers, setLeftovers] = useState<import("../api/commands").LeftoverScan | null>(null);
  const [leftoverSelected, setLeftoverSelected] = useState<Set<string>>(new Set());
  const [scanning, setScanning] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [result, setResult] = useState<{ freed: number; errors: string[]; skipped: string[] } | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 深度扫描状态
  const [deep, setDeep] = useState<import("../api/commands").DeepScanResult | null>(null);
  const [deepScanning, setDeepScanning] = useState(false);
  const [deepProgress, setDeepProgress] = useState<{ category: string; done: number; total: number } | null>(null);
  const [deepSelected, setDeepSelected] = useState<Set<string>>(new Set());
  // 软件体检状态（Q4）
  const [bloat, setBloat] = useState<import("../api/commands").BloatwareScan | null>(null);
  const [bloatLoading, setBloatLoading] = useState(false);
  const [includeAll, setIncludeAll] = useState(false);
  const [bloatBusy, setBloatBusy] = useState<string | null>(null);
  const [forcePlan, setForcePlan] = useState<import("../api/commands").ForcePlan | null>(null);
  const [residue, setResidue] = useState<import("../api/commands").ResidueScanReport | null>(null);
  const [residueTarget, setResidueTarget] = useState<import("../api/commands").BloatwareEntry | null>(null);
  // 社交缓存状态
  const [social, setSocial] = useState<import("../api/commands").SocialScan | null>(null);
  const [socialDeleting, setSocialDeleting] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      // 主视图（清理项清单 + 统计）立即返回，不阻塞切页；
      // 残留扫描（注册表遍历）+ 社交缓存扫描较慢，独立后台加载
      const [scan, st] = await Promise.all([
        api_clean.list_cleanup_items(),
        api_clean.get_cleanup_stats(),
      ]);
      setItems(scan.items);
      setStats(st);
      api_clean.scan_uninstall_leftovers().then((lo) => {
        setLeftovers(lo);
        setLeftoverSelected(new Set(lo.high.map((e) => e.path)));
      }).catch(() => {});
      api_clean.scan_social_cache().then(setSocial).catch(() => {});
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // 订阅深度扫描分类级进度事件
  useEffect(() => {
    const un = listen<{ category: string; done: number; total: number }>("deep-junk:progress", (e) => {
      setDeepProgress(e.payload);
    });
    return () => { un.then((f) => f()); };
  }, []);

  const toggle = (id: string) => {
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id); else n.add(id);
      return n;
    });
  };

  const toggleGroup = (cat: string) => {
    setSelected((s) => {
      const n = new Set(s);
      const group = items.filter((i) => i.category === cat);
      const allIn = group.every((i) => n.has(i.id));
      group.forEach((i) => (allIn ? n.delete(i.id) : n.add(i.id)));
      return n;
    });
  };

  // ===== 软件体检（Q4）操作 =====
  const runBloatScan = useCallback(async (all: boolean) => {
    setBloatLoading(true);
    setError(null);
    try {
      setBloat(await api_clean.scan_bloatware(all));
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBloatLoading(false);
    }
  }, []);

  const bloatAction = async (_id: string, fn: () => Promise<unknown>, label: string, reload = true) => {
    setBloatBusy(label);
    setError(null);
    try {
      await fn();
      setResult({ freed: 0, errors: [], skipped: [] });
      if (reload) await runBloatScan(includeAll);
    } catch (e: any) {
      setError(`${label}：${e?.message ?? e}`);
    } finally {
      setBloatBusy(null);
    }
  };

  const previewForce = async (id: string) => {
    setBloatBusy("强力卸载预览");
    setError(null);
    try {
      setForcePlan(await api_clean.bloatware_force_preview(id));
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBloatBusy(null);
    }
  };

  const scanResidueFor = async (e: import("../api/commands").BloatwareEntry) => {
    setBloatBusy("扫描残留");
    setResidueTarget(e);
    setError(null);
    try {
      setResidue(await api_clean.scan_residue(e.name, e.publisher, e.installLocation));
    } catch (err: any) {
      setError(err?.message ?? String(err));
    } finally {
      setBloatBusy(null);
    }
  };

  const cleanResidue = async () => {
    if (!residueTarget || !residue) return;
    setBloatBusy("清理残留");
    setError(null);
    try {
      const r = await api_clean.clean_residue(
        residueTarget.name, residueTarget.publisher, residueTarget.installLocation,
        residue.dirs.map((d) => d.path), residue.regKeys
      );
      setResult({ freed: r.freed, errors: r.errors, skipped: [] });
      setResidue(null);
      setResidueTarget(null);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBloatBusy(null);
    }
  };

  // ===== 深度扫描状态与操作 =====
  const runDeepScan = async () => {
    setDeepScanning(true);
    setDeepProgress(null);
    setDeep(null);
    setDeepSelected(new Set());
    setError(null);
    try {
      const r = await api_clean.scan_deep_junk_files();
      setDeep(r);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setDeepScanning(false);
      setDeepProgress(null);
    }
  };

  const cancelDeep = async () => {
    await api_clean.cancel_deep_junk_scan();
  };

  const toggleDeep = (id: string) => {
    setDeepSelected((s) => {
      const n = new Set(s);
      if (n.has(id)) n.delete(id); else n.add(id);
      return n;
    });
  };

  const doDeepDelete = async () => {
    setCleaning(true);
    setResult(null);
    try {
      const r = await api_clean.delete_deep_junk_files(Array.from(deepSelected));
      setResult({ freed: r.freed, errors: r.errors, skipped: [] });
      setDeepSelected(new Set());
      // 重扫深度结果 + 快速列表
      const [d2] = await Promise.all([api_clean.scan_deep_junk_files(), refresh()]);
      setDeep(d2);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
    }
  };

  const doClean = async () => {
    setCleaning(true);
    setResult(null);
    try {
      const r = await api_clean.run_cleanup(Array.from(selected));
      setResult({ freed: r.freed, errors: r.errors, skipped: r.skipped });
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
    }
  };

  const toggleLeftover = (path: string) => {
    setLeftoverSelected((s) => {
      const n = new Set(s);
      if (n.has(path)) n.delete(path); else n.add(path);
      return n;
    });
  };

  const deleteLeftovers = async () => {
    setCleaning(true);
    try {
      const r = await api_clean.delete_leftovers(Array.from(leftoverSelected));
      setResult({ freed: r.freed, errors: r.errors, skipped: r.skipped });
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
    }
  };

  const protectLeftover = async (name: string) => {
    try {
      await api_clean.add_leftover_whitelist_entry(name);
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  // 社交缓存删除（按应用 + 类别；数据库类别后端硬拒绝）
  const deleteSocial = async (app: string, categories: string[]) => {
    setSocialDeleting(app);
    try {
      const r = await api_clean.delete_social_cache(app, categories);
      setResult({ freed: r.freed, errors: r.errors, skipped: [] });
      const s2 = await api_clean.scan_social_cache();
      setSocial(s2);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setSocialDeleting(null);
    }
  };

  const totalSelected = Array.from(selected).reduce(
    (sum, id) => sum + (items.find((i) => i.id === id)?.size ?? 0), 0,
  );

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <Trash2 size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">一键清理</h1>
        </div>
        <button
          onClick={refresh}
          disabled={scanning || cleaning}
          className="flex items-center gap-1.5 rounded-lg bg-[var(--color-surface)] px-3 py-1.5 text-sm border border-[var(--color-line)] disabled:opacity-50"
        >
          <RefreshCw size={14} className={scanning ? "animate-spin" : ""} />
          重新扫描
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {error && (
          <div className="mb-4 flex items-center gap-2 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">
            <AlertTriangle size={16} /> {error}
          </div>
        )}

        {result && (
          <div className="mb-4 rounded-xl border border-[var(--color-safe)]/30 bg-[var(--color-safe)]/5 px-4 py-3 text-sm">
            ✓ 清理完成：释放 <strong>{fmt(result.freed)}</strong>
            {result.skipped.length > 0 && `（跳过 ${result.skipped.length} 项）`}
            {result.errors.length > 0 && `（错误 ${result.errors.length} 项）`}
          </div>
        )}

        {/* 统计 */}
        {stats && (
          <div className="mb-6 flex gap-4">
            <div className="flex-1 rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
              <div className="text-xs text-[var(--color-text-secondary)]">累计释放</div>
              <div className="text-lg font-bold text-[var(--color-safe)]">{fmt(stats.totalFreed)}</div>
            </div>
            <div className="flex-1 rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
              <div className="text-xs text-[var(--color-text-secondary)]">清理次数</div>
              <div className="text-lg font-bold">{stats.totalRuns}</div>
            </div>
            <div className="flex-1 rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
              <div className="text-xs text-[var(--color-text-secondary)]">上次清理</div>
              <div className="text-sm font-medium">{fmtTime(stats.lastAt)}</div>
            </div>
          </div>
        )}

        {/* 清理项列表 */}
        <section className="mb-6">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-sm font-semibold text-[var(--color-text-secondary)]">清理项</h2>
            {selected.size > 0 && (
              <button
                onClick={doClean}
                disabled={cleaning}
                className="flex items-center gap-1.5 rounded-lg bg-[var(--color-primary)] px-4 py-1.5 text-sm font-medium text-white disabled:opacity-50"
              >
                <Trash2 size={14} />
                清理 {selected.size} 项（{fmt(totalSelected)}）
              </button>
            )}
          </div>
          {/* 按类别分组展示（46 项合并清单） */}
          <div className="space-y-4">
            {CATEGORY_ORDER.map((cat) => {
              const group = items.filter((i) => i.category === cat);
              if (group.length === 0) return null;
              const groupSize = group.reduce((s, i) => s + i.size, 0);
              const groupSelected = group.filter((i) => selected.has(i.id)).length;
              return (
                <div key={cat}>
                  <div className="mb-1.5 flex items-center justify-between px-1">
                    <div className="flex items-center gap-2">
                      <span className="text-[12px] font-semibold uppercase tracking-wider text-[var(--color-text-dim)]">
                        {CATEGORY_NAMES[cat]}
                      </span>
                      <span className="text-[11px] text-[var(--color-text-dim)]">
                        {group.length} 项 · {fmt(groupSize)}
                      </span>
                    </div>
                    <button
                      onClick={() => toggleGroup(cat)}
                      className="text-[11px] text-[var(--color-primary)] hover:underline"
                    >
                      {groupSelected === group.length ? "取消全选" : "全选"}
                    </button>
                  </div>
                  <div className="space-y-0.5">
                    {group.map((item) => {
                      const st = SAFETY_STYLE[item.safety] ?? SAFETY_STYLE.safe;
                      return (
                        <label
                          key={item.id}
                          className={`flex cursor-pointer items-center gap-3 rounded-lg px-3 py-2 hover:bg-[var(--color-surface-hover)] ${
                            selected.has(item.id) ? "bg-[var(--color-primary-soft)]" : ""
                          }`}
                        >
                          <input
                            type="checkbox"
                            checked={selected.has(item.id)}
                            onChange={() => toggle(item.id)}
                            disabled={cleaning}
                            className="h-4 w-4 accent-[var(--color-primary)]"
                          />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="text-sm font-medium">{item.name}</span>
                              <span className="rounded-full px-2 py-0.5 text-xs" style={{ background: `${st.color}15`, color: st.color }}>
                                {st.label}
                              </span>
                            </div>
                            <div className="truncate text-xs text-[var(--color-text-secondary)]">{item.description}</div>
                          </div>
                          <span className="font-num shrink-0 text-sm text-[var(--color-text-secondary)]">{fmt(item.size)}</span>
                        </label>
                      );
                    })}
                  </div>
                </div>
              );
            })}
            {items.length === 0 && !scanning && (
              <div className="py-8 text-center text-sm text-[var(--color-text-secondary)]">无可清理项</div>
            )}
          </div>
        </section>

        {/* 深度垃圾扫描（light-c deep_junk 移植） */}
        <section className="mb-6">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
              <Layers size={14} /> 深度扫描（19 类文件级匹配）
            </h2>
            {deepScanning ? (
              <button onClick={cancelDeep} className="flex items-center gap-1.5 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)]">
                <X size={14} /> 取消扫描
              </button>
            ) : (
              <button onClick={runDeepScan} disabled={cleaning} className="flex items-center gap-1.5 rounded-lg bg-[var(--color-primary)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50">
                <Layers size={14} /> 开始深度扫描
              </button>
            )}
          </div>

          {deepScanning && (
            <div className="pulse rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3 text-sm text-[var(--color-text-secondary)]">
              正在全分区深度扫描（19 类，分钟级）… 可随时取消
              {deepProgress && deepProgress.total > 0 && (
                <div className="mt-1 text-xs">
                  <span className="font-num">{deepProgress.done}/{deepProgress.total}</span>
                  <span className="ml-2">{deepProgress.category}</span>
                </div>
              )}
            </div>
          )}

          {deep && !deepScanning && (
            <>
              <div className="mb-3 flex items-center justify-between rounded-xl bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
                <span>
                  {deep.cancelled ? "扫描已取消（部分结果）" : `扫描完成（${(deep.durationMs / 1000).toFixed(1)}s）`}
                  ：{deep.categories.length} 类 · {deep.totalFiles.toLocaleString()} 个文件 · 共 <strong>{fmt(deep.totalSize)}</strong>
                </span>
                {deepSelected.size > 0 && (
                  <button onClick={doDeepDelete} disabled={cleaning} className="flex items-center gap-1.5 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)] disabled:opacity-50">
                    <Trash2 size={14} /> 删除 {deepSelected.size} 类
                  </button>
                )}
              </div>
              <div className="space-y-1">
                {deep.categories.map((c) => (
                  <label
                    key={c.id}
                    className={`flex items-center gap-3 rounded-lg px-3 py-2 ${
                      c.systemManaged ? "opacity-60" : "cursor-pointer hover:bg-[var(--color-surface-hover)]"
                    } ${deepSelected.has(c.id) ? "bg-[var(--color-primary-soft)]" : ""}`}
                  >
                    <input
                      type="checkbox"
                      checked={deepSelected.has(c.id)}
                      onChange={() => toggleDeep(c.id)}
                      disabled={cleaning || c.systemManaged}
                      className="h-4 w-4 accent-[var(--color-primary)]"
                    />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium">{c.name}</span>
                        <span className="rounded-full px-2 py-0.5 text-[10px]" style={{
                          background: c.risk >= 3 ? "rgba(248,113,113,0.12)" : c.risk === 2 ? "rgba(251,191,36,0.12)" : "rgba(52,211,153,0.12)",
                          color: c.risk >= 3 ? "var(--color-danger)" : c.risk === 2 ? "var(--color-caution)" : "var(--color-safe)",
                        }}>
                          {c.risk >= 3 ? "谨慎" : c.risk === 2 ? "低风险" : "安全"}
                        </span>
                        {c.systemManaged && <span className="text-[10px] text-[var(--color-caution)]">系统托管</span>}
                      </div>
                      <div className="truncate text-xs text-[var(--color-text-secondary)]">{c.description}</div>
                    </div>
                    <div className="shrink-0 text-right">
                      <div className="font-num text-sm text-[var(--color-text-secondary)]">{fmt(c.size)}</div>
                      <div className="text-[10px] text-[var(--color-text-dim)]">{c.fileCount.toLocaleString()} 个文件</div>
                    </div>
                  </label>
                ))}
              </div>
            </>
          )}
        </section>

        {/* 卸载残留（置信度评分引擎） */}
        {leftovers && (leftovers.high.length > 0 || leftovers.suspicious.length > 0) && (
          <section>
            <div className="mb-3 flex items-center justify-between">
              <h2 className="flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
                <FolderX size={14} /> 卸载残留（评分引擎）
              </h2>
              {leftoverSelected.size > 0 && (
                <button
                  onClick={deleteLeftovers}
                  disabled={cleaning}
                  className="flex items-center gap-1.5 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)] disabled:opacity-50"
                >
                  <Trash2 size={14} />
                  删除 {leftoverSelected.size} 项
                </button>
              )}
            </div>

            {/* 高置信度（默认勾选） */}
            {leftovers.high.length > 0 && (
              <>
                <div className="mb-1.5 px-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--color-text-dim)]">
                  高置信度（{leftovers.high.length}）— 已默认勾选
                </div>
                <div className="mb-3 space-y-0.5">
                  {leftovers.high.map((e) => (
                    <div key={e.path} className={`flex items-center gap-3 rounded-lg px-3 py-2 ${leftoverSelected.has(e.path) ? "bg-[var(--color-primary-soft)]" : "hover:bg-[var(--color-surface-hover)]"}`}>
                      <input type="checkbox" checked={leftoverSelected.has(e.path)} onChange={() => toggleLeftover(e.path)} disabled={cleaning} className="h-4 w-4 accent-[var(--color-primary)]" />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium">{e.name}</span>
                          <span className="rounded-full bg-[var(--color-safe)]/10 px-2 py-0.5 text-[10px] text-[var(--color-safe)]">评分 {e.score.toFixed(2)}</span>
                        </div>
                        <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{e.path}</div>
                        <div className="truncate text-[10px] text-[var(--color-text-dim)]">{e.reasons.slice(0, 3).join(" · ")}</div>
                      </div>
                      <span className="font-num shrink-0 text-sm text-[var(--color-text-secondary)]">{fmt(e.size)}</span>
                    </div>
                  ))}
                </div>
              </>
            )}

            {/* 可疑（不默认勾选） */}
            {leftovers.suspicious.length > 0 && (
              <>
                <div className="mb-1.5 px-1 text-[11px] font-semibold uppercase tracking-wider text-[var(--color-text-dim)]">
                  可疑（{leftovers.suspicious.length}）— 请人工确认
                </div>
                <div className="space-y-0.5">
                  {leftovers.suspicious.map((e) => (
                    <div key={e.path} className={`flex items-center gap-3 rounded-lg px-3 py-2 ${leftoverSelected.has(e.path) ? "bg-[var(--color-primary-soft)]" : "hover:bg-[var(--color-surface-hover)]"}`}>
                      <input type="checkbox" checked={leftoverSelected.has(e.path)} onChange={() => toggleLeftover(e.path)} disabled={cleaning} className="h-4 w-4 accent-[var(--color-primary)]" />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium">{e.name}</span>
                          <span className="rounded-full bg-[var(--color-caution)]/10 px-2 py-0.5 text-[10px] text-[var(--color-caution)]">评分 {e.score.toFixed(2)}</span>
                        </div>
                        <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{e.path}</div>
                        <div className="truncate text-[10px] text-[var(--color-text-dim)]">{e.reasons.slice(0, 3).join(" · ")}</div>
                      </div>
                      <button
                        onClick={() => protectLeftover(e.name)}
                        className="shrink-0 rounded-lg px-2 py-1 text-[10px] text-[var(--color-primary)] hover:bg-[var(--color-primary-soft)]"
                        title="加入白名单（永不判为残留）"
                      >
                        保护
                      </button>
                      <span className="font-num shrink-0 text-sm text-[var(--color-text-secondary)]">{fmt(e.size)}</span>
                    </div>
                  ))}
                </div>
              </>
            )}
          </section>
        )}

        {/* 社交软件缓存（四级风险分类） */}
        {social && social.apps.length > 0 && (
          <section className="mb-6">
            <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
              <MessageSquare size={14} /> 社交软件缓存（共 {fmt(social.totalSize)}，可安全清理 {fmt(social.deletableSize)}）
            </h2>
            <div className="space-y-2">
              {social.apps.map((app) => {
                const deletableCats = app.categories.filter((c) => c.deletable);
                const safeCats = deletableCats.filter((c) => c.risk === "none" || c.risk === "low");
                const safeSize = safeCats.reduce((s, c) => s + c.size, 0);
                return (
                  <div key={app.app} className="card p-3.5">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-semibold">{app.app}</span>
                        <span className="font-num text-xs text-[var(--color-text-secondary)]">{fmt(app.totalSize)}</span>
                      </div>
                      {safeSize > 0 && (
                        <button
                          onClick={() => deleteSocial(app.app, safeCats.map((c) => c.category))}
                          disabled={socialDeleting !== null}
                          className="flex items-center gap-1.5 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1 text-xs text-[var(--color-danger-text)] disabled:opacity-50"
                        >
                          <Trash2 size={12} />
                          {socialDeleting === app.app ? "清理中…" : `清理缓存+媒体（${fmt(safeSize)}）`}
                        </button>
                      )}
                    </div>
                    <div className="mt-2 flex flex-wrap gap-1.5">
                      {app.categories.map((c) => (
                        <span
                          key={c.category}
                          className="chip"
                          title={c.risk === "critical" ? "聊天记录数据库，永不删除" : c.risk === "medium" ? "接收的文件，请手动确认" : "可清理"}
                          style={{
                            color: c.risk === "critical" ? "var(--color-danger)" : c.risk === "medium" ? "var(--color-caution)" : "var(--color-safe)",
                            borderColor: c.risk === "critical" ? "rgba(248,113,113,0.3)" : c.risk === "medium" ? "rgba(251,191,36,0.3)" : "rgba(52,211,153,0.3)",
                          }}
                        >
                          {c.category === "database" ? "聊天记录(保护)" : c.category === "files" ? "接收文件" : c.category === "media" ? "图片视频" : "缓存日志"}
                          {" "}{fmt(c.size)}
                        </span>
                      ))}
                    </div>
                  </div>
                );
              })}
            </div>
          </section>
        )}
        {/* ===== 软件体检（Q4：客观行为陈述，不定性） ===== */}
        <section className="mb-6">
          <div className="mb-3 flex items-center justify-between">
            <h2 className="flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
              <PackageX size={14} /> 软件体检
              {bloat && <span className="text-xs font-normal">（重点关注 {bloat.shownCount}）</span>}
            </h2>
            <div className="flex items-center gap-2">
              <label className="flex items-center gap-1 text-xs text-[var(--color-text-secondary)]">
                <input type="checkbox" checked={includeAll} onChange={(e) => setIncludeAll(e.target.checked)} />
                列出全部
              </label>
              <button
                onClick={() => runBloatScan(includeAll)}
                disabled={bloatLoading || bloatBusy !== null}
                className="flex items-center gap-1.5 rounded-lg bg-[var(--color-primary)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
              >
                <RefreshCw size={14} className={bloatLoading ? "animate-spin" : ""} />
                {bloatLoading ? "体检中…" : bloat ? "重新体检" : "开始体检"}
              </button>
            </div>
          </div>

          {bloat === null && !bloatLoading && (
            <div className="py-2 text-xs text-[var(--color-text-secondary)]">
              只陈述客观行为（开机自启 / 后台常驻 / 占用较大），不对任何软件定性；卸载一律调起软件自带官方卸载程序
            </div>
          )}

          {bloat && bloat.browserNotes.length > 0 && (
            <div className="mb-3 rounded-xl border border-[var(--color-caution)]/30 bg-[var(--color-caution)]/5 px-4 py-2 text-xs text-[var(--color-caution)]">
              {bloat.browserNotes.map((n, i) => <div key={i}>{n}</div>)}
            </div>
          )}

          {bloat && bloat.entries.length === 0 && (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">未发现需要关注的软件行为</div>
          )}

          {bloat && bloat.entries.length > 0 && (
            <div className="max-h-96 space-y-1 overflow-y-auto">
              {bloat.entries.map((e) => (
                <div key={e.id}
                  className={`flex items-start gap-3 rounded-lg border px-3 py-2 ${
                    e.dismissed
                      ? "border-[var(--color-line)] bg-[var(--color-surface-hover)] opacity-60"
                      : "border-[var(--color-line)] bg-[var(--color-surface)]"
                  }`}>
                  <div className="mt-0.5 h-8 w-8 shrink-0 overflow-hidden rounded-lg bg-[var(--color-surface-hover)]">
                    {e.icon ? <img src={e.icon} alt="" className="h-full w-full" /> : null}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex flex-wrap items-center gap-1.5 text-sm">
                      <span className="truncate font-medium">{e.name}</span>
                      {e.security && (
                        <span className="chip" style={{ color: "var(--color-caution)" }}><ShieldCheck size={10} className="mr-0.5 inline" />安全软件</span>
                      )}
                      {e.trusted && <span className="chip" style={{ color: "var(--color-text-dim)" }}>常用/系统</span>}
                      {e.tags.map((t) => (
                        <span key={t} className="chip" style={{ color: "var(--color-primary)" }}>{t}</span>
                      ))}
                      {e.dismissed && <span className="chip" style={{ color: "var(--color-text-dim)" }}>已忽略</span>}
                    </div>
                    <div className="mt-0.5 text-xs text-[var(--color-text-secondary)]">
                      {e.publisher || "未知发行商"}
                      {e.sizeMb != null && e.sizeMb > 0 && ` · ${e.sizeMb >= 1024 ? `${(e.sizeMb / 1024).toFixed(1)} GB` : `${e.sizeMb.toFixed(0)} MB`}`}
                      {e.version && ` · v${e.version}`}
                    </div>
                    {e.behaviors.map((b, i) => (
                      <div key={i} className="text-xs text-[var(--color-text-secondary)]">{b}</div>
                    ))}
                    <div className="mt-0.5 text-[11px] text-[var(--color-text-dim)]">{e.suggestion}</div>
                  </div>
                  <div className="flex shrink-0 flex-wrap items-center justify-end gap-1" style={{ maxWidth: 200 }}>
                    {e.uninstallable && (
                      <button
                        title="一键卸载（停后台+官方卸载器静默，需 UAC）"
                        disabled={bloatBusy !== null}
                        onClick={() => bloatAction(e.id, () => api_clean.uninstall_software(e.id), `卸载 ${e.name}`)}
                        className="rounded p-1 text-[var(--color-danger)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                    <button
                      title="前台打开官方卸载程序（安全软件推荐）"
                      disabled={bloatBusy !== null}
                      onClick={() => bloatAction(e.id, () => api_clean.open_official_uninstaller(e.id), `打开卸载程序 ${e.name}`, false)}
                      className="rounded p-1 hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                    >
                      <ExternalLink size={13} />
                    </button>
                    {e.residentMemMb > 0 && (
                      <button
                        title="停止后台运行（提权）"
                        disabled={bloatBusy !== null}
                        onClick={() => bloatAction(e.id, () => api_clean.stop_software(e.id), `停止 ${e.name}`)}
                        className="rounded p-1 text-[var(--color-caution)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                      >
                        <Power size={13} />
                      </button>
                    )}
                    <button
                      title="强力卸载（预览后红色确认）"
                      disabled={bloatBusy !== null}
                      onClick={() => previewForce(e.id)}
                      className="rounded p-1 text-[var(--color-danger)] hover:bg-[var(--color-danger-bg-soft)] disabled:opacity-50"
                    >
                      <Bomb size={13} />
                    </button>
                    <button
                      title="扫描卸载残留"
                      disabled={bloatBusy !== null}
                      onClick={() => scanResidueFor(e)}
                      className="rounded p-1 hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                    >
                      <FolderX size={13} />
                    </button>
                    <button
                      title={e.dismissed ? "恢复提醒" : "不再提醒"}
                      disabled={bloatBusy !== null}
                      onClick={() => bloatAction(e.id, () => api_clean.bloatware_set_ignored(e.key, !e.dismissed), e.dismissed ? `恢复 ${e.name}` : `忽略 ${e.name}`)}
                      className="rounded p-1 hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                    >
                      {e.dismissed ? <Eye size={13} /> : <EyeOff size={13} />}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* 强力卸载红色确认 */}
          {forcePlan && (
            <div className="mt-3 rounded-xl border-2 border-[var(--color-danger)]/50 bg-[var(--color-danger-bg-soft)] p-4">
              <div className="mb-2 flex items-center gap-2 text-sm font-semibold text-[var(--color-danger-text)]">
                <AlertTriangle size={14} /> 强力卸载确认：{forcePlan.name || "未知软件"}
              </div>
              <div className="mb-3 space-y-1 text-xs text-[var(--color-danger-text)]">
                <div>安装目录：{forcePlan.installDir || "未定位"}{forcePlan.dirDeletable ? "（将被删除）" : "（不可删，跳过）"}</div>
                {forcePlan.services.length > 0 && <div>删除服务：{forcePlan.services.join("、")}</div>}
                {forcePlan.tasks.length > 0 && <div>删除计划任务：{forcePlan.tasks.join("、")}</div>}
                <div>删除注册表项：{forcePlan.regPath || "无"}</div>
              </div>
              <div className="flex gap-2">
                <button
                  disabled={bloatBusy !== null}
                  onClick={() => {
                    const plan = forcePlan;
                    setForcePlan(null);
                    // 从当前体检结果中找回 id（name 匹配）
                    const entry = bloat?.entries.find((x) => x.name === plan.name);
                    if (entry) {
                      bloatAction(entry.id, () => api_clean.force_uninstall_software(entry.id), `强力卸载 ${plan.name}`);
                    } else {
                      setError("无法定位该软件（请重新体检后重试）");
                    }
                  }}
                  className="rounded-lg bg-[var(--color-danger)] px-4 py-1.5 text-sm font-medium text-white disabled:opacity-50"
                >
                  确认强力卸载（不可恢复）
                </button>
                <button onClick={() => setForcePlan(null)} className="btn text-sm">取消</button>
              </div>
            </div>
          )}

          {/* 残留清理确认 */}
          {residue && residueTarget && (
            <div className="mt-3 rounded-xl border border-[var(--color-caution)]/40 bg-[var(--color-caution)]/5 p-4">
              <div className="mb-2 text-sm font-semibold">「{residueTarget.name}」残留清单</div>
              {residue.dirs.length === 0 && residue.regKeys.length === 0 ? (
                <div className="text-xs text-[var(--color-text-secondary)]">未发现残留</div>
              ) : (
                <>
                  {residue.dirs.map((d) => (
                    <div key={d.path} className="flex justify-between font-mono text-xs">
                      <span className="truncate">{d.path}</span>
                      <span className="font-num shrink-0">{fmt(d.size)}</span>
                    </div>
                  ))}
                  {residue.regKeys.map((k) => (
                    <div key={k} className="font-mono text-xs">{k}</div>
                  ))}
                  <div className="mt-3 flex gap-2">
                    <button
                      disabled={bloatBusy !== null}
                      onClick={cleanResidue}
                      className="rounded-lg bg-[var(--color-caution)] px-4 py-1.5 text-sm font-medium text-black disabled:opacity-50"
                    >
                      清理全部残留
                    </button>
                    <button onClick={() => { setResidue(null); setResidueTarget(null); }} className="btn text-sm">取消</button>
                  </div>
                </>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
