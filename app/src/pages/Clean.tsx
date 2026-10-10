import { useEffect, useState, useCallback } from "react";
import { api_clean } from "../api/commands";
import {
  Trash2, RefreshCw, AlertTriangle, FolderX, Layers, X, MessageSquare,
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
  const [deepSelected, setDeepSelected] = useState<Set<string>>(new Set());
  // 社交缓存状态
  const [social, setSocial] = useState<import("../api/commands").SocialScan | null>(null);
  const [socialDeleting, setSocialDeleting] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const [scan, st, lo] = await Promise.all([
        api_clean.list_cleanup_items(),
        api_clean.get_cleanup_stats(),
        api_clean.scan_uninstall_leftovers(),
      ]);
      setItems(scan.items);
      setStats(st);
      setLeftovers(lo);
      setLeftoverSelected(new Set(lo.high.map((e) => e.path)));
      // 社交缓存独立加载（扫描较慢，失败不阻塞）
      api_clean.scan_social_cache().then(setSocial).catch(() => {});
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

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

  // ===== 深度扫描状态与操作 =====
  const runDeepScan = async () => {
    setDeepScanning(true);
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
      </div>
    </div>
  );
}
