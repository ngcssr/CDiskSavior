import { useAppStore } from "./store";
import { MODULES } from "./modules";
import { HardDriveDownload } from "lucide-react";

export default function App() {
  const { active, setActive, theme } = useAppStore();

  return (
    <div
      className="flex h-screen w-screen overflow-hidden bg-[var(--color-bg)] text-[var(--color-text)]"
      data-theme={theme}
    >
      {/* 左侧导航 */}
      <nav className="flex w-56 shrink-0 flex-col gap-1 border-r border-[var(--color-line)] bg-[var(--color-surface)] p-3">
        <div className="mb-4 flex items-center gap-2 px-2 py-3">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-[var(--color-primary)] text-white">
            <HardDriveDownload size={20} />
          </div>
          <div>
            <div className="text-sm font-bold leading-tight">CDisk Savior</div>
            <div className="text-[10px] text-[var(--color-text-secondary)]">磁盘与内存维护助手</div>
          </div>
        </div>

        {MODULES.map((m) => (
          <button
            key={m.id}
            onClick={() => setActive(m.id)}
            className={`flex items-center gap-3 rounded-xl px-3 py-2.5 text-left text-sm transition-colors ${
              active === m.id
                ? "bg-[var(--color-primary-soft)] font-medium text-[var(--color-primary)]"
                : "text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
            }`}
          >
            <m.icon size={18} />
            <span className="flex-1">{m.label}</span>
          </button>
        ))}

        <div className="mt-auto px-2 pb-1 text-[10px] text-[var(--color-text-secondary)]">
          v0.1.0 · P1 骨架
        </div>
      </nav>

      {/* 主区域 */}
      <main className="flex-1 overflow-y-auto">
        {MODULES.map((m) => (
          <div key={m.id} className={active === m.id ? "block h-full" : "hidden"}>
            <m.page />
          </div>
        ))}
      </main>
    </div>
  );
}
