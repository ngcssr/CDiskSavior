import { useEffect, useState } from "react";
import { api_dashboard } from "../api/commands";
import { HardDrive, MemoryStick } from "lucide-react";

type DriveInfo = { mount: string; label: string; total: number; free: number; fs: string; kind: string };

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

export default function Dashboard() {
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [mem, setMem] = useState<{ total: number; available: number; usedPercent: number; swapTotal?: number } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function load() {
      try {
        const [d, m] = await Promise.all([
          api_dashboard.get_drives(),
          api_dashboard.memory_report(),
        ]);
        setDrives(d);
        setMem(m.overview);
      } catch (e: any) {
        setError(e?.message ?? String(e));
      } finally {
        setLoading(false);
      }
    }
    load();
  }, []);

  if (loading) return <div className="p-10 text-[var(--color-text-secondary)]">加载中…</div>;
  if (error) return <div className="p-10 text-[var(--color-danger)]">错误：{error}</div>;

  return (
    <div className="p-6">
      <h1 className="mb-6 text-2xl font-bold">总览</h1>

      {/* 磁盘卡片 */}
      <section className="mb-8">
        <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">磁盘</h2>
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-3">
          {drives.map((d) => {
            const used = d.total - d.free;
            const pct = d.total > 0 ? (used / d.total) * 100 : 0;
            const barColor = pct > 90 ? "var(--color-danger)" : pct > 75 ? "var(--color-caution)" : "var(--color-safe)";
            return (
              <div key={d.mount} className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-card)]">
                <div className="mb-3 flex items-center gap-2">
                  <HardDrive size={18} className="text-[var(--color-primary)]" />
                  <span className="font-semibold">{d.mount}</span>
                  {d.label && <span className="text-xs text-[var(--color-text-secondary)]">({d.label})</span>}
                </div>
                <div className="mb-2 h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                  <div className="h-full rounded-full transition-all" style={{ width: `${pct}%`, background: barColor }} />
                </div>
                <div className="flex justify-between text-xs text-[var(--color-text-secondary)]">
                  <span>{formatSize(used)} 已用</span>
                  <span>{formatSize(d.free)} 可用</span>
                </div>
                <div className="mt-1 text-xs text-[var(--color-text-secondary)]">
                  {formatSize(d.total)} · {d.fs} · {d.kind}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* 内存卡片 */}
      {mem && (
        <section>
          <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">内存</h2>
          <div className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-card)]">
            <div className="mb-3 flex items-center gap-2">
              <MemoryStick size={18} className="text-[var(--color-primary)]" />
              <span className="font-semibold">物理内存</span>
            </div>
            <div className="mb-2 h-2 overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
              <div
                className="h-full rounded-full bg-[var(--color-primary)] transition-all"
                style={{ width: `${mem.usedPercent}%` }}
              />
            </div>
            <div className="flex justify-between text-xs text-[var(--color-text-secondary)]">
              <span>{formatSize(mem.total - mem.available)} 已用 ({mem.usedPercent}%)</span>
              <span>{formatSize(mem.available)} 可用</span>
            </div>
            <div className="mt-1 text-xs text-[var(--color-text-secondary)]">
              总计 {formatSize(mem.total)}
              {mem.swapTotal && mem.swapTotal > 0 ? ` · 虚拟内存 ${formatSize(mem.swapTotal)}` : ""}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
