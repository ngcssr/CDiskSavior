import { useEffect, useState, useCallback } from "react";
import { api_tune } from "../api/commands";
import { Rocket, ToggleLeft, ToggleRight, Shield } from "lucide-react";

import type { StartupItem } from "../api/commands";

const ADVICE_STYLE: Record<string, { color: string; label: string }> = {
  disable: { color: "var(--color-caution)", label: "建议禁用" },
  keep: { color: "var(--color-safe)", label: "建议保留" },
  neutral: { color: "var(--color-text-secondary)", label: "看习惯" },
};

export default function Tune() {
  const [items, setItems] = useState<StartupItem[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await api_tune.list_startup_items();
      setItems(list ?? []);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  const toggle = async (item: StartupItem) => {
    try {
      await api_tune.set_startup_enabled(item.id, !item.enabled);
      setItems(items.map(i => i.id === item.id ? { ...i, enabled: !i.enabled } : i));
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <Rocket size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">启动与内存</h1>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto p-6">
        {error && <div className="mb-4 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">{error}</div>}
        {loading ? <div className="text-center text-[var(--color-text-secondary)]">加载中…</div> : (
          <div className="space-y-1">
            {items.map(item => {
              const st = ADVICE_STYLE[item.advice] ?? ADVICE_STYLE.neutral;
              return (
                <div key={item.id} className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2.5">
                  <button onClick={() => toggle(item)} className="shrink-0">
                    {item.enabled
                      ? <ToggleRight size={24} className="text-[var(--color-safe)]" />
                      : <ToggleLeft size={24} className="text-[var(--color-text-secondary)]" />}
                  </button>
                  <div className="flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium">{item.name}</span>
                      <span className="rounded-full px-2 py-0.5 text-xs" style={{ background: `${st.color}15`, color: st.color }}>{st.label}</span>
                      {item.needsAdmin && <span className="flex items-center gap-0.5 text-xs text-[var(--color-caution)]"><Shield size={10} />需管理员</span>}
                    </div>
                    <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{item.command}</div>
                    {item.reason && <div className="text-xs text-[var(--color-text-secondary)]">{item.reason}</div>}
                  </div>
                  <span className="text-xs text-[var(--color-text-secondary)]">{item.location}</span>
                </div>
              );
            })}
            {items.length === 0 && <div className="py-8 text-center text-sm text-[var(--color-text-secondary)]">无启动项</div>}
          </div>
        )}
      </div>
    </div>
  );
}
