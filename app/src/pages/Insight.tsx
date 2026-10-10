import { useEffect, useState, useCallback } from "react";
import { api_insight } from "../api/commands";
import { ChevronRight, RefreshCw, Folder } from "lucide-react";

type TreeNode = {
  name: string; path: string; size: number; isDir: boolean; hasChildren: boolean;
  category: string; safety: string; children?: TreeNode[];
};

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

// 颜色：按大小比例（红=大，绿=小）
function sizeColor(ratio: number): string {
  if (ratio > 0.5) return "#dc2626";
  if (ratio > 0.2) return "#d97706";
  if (ratio > 0.05) return "#3b82f6";
  return "#16a34a";
}

export default function Insight() {
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [stack, setStack] = useState<TreeNode[]>([]); // 面包屑路径
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scan = useCallback(async (path: string, pushToStack: boolean) => {
    setLoading(true);
    setError(null);
    try {
      const result = await api_insight.scan_drive(path);
      setTree(result);
      if (pushToStack) {
        // 下钻：压栈（面包屑累积）
        setStack((s) => [...s, { ...result, name: path }]);
      } else {
        // 回退/刷新：截断到该路径
        setStack((s) => {
          const idx = s.findIndex((n) => n.path === path);
          return idx >= 0 ? s.slice(0, idx + 1) : [{ ...result, name: path }];
        });
      }
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  // 首次加载：扫描 C:\
  useEffect(() => {
    scan("C:\\", false);
  }, [scan]);

  const drillInto = (node: TreeNode) => {
    if (!node.isDir) return;
    scan(node.path, true);
  };

  const goBack = (index: number) => {
    const target = stack[index];
    if (target) {
      scan(target.path, false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      {/* 工具栏 */}
      <div className="flex items-center gap-2 border-b border-[var(--color-line)] px-4 py-3">
        <button
          onClick={() => scan(stack[stack.length - 1]?.path ?? "C:\\", false)}
          disabled={loading}
          className="flex items-center gap-1.5 rounded-lg bg-[var(--color-primary)] px-3 py-1.5 text-sm font-medium text-white disabled:opacity-50"
        >
          <RefreshCw size={14} className={loading ? "animate-spin" : ""} />
          重新扫描
        </button>

        {/* 面包屑 */}
        <div className="flex flex-1 items-center gap-0.5 overflow-x-auto text-sm">
          {stack.map((node, i) => (
            <span key={i} className="flex items-center">
              {i > 0 && <ChevronRight size={14} className="text-[var(--color-text-secondary)]" />}
              <button
                onClick={() => goBack(i)}
                className="rounded px-1.5 py-0.5 hover:bg-[var(--color-surface-hover)]"
              >
                {node.name}
              </button>
            </span>
          ))}
        </div>
      </div>

      {/* 内容区 */}
      <div className="flex-1 overflow-y-auto p-4">
        {loading && <div className="text-center text-[var(--color-text-secondary)]">扫描中…</div>}
        {error && <div className="text-center text-[var(--color-danger)]">{error}</div>}

        {tree && !loading && (
          <>
            {/* TreeMap 简化版：水平条形图（P3 阶段；d3-hierarchy 在 P5 接入） */}
            <div className="mb-6">
              <h3 className="mb-2 text-xs font-semibold text-[var(--color-text-secondary)]">
                空间分布（按大小降序）
              </h3>
              <div className="space-y-1">
                {tree.children?.filter((c) => c.size > 0)
                  .slice(0, 30)
                  .map((child) => {
                    const ratio = tree.size > 0 ? child.size / tree.size : 0;
                    return (
                      <div
                        key={child.path}
                        onClick={() => drillInto(child)}
                        className="group flex cursor-pointer items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-[var(--color-surface-hover)]"
                      >
                        <div className="w-5">
                          {child.isDir ? (
                            <Folder size={14} className="text-[var(--color-primary)]" />
                          ) : null}
                        </div>
                        <div className="w-48 truncate text-sm">{child.name}</div>
                        <div className="relative h-5 flex-1 overflow-hidden rounded bg-[var(--color-surface-hover)]">
                          <div
                            className="h-full rounded transition-all"
                            style={{ width: `${Math.max(ratio * 100, 2)}%`, background: sizeColor(ratio) }}
                          />
                        </div>
                        <div className="w-20 text-right text-xs tabular-nums text-[var(--color-text-secondary)]">
                          {formatSize(child.size)}
                        </div>
                        <div className="w-12 text-right text-xs tabular-nums text-[var(--color-text-secondary)]">
                          {(ratio * 100).toFixed(1)}%
                        </div>
                      </div>
                    );
                  })}
              </div>
            </div>

            {/* 总计 */}
            <div className="rounded-xl bg-[var(--color-surface)] p-4 text-sm">
              <span className="text-[var(--color-text-secondary)]">当前目录：</span>
              <span className="font-mono">{tree.path}</span>
              <span className="mx-2 text-[var(--color-text-secondary)]">|</span>
              <span className="text-[var(--color-text-secondary)]">总计：</span>
              <span className="font-semibold">{formatSize(tree.size)}</span>
              <span className="mx-2 text-[var(--color-text-secondary)]">|</span>
              <span className="text-[var(--color-text-secondary)]">子项：</span>
              <span>{tree.children?.length ?? 0}</span>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
