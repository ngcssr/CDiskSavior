import { useEffect, useState, useCallback } from "react";
import { api_clean } from "../api/commands";
import {
  Trash2, RefreshCw, ShieldCheck, AlertTriangle, FolderX,
} from "lucide-react";

type CleanupItem = {
  id: string; name: string; description: string; size: number;
  path: string; category: string; safety: string;
};
type CleanupStats = { totalFreed: number; totalRuns: number; lastAt: number };
type OrphanEntry = { path: string; appName: string; size: number; note?: string };

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
  const [orphans, setOrphans] = useState<OrphanEntry[]>([]);
  const [scanning, setScanning] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [result, setResult] = useState<{ freed: number; errors: string[]; skipped: string[] } | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const [scan, st, orph] = await Promise.all([
        api_clean.list_cleanup_items(),
        api_clean.get_cleanup_stats(),
        api_clean.scan_orphan_dirs(),
      ]);
      setItems(scan.items);
      setStats(st);
      setOrphans(orph.confirmed);
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

  const cleanOrphans = async () => {
    setCleaning(true);
    try {
      const r = await api_clean.clean_orphan_dirs(orphans.map((o) => o.path));
      setResult({ freed: r.freed, errors: r.errors, skipped: [] });
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCleaning(false);
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

        {/* 孤儿目录 */}
        {orphans.length > 0 && (
          <section>
            <div className="mb-3 flex items-center justify-between">
              <h2 className="flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
                <FolderX size={14} /> 孤儿残留
              </h2>
              <button
                onClick={cleanOrphans}
                disabled={cleaning}
                className="flex items-center gap-1.5 rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)] disabled:opacity-50"
              >
                <Trash2 size={14} />
                全部清理
              </button>
            </div>
            <div className="space-y-1">
              {orphans.map((o) => (
                <div key={o.path} className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
                  <ShieldCheck size={16} className="text-[var(--color-caution)]" />
                  <div className="flex-1">
                    <div className="text-sm font-medium">{o.appName}</div>
                    <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{o.path}</div>
                  </div>
                  <span className="text-sm tabular-nums text-[var(--color-text-secondary)]">{fmt(o.size)}</span>
                </div>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
