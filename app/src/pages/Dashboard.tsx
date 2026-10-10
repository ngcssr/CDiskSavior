import { useEffect, useState } from "react";
import { api_dashboard } from "../api/commands";
import { HardDrive, MemoryStick, RefreshCw, Zap } from "lucide-react";

type DriveInfo = { mount: string; label: string; total: number; free: number; fs: string; kind: string };

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function barClass(pct: number): string {
  if (pct > 90) return "bar bar-danger";
  if (pct > 75) return "bar bar-caution";
  return "bar bar-cyan";
}

export default function Dashboard() {
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [mem, setMem] = useState<{ total: number; available: number; usedPercent: number; swapTotal?: number } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
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
  };

  useEffect(() => { load(); }, []);

  // 主盘（C:）使用率作为记忆点大数字
  const systemDrive = drives.find((d) => d.mount.startsWith("C:")) ?? drives[0];
  const sysPct = systemDrive && systemDrive.total > 0
    ? Math.round(((systemDrive.total - systemDrive.free) / systemDrive.total) * 100)
    : 0;

  return (
    <div className="p-6">
      {/* 标题栏 */}
      <div className="mb-5 flex items-center justify-between rise">
        <div>
          <h1 className="font-display text-[22px] font-bold tracking-tight">总览</h1>
          <p className="mt-0.5 text-[12px] text-[var(--color-text-dim)]">磁盘容量 · 内存占用 · 实时状态</p>
        </div>
        <button onClick={load} disabled={loading} className="btn">
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
          刷新
        </button>
      </div>

      {error && (
        <div className="mb-4 rounded-[var(--radius-m)] border px-4 py-3 text-[13px]" style={{ borderColor: "rgba(248,113,113,0.3)", background: "var(--color-danger-bg-soft)", color: "var(--color-danger-text)" }}>
          {error}
        </div>
      )}

      {/* 记忆点：系统盘大数字 */}
      {systemDrive && (
        <section className="card card-hover rise rise-1 mb-5 p-5">
          <div className="flex items-end justify-between gap-6">
            <div>
              <div className="flex items-center gap-2 text-[12px] font-medium text-[var(--color-text-secondary)]">
                <Zap size={13} style={{ color: "var(--color-primary)" }} />
                系统盘 {systemDrive.mount} {systemDrive.label && `· ${systemDrive.label}`}
              </div>
              <div className="font-num mt-2 flex items-baseline gap-2">
                <span
                  className="font-display text-[52px] font-bold leading-none tracking-tight"
                  style={{ color: sysPct > 90 ? "var(--color-danger)" : sysPct > 75 ? "var(--color-caution)" : "var(--color-primary)" }}
                >
                  {sysPct}
                </span>
                <span className="text-[18px] text-[var(--color-text-secondary)]">% 已用</span>
              </div>
              <div className="font-num mt-1.5 text-[12px] text-[var(--color-text-secondary)]">
                {formatSize(systemDrive.total - systemDrive.free)} / {formatSize(systemDrive.total)} · 剩余 {formatSize(systemDrive.free)}
              </div>
            </div>
            <div className="w-48 shrink-0">
              <div className={barClass(sysPct)} style={{ height: 8 }}>
                <div className="bar-fill" style={{ width: `${sysPct}%` }} />
              </div>
              {sysPct > 75 && (
                <div className="mt-2 text-[11px] font-medium" style={{ color: sysPct > 90 ? "var(--color-danger)" : "var(--color-caution)" }}>
                  {sysPct > 90 ? "空间告急，建议立即清理" : "空间紧张，建议腾挪大文件夹"}
                </div>
              )}
            </div>
          </div>
        </section>
      )}

      {/* 磁盘卡片网格 */}
      <section className="mb-5">
        <h2 className="mb-3 text-[12px] font-semibold uppercase tracking-wider text-[var(--color-text-dim)] rise rise-2">全部磁盘</h2>
        <div className="grid grid-cols-2 gap-3.5 lg:grid-cols-3">
          {drives.map((d, i) => {
            const used = d.total - d.free;
            const pct = d.total > 0 ? (used / d.total) * 100 : 0;
            return (
              <div key={d.mount} className={`card card-hover rise rise-${Math.min(i + 2, 4)} p-4`}>
                <div className="mb-2.5 flex items-center gap-2">
                  <HardDrive size={15} style={{ color: "var(--color-primary)" }} />
                  <span className="font-display text-[14px] font-semibold">{d.mount}</span>
                  {d.label && <span className="truncate text-[11px] text-[var(--color-text-dim)]">{d.label}</span>}
                </div>
                <div className={barClass(pct)}>
                  <div className="bar-fill" style={{ width: `${pct}%` }} />
                </div>
                <div className="font-num mt-2 flex justify-between text-[11px] text-[var(--color-text-secondary)]">
                  <span>{formatSize(used)} 已用</span>
                  <span style={{ color: d.free < d.total * 0.1 ? "var(--color-danger)" : "inherit" }}>
                    {formatSize(d.free)} 可用
                  </span>
                </div>
                <div className="mt-1 text-[10px] text-[var(--color-text-dim)]">
                  {formatSize(d.total)} · {d.fs} · {d.kind}
                </div>
              </div>
            );
          })}
        </div>
      </section>

      {/* 内存 */}
      {mem && (
        <section className="rise rise-3">
          <h2 className="mb-3 text-[12px] font-semibold uppercase tracking-wider text-[var(--color-text-dim)]">内存</h2>
          <div className="card card-hover p-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-2">
                <MemoryStick size={15} style={{ color: "var(--color-primary)" }} />
                <span className="font-display text-[14px] font-semibold">物理内存</span>
              </div>
              <span className="font-num text-[15px] font-semibold" style={{ color: "var(--color-primary)" }}>
                {mem.usedPercent}%
              </span>
            </div>
            <div className="bar bar-cyan mt-3">
              <div className="bar-fill" style={{ width: `${mem.usedPercent}%` }} />
            </div>
            <div className="font-num mt-2 flex justify-between text-[11px] text-[var(--color-text-secondary)]">
              <span>{formatSize(mem.total - mem.available)} 已用</span>
              <span>{formatSize(mem.available)} 可用</span>
            </div>
            <div className="font-num mt-1 text-[10px] text-[var(--color-text-dim)]">
              总计 {formatSize(mem.total)}
              {mem.swapTotal && mem.swapTotal > 0 ? ` · 虚拟内存 ${formatSize(mem.swapTotal)}` : ""}
            </div>
          </div>
        </section>
      )}

      {loading && drives.length === 0 && (
        <div className="pulse p-10 text-center text-[13px] text-[var(--color-text-secondary)]">加载中…</div>
      )}
    </div>
  );
}
