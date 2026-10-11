import { useEffect, useState, useCallback, useMemo } from "react";
import { api_insight, api_settings, type LargeFileReport } from "../api/commands";
import { listen } from "@tauri-apps/api/event";
import { ChevronRight, RefreshCw, TrendingUp, FileWarning, FolderOpen, ExternalLink, X } from "lucide-react";
import { hierarchy, treemap, treemapSquarify } from "d3-hierarchy";

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

/** d3-hierarchy 真 TreeMap（面积 = 大小，点击下钻） */
function TreeMapView({ tree, onDrill }: { tree: TreeNode; onDrill: (n: TreeNode) => void }) {
  const W = 1000;
  const H = 420;
  const leaves = useMemo(() => {
    // 只渲染当前目录的一级子项（面积=大小），点击下钻后以新节点的子项重排。
    // 不可把整棵树喂给 d3 展开——服务树含 65 万文件级叶子，
    // 叶子级渲染会产生海量亚像素方块（碎点+空白）且卡死 UI 线程。
    const blocks = (tree.children ?? [])
      .filter((c) => c.size > 0)
      .map((c) => ({ ...c, children: undefined }));
    if (blocks.length === 0) return [];
    const root = hierarchy({ name: tree.name, size: 0, children: blocks } as any)
      .sum((d: any) => d.size)
      .sort((a, b) => (b.value ?? 0) - (a.value ?? 0));
    const layout = treemap<any>()
      .size([W, H])
      .tile(treemapSquarify.ratio(1.2))
      .paddingInner(3)
      .paddingOuter(2);
    return layout(root).leaves();
  }, [tree]);

  if (leaves.length === 0) {
    return <div className="py-6 text-center text-sm text-[var(--color-text-secondary)]">当前目录无可视化数据</div>;
  }

  const total = tree.size || 1;
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-auto w-full select-none" role="img" aria-label="磁盘空间 TreeMap">
      {leaves.map((leaf) => {
        const d = leaf.data as any;
        const w = leaf.x1 - leaf.x0;
        const h = leaf.y1 - leaf.y0;
        const ratio = (leaf.value ?? 0) / total;
        const showText = w > 70 && h > 30;
        const showSize = w > 90 && h > 48;
        return (
          <g
            key={d.path}
            onClick={() => {
              if (!d.isDir) return;
              // 副本被剥了 children，下钻必须回查原始节点
              const original = (tree.children ?? []).find((c) => c.path === d.path);
              if (original) onDrill(original as TreeNode);
            }}
            className={d.isDir ? "cursor-pointer" : ""}
          >
            <title>{`${d.path}\n${formatSize(leaf.value ?? 0)}（${(ratio * 100).toFixed(1)}%）`}</title>
            <rect
              x={leaf.x0}
              y={leaf.y0}
              width={Math.max(w, 0)}
              height={Math.max(h, 0)}
              rx={4}
              fill={sizeColor(ratio)}
              fillOpacity={d.isDir ? 0.85 : 0.45}
              stroke="var(--color-surface)"
              strokeWidth={1.5}
              className="transition-all hover:fillOpacity-100 hover:brightness-110"
            />
            {showText && (
              <text
                x={leaf.x0 + 6}
                y={leaf.y0 + 16}
                fontSize={12}
                fill="#fff"
                style={{ pointerEvents: "none", textShadow: "0 1px 2px rgba(0,0,0,.45)" }}
              >
                {d.name.length > Math.floor(w / 8) ? `${d.name.slice(0, Math.max(Math.floor(w / 8) - 1, 2))}…` : d.name}
              </text>
            )}
            {showSize && (
              <text
                x={leaf.x0 + 6}
                y={leaf.y0 + 32}
                fontSize={11}
                fill="#fff"
                fillOpacity={0.85}
                style={{ pointerEvents: "none", textShadow: "0 1px 2px rgba(0,0,0,.45)" }}
              >
                {formatSize(leaf.value ?? 0)}
              </text>
            )}
          </g>
        );
      })}
    </svg>
  );
}

