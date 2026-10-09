import { Construction } from "lucide-react";

interface Props {
  id: string;
  title: string;
  subtitle: string;
}

/** P1 骨架占位页：模块导航就位、真实功能待接入 */
export default function ModulePlaceholder({ id, title, subtitle }: Props) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-4 p-10">
      <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]">
        <Construction size={32} />
      </div>
      <div className="text-center">
        <h1 className="text-xl font-semibold">{title}</h1>
        <p className="mt-1 text-sm text-[var(--color-text-secondary)]">{subtitle}</p>
      </div>
      <div className="rounded-xl border border-dashed border-[var(--color-line)] px-4 py-2 text-xs text-[var(--color-text-secondary)]">
        模块 <code className="mx-1 rounded bg-[var(--color-surface-hover)] px-1.5 py-0.5">{id}</code>
        已挂载 · 待 P2+ 接入后端命令
      </div>
    </div>
  );
}