export default function Insight() {
  const [tree, setTree] = useState<TreeNode | null>(null);
  const [stack, setStack] = useState<TreeNode[]>([]); // 面包屑路径
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"tree" | "hotspot" | "growth" | "bigfile">("tree");
  // 热点
  const [hotspot, setHotspot] = useState<import("../api/commands").HotspotReport | null>(null);
  const [hotspotLoading, setHotspotLoading] = useState(false);
  // 增长
  const [growth, setGrowth] = useState<import("../api/commands").GrowthReport | null>(null);
  const [growthLoading, setGrowthLoading] = useState(false);
  const [hasBaseline, setHasBaseline] = useState(false);
  // 大文件
  const [bigfile, setBigfile] = useState<LargeFileReport | null>(null);
  const [bigfileLoading, setBigfileLoading] = useState(false);
  const [bigfileMin, setBigfileMin] = useState(100); // MB
  // 扫描进度（Tauri 事件）
  const [bigfileProgress, setBigfileProgress] = useState(0);
  const [growthProgress, setGrowthProgress] = useState(0);

  // 订阅后端进度事件
  useEffect(() => {
    const u1 = listen<{ scannedFiles: number }>("bigfiles:progress", (e) => {
      setBigfileProgress(e.payload.scannedFiles);
    });
    const u2 = listen<{ filesScanned: number }>("disk-growth:progress", (e) => {
      setGrowthProgress(e.payload.filesScanned);
    });
    return () => {
      u1.then((f) => f());
      u2.then((f) => f());
    };
  }, []);

  const scan = useCallback(async (path: string, pushToStack: boolean) => {
    setLoading(true);
    setError(null);
    try {
      const result = await api_insight.scan_drive(path);
      setTree(result);
      if (pushToStack) {
        setStack((s) => [...s, { ...result, name: path }]);
      } else {
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

  // 首次加载：扫描 C:\ + 检查基线
  useEffect(() => {
    scan("C:\\", false);
    api_insight.has_disk_snapshot("C:\\").then(setHasBaseline).catch(() => {});
  }, [scan]);

  const runHotspot = async () => {
    setHotspotLoading(true);
    setError(null);
    try {
      setHotspot(await api_insight.scan_hotspot());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setHotspotLoading(false);
    }
  };

  const createBaseline = async () => {
    setGrowthLoading(true);
    setGrowthProgress(0);
    try {
      const resp = await api_insight.scan_disk_growth("C:\\");
      setHasBaseline(true);
      setGrowth(resp.growth);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setGrowthLoading(false);
    }
  };

  const runGrowth = async () => {
    setGrowthLoading(true);
    setGrowthProgress(0);
    setError(null);
    try {
      const resp = await api_insight.scan_disk_growth("C:\\");
      setGrowth(resp.growth);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setGrowthLoading(false);
    }
  };

  const runBigfile = async () => {
    setBigfileLoading(true);
    setBigfileProgress(0);
    setError(null);
    try {
      setBigfile(await api_insight.scan_large_files("C:\\", bigfileMin * 1024 * 1024));
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBigfileLoading(false);
    }
  };

  const cancelBigfile = () => {
    api_insight.cancel_large_file_scan();
  };

  const drillInto = (node: TreeNode) => {    if (!node.isDir) return;
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

        {/* 视图切换 */}
        <div className="flex gap-1 rounded-lg bg-[var(--color-surface-hover)] p-0.5">
          {([["tree", "目录树"], ["hotspot", "热点"], ["growth", "增长"], ["bigfile", "大文件"]] as const).map(([id, label]) => (
            <button
              key={id}
              onClick={() => {
                setTab(id);
                if (id === "hotspot" && !hotspot && !hotspotLoading) runHotspot();
                if (id === "growth" && !growth && hasBaseline && !growthLoading) runGrowth();
                if (id === "bigfile" && !bigfile && !bigfileLoading) runBigfile();
              }}
              className="rounded-md px-3 py-1 text-xs font-medium transition-all"
              style={{
                background: tab === id ? "var(--color-surface)" : "transparent",
                color: tab === id ? "var(--color-primary)" : "var(--color-text-secondary)",
              }}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* 内容区 */}
      <div className="flex-1 overflow-y-auto p-4">
        {tab === "tree" && loading && <div className="text-center text-[var(--color-text-secondary)]">扫描中…</div>}
        {error && <div className="mb-4 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">{error}</div>}

        {tab === "tree" && tree && !loading && (
          <>
            {/* d3-hierarchy 真 TreeMap（面积=大小，点击下钻） */}
            <div className="mb-6">
              <h3 className="mb-2 text-xs font-semibold text-[var(--color-text-secondary)]">
                空间分布 TreeMap（面积 = 大小，点击目录下钻）
              </h3>
              <TreeMapView tree={tree} onDrill={drillInto} />
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

        {/* ===== 热点目录 tab ===== */}
        {tab === "hotspot" && (
          <div>
            {hotspotLoading && <div className="pulse p-8 text-center text-sm text-[var(--color-text-secondary)]">正在扫描 AppData 大目录…</div>}
            {hotspot && !hotspotLoading && (
              <>
                <div className="mb-3 rounded-xl bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
                  扫描 {hotspot.scannedFolders} 个目录（{(hotspot.durationMs / 1000).toFixed(1)}s），{hotspot.entries.length} 个大目录上榜（&gt;10MB）
                </div>
                <div className="space-y-1">
                  {hotspot.entries.map((e) => {
                    const maxSize = hotspot.entries[0]?.totalSize ?? 1;
                    const ratio = e.totalSize / maxSize;
                    return (
                      <div key={e.path} className="group rounded-lg px-3 py-2 hover:bg-[var(--color-surface-hover)]">
                        <div className="flex items-center gap-3">
                          <div className="w-44 truncate text-sm font-medium">
                            {e.name}
                            {e.isCache && <span className="ml-1.5 rounded-full bg-[var(--color-safe)]/10 px-1.5 py-0.5 text-[10px] text-[var(--color-safe)]">缓存</span>}
                          </div>
                          <div className="relative h-4 flex-1 overflow-hidden rounded bg-[var(--color-surface-hover)]">
                            <div className="bar-fill h-full rounded" style={{ width: `${Math.max(ratio * 100, 2)}%`, background: e.isCache ? "var(--color-safe)" : "var(--color-primary)" }} />
                          </div>
                          <span className="font-num w-20 text-right text-xs text-[var(--color-text-secondary)]">{formatSize(e.totalSize)}</span>
                        </div>
                        <div className="mt-0.5 flex justify-between pl-0 text-[10px] text-[var(--color-text-dim)]">
                          <span className="truncate font-mono">{e.path}</span>
                          <span className="shrink-0">{e.fileCount.toLocaleString()} 个文件</span>
                        </div>
                        {e.children.length > 0 && (
                          <div className="mt-1.5 space-y-0.5 border-l-2 border-[var(--color-line)] pl-3">
                            {e.children.map((c) => (
                              <div key={c.path} className="flex items-center gap-2 text-xs">
                                <span className="w-40 truncate text-[var(--color-text-secondary)]">└ {c.name}</span>
                                <span className="font-num text-[var(--color-text-dim)]">{formatSize(c.totalSize)}</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </>
            )}
          </div>
        )}

        {/* ===== 增长对比 tab ===== */}
        {tab === "growth" && (
          <div>
            {!hasBaseline ? (
              <div className="card mx-auto mt-10 max-w-md p-6 text-center">
                <TrendingUp size={28} className="mx-auto mb-3 text-[var(--color-primary)]" />
                <div className="mb-2 text-sm font-semibold">尚未创建基线快照</div>
                <p className="mb-4 text-xs text-[var(--color-text-secondary)]">
                  先创建一次基线，之后每次对比就能看出哪些目录在增长
                </p>
                <button onClick={createBaseline} disabled={growthLoading} className="btn btn-primary">
                  {growthLoading ? "创建中…" : "创建基线快照"}
                </button>
              </div>
            ) : growthLoading ? (
              <div className="pulse p-8 text-center text-sm text-[var(--color-text-secondary)]">
                对比计算中…
                {growthProgress > 0 && (
                  <div className="mt-1 font-num text-xs">已扫描 {growthProgress.toLocaleString()} 个文件</div>
                )}
                <button
                  onClick={() => api_insight.cancel_disk_growth_scan()}
                  className="btn mt-3 flex items-center gap-1 text-xs"
                >
                  <X size={12} /> 取消扫描
                </button>
              </div>
            ) : growth ? (
              <>
                <div className="mb-3 flex items-center justify-between rounded-xl bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
                  <span>
                    基线 {new Date(growth.baselineAt * 1000).toLocaleString("zh-CN")}（{growth.timeSpanHours.toFixed(1)} 小时前）
                    · 总增长 <strong>{formatSize(Math.max(growth.totalGrowth, 0))}</strong>
                  </span>
                  <div className="flex gap-2">
                    <button onClick={runGrowth} className="btn text-xs">重新对比</button>
                    <button onClick={createBaseline} className="btn text-xs">重置基线</button>
                  </div>
                </div>
                {growth.entries.length === 0 ? (
                  <div className="p-8 text-center text-sm text-[var(--color-text-secondary)]">与基线相比无显著变化</div>
                ) : (
                  <div className="space-y-1">
                    {growth.entries.map((e) => (
                      <div key={e.path} className="flex items-center gap-3 rounded-lg px-3 py-2 hover:bg-[var(--color-surface-hover)]">
                        <span className="chip shrink-0" style={{
                          color: e.level === "significant" ? "var(--color-danger)" : e.level === "fast" ? "var(--color-caution)" : e.level === "new" ? "var(--color-primary)" : "var(--color-safe)",
                        }}>
                          {e.level === "significant" ? "激增" : e.level === "fast" ? "快增" : e.level === "new" ? "新增" : "减少"}
                        </span>
                        <div className="flex-1 min-w-0">
                          <div className="truncate text-sm">{e.name}</div>
                          <div className="truncate font-mono text-[10px] text-[var(--color-text-dim)]">{e.path}</div>
                        </div>
                        <div className="font-num shrink-0 text-right text-xs">
                          <div style={{ color: e.diff > 0 ? "var(--color-danger)" : "var(--color-safe)" }}>
                            {e.diff > 0 ? "+" : ""}{formatSize(Math.abs(e.diff))}
                            {e.oldSize > 0 && ` (${e.diffPercent > 0 ? "+" : ""}${e.diffPercent.toFixed(0)}%)`}
                          </div>
                          <div className="text-[10px] text-[var(--color-text-dim)]">
                            {formatSize(e.oldSize)} → {formatSize(e.newSize)}
                          </div>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </>
            ) : (
              <div className="p-8 text-center">
                <button onClick={runGrowth} className="btn btn-primary">开始对比</button>
              </div>
            )}
          </div>
        )}
        {/* ===== 大文件 tab ===== */}
        {tab === "bigfile" && (
          <div>
            <div className="mb-3 flex items-center justify-between rounded-xl bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
              <span className="flex items-center gap-2">
                阈值
                <select
                  value={bigfileMin}
                  onChange={(e) => setBigfileMin(Number(e.target.value))}
                  disabled={bigfileLoading}
                  className="rounded-md border border-[var(--color-line)] bg-[var(--color-surface)] px-2 py-0.5 text-xs"
                >
                  {[10, 50, 100, 500, 1024].map((mb) => (
                    <option key={mb} value={mb}>{mb >= 1024 ? "1 GB" : `${mb} MB`}</option>
                  ))}
                </select>
                {bigfile && !bigfileLoading && (
                  <span className="text-xs text-[var(--color-text-secondary)]">
                    命中 {bigfile.totalFound.toLocaleString()} 个 / 扫描 {bigfile.scannedFiles.toLocaleString()} 个文件 · {(bigfile.durationMs / 1000).toFixed(1)}s
                    {bigfile.truncated && "（仅显示 Top 200）"}
                    {bigfile.cancelled && "（已取消）"}
                  </span>
                )}
              </span>
              <div className="flex gap-2">
                {bigfileLoading ? (
                  <button onClick={cancelBigfile} className="btn flex items-center gap-1 text-xs">
                    <X size={12} /> 取消扫描
                  </button>
                ) : (
                  <button onClick={runBigfile} className="btn btn-primary flex items-center gap-1 text-xs">
                    <RefreshCw size={12} /> {bigfile ? "重新扫描" : "开始扫描"}
                  </button>
                )}
              </div>
            </div>

            {bigfileLoading && (
              <div className="pulse p-8 text-center text-sm text-[var(--color-text-secondary)]">
                正在扫描 C:\ 大文件（{bigfileMin >= 1024 ? "1 GB" : `${bigfileMin} MB`} 以上）…
                {bigfileProgress > 0 && (
                  <div className="mt-1 font-num text-xs">已扫描 {bigfileProgress.toLocaleString()} 个文件</div>
                )}
              </div>
            )}

            {bigfile && !bigfileLoading && bigfile.entries.length === 0 && (
              <div className="p-8 text-center text-sm text-[var(--color-text-secondary)]">
                未找到 {bigfileMin >= 1024 ? "1 GB" : `${bigfileMin} MB`} 以上的文件
              </div>
            )}

            {bigfile && !bigfileLoading && bigfile.entries.length > 0 && (
              <div className="space-y-1">
                {bigfile.entries.map((e) => {
                  const maxSize = bigfile.entries[0]?.size ?? 1;
                  const ratio = e.size / maxSize;
                  return (
                    <div key={e.path} className="group rounded-lg px-3 py-2 hover:bg-[var(--color-surface-hover)]">
                      <div className="flex items-center gap-3">
                        <FileWarning size={14} className="shrink-0 text-[var(--color-caution)]" />
                        <div className="w-56 truncate text-sm font-medium" title={e.path}>{e.name}</div>
                        <div className="relative h-4 flex-1 overflow-hidden rounded bg-[var(--color-surface-hover)]">
                          <div className="bar-fill h-full rounded" style={{ width: `${Math.max(ratio * 100, 2)}%`, background: "var(--color-caution)" }} />
                        </div>
                        <span className="font-num w-20 text-right text-xs text-[var(--color-text-secondary)]">{formatSize(e.size)}</span>
                        <div className="flex shrink-0 gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                          <button
                            title="打开所在目录"
                            onClick={() => api_settings.reveal_in_folder(e.path).catch(() => {})}
                            className="rounded p-1 hover:bg-[var(--color-surface)]"
                          >
                            <FolderOpen size={13} />
                          </button>
                          <button
                            title="打开文件"
                            onClick={() => api_settings.open_file(e.path).catch(() => {})}
                            className="rounded p-1 hover:bg-[var(--color-surface)]"
                          >
                            <ExternalLink size={13} />
                          </button>
                        </div>
                      </div>
                      <div className="mt-0.5 flex justify-between pl-6 text-[10px] text-[var(--color-text-dim)]">
                        <span className="truncate font-mono">{e.dir}</span>
                        <span className="shrink-0">{e.modified > 0 ? new Date(e.modified).toLocaleDateString("zh-CN") : ""}</span>
                      </div>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
