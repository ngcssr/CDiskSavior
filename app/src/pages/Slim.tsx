import { useEffect, useState, useCallback } from "react";
import { api_slim } from "../api/commands";
import type {
  ShellIconInfo, ShellIconTarget,
  ContextMenuScanResult, ContextMenuEntry,
  RegistryScanResult, RegistryEntry,
} from "../api/commands";
import {
  Wrench, Power, Layers, HardDrive,
  Bot, RotateCw, ExternalLink, Trash2, Monitor,
  Lock, LockOpen, Undo2, FolderOpen, FileText,
  MousePointer2, Database,
} from "lucide-react";

function fmt(bytes: number): string {
  if (!bytes) return "0 B";
  const u = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${u[i]}`;
}

export default function Slim() {
  const [status, setStatus] = useState<any>(null);
  const [drivers, setDrivers] = useState<any[]>([]);
  const [driverSelected, setDriverSelected] = useState<Set<string>>(new Set());
  const [aiModels, setAiModels] = useState<any[]>([]);
  const [shellIcons, setShellIcons] = useState<ShellIconInfo[] | null>(null);
  const [shellIconsLoading, setShellIconsLoading] = useState(false);
  // 右键菜单
  const [ctxMenu, setCtxMenu] = useState<ContextMenuScanResult | null>(null);
  const [ctxMenuLoading, setCtxMenuLoading] = useState(false);
  const [ctxSelected, setCtxSelected] = useState<Set<string>>(new Set());
  // 注册表残留
  const [regScan, setRegScan] = useState<RegistryScanResult | null>(null);
  const [regLoading, setRegLoading] = useState(false);
  const [regSelected, setRegSelected] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [st, drv, ai] = await Promise.all([
        api_slim.get_system_slim_status(),
        api_slim.scan_old_drivers(),
        api_slim.scan_ai_model_assets(),
      ]);
      setStatus(st);
      setDrivers(drv);
      setAiModels(ai);
    } catch (e: any) {
      setError(e?.message ?? String(e));
    }
  }, []);

  useEffect(() => { refresh(); }, [refresh]);

  // 旧驱动操作
  const toggleDriver = (id: string) =>
    setDriverSelected(prev => { const n = new Set(prev); n.has(id) ? n.delete(id) : n.add(id); return n; });
  const toggleAllDrivers = () =>
    setDriverSelected(prev => prev.size === drivers.length ? new Set() : new Set(drivers.map(d => d.id)));
  const deleteSelectedDrivers = async () => {
    const ids = Array.from(driverSelected);
    if (ids.length === 0) return;
    if (!confirm(`确认删除 ${ids.length} 个旧驱动备份？此操作不可恢复。`)) return;
    setBusy("删除旧驱动");
    setError(null);
    try {
      const r = (await api_slim.delete_old_drivers(ids)) as any;
      setResult(r.message || `已删除 ${ids.length} 个旧驱动`);
      setDriverSelected(new Set());
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };
  const restoreDrivers = async () => {
    setBusy("恢复驱动备份");
    setError(null);
    try {
      const r = (await api_slim.restore_all_driver_backups()) as any;
      setResult(r.message || "驱动备份恢复完成");
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };
  const openDriverBackup = () => api_slim.open_driver_backup_dir().catch(() => {});

  const scanShellIcons = useCallback(async () => {
    setShellIconsLoading(true);
    setError(null);
    try {
      setShellIcons(await api_slim.scan_shell_icons());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setShellIconsLoading(false);
    }
  }, []);

  const iconTarget = (info: ShellIconInfo): ShellIconTarget => ({
    clsid: info.clsid,
    hive: info.hive,
    registryView: info.registryView,
    entryKind: info.entryKind,
  });

  const execIcon = async (fn: () => Promise<{ message: string }>, label: string) => {
    setBusy(label);
    setError(null);
    setResult(null);
    try {
      const r = await fn();
      setResult(r.message || `${label} ✓`);
      await scanShellIcons();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };

  const scanCtxMenu = useCallback(async () => {
    setCtxMenuLoading(true);
    setError(null);
    try {
      const r = await api_slim.scan_context_menu();
      setCtxMenu(r);
      setCtxSelected(new Set());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setCtxMenuLoading(false);
    }
  }, []);

  const scanRegistry = useCallback(async () => {
    setRegLoading(true);
    setError(null);
    try {
      const r = await api_slim.scan_registry_redundancy();
      setRegScan(r);
      setRegSelected(new Set());
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setRegLoading(false);
    }
  }, []);

  const toggleCtx = (id: string, protected_: boolean) => {
    if (protected_) return;
    setCtxSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };

  const toggleReg = (path: string) => {
    setRegSelected((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  };

  const deleteCtxSelected = async () => {
    setBusy("删除右键菜单");
    setError(null);
    setResult(null);
    try {
      const r = await api_slim.delete_context_menu_entries([...ctxSelected]);
      setResult(`已删除 ${r.deletedCount} 条，失败 ${r.failedCount} 条`);
      await scanCtxMenu();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };

  const deleteRegSelected = async () => {
    setBusy("删除注册表残留");
    setError(null);
    setResult(null);
    try {
      const entries = regScan?.entries.filter((e) => regSelected.has(e.path)) ?? [];
      const r = await api_slim.delete_registry_entries(entries);
      setResult(
        `已删除 ${r.deletedCount} 条` +
        (r.backupPath ? `，备份：${r.backupPath}` : "") +
        (r.errors.length > 0 ? `，${r.errors.length} 个错误` : "")
      );
      await scanRegistry();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };

  const exec = async (fn: () => Promise<any>, label: string) => {
    setBusy(label);
    setError(null);
    setResult(null);
    try {
      await fn();
      setResult(`${label} ✓`);
      await refresh();
    } catch (e: any) {
      setError(e?.message ?? String(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <Wrench size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">系统瘦身</h1>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto p-6">
        {error && (
          <div className="mb-4 rounded-xl border border-[var(--color-danger)]/30 bg-[var(--color-danger-bg-soft)] px-4 py-3 text-sm text-[var(--color-danger-text)]">
            {error}
          </div>
        )}
        {result && (
          <div className="mb-4 rounded-xl border border-[var(--color-safe)]/30 bg-[var(--color-safe)]/5 px-4 py-3 text-sm">
            {result}
          </div>
        )}

        {/* 休眠 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <Power size={14} /> 休眠
          </h2>
          <div className="flex items-center justify-between rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
            <div>
              <span className="text-sm font-medium">
                休眠状态：{status?.hibernationEnabled ? "已开启" : "已关闭"}
              </span>
              <div className="text-xs text-[var(--color-text-secondary)]">
                {status?.hibernationEnabled ? "hiberfil.sys 占用磁盘空间（约等于物理内存大小）" : "已释放 hiberfil.sys"}
              </div>
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => exec(() => api_slim.enable_hibernation(), "启用休眠")}
                disabled={busy !== null || status?.hibernationEnabled}
                className="rounded-lg bg-[var(--color-surface-hover)] px-3 py-1.5 text-sm disabled:opacity-50"
              >启用</button>
              <button
                onClick={() => exec(() => api_slim.disable_hibernation(), "禁用休眠")}
                disabled={busy !== null || !status?.hibernationEnabled}
                className="rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)] disabled:opacity-50"
              >禁用（释放空间）</button>
            </div>
          </div>
        </section>

        {/* WinSxS */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <Layers size={14} /> WinSxS 组件存储
          </h2>
          <div className="flex items-center justify-between rounded-xl border border-[var(--color-line)] bg-[var(--color-surface)] px-4 py-3">
            <div>
              <span className="text-sm font-medium">
                WinSxS 大小：{status?.winsxsSize ? fmt(status.winsxsSize) : "未知"}
              </span>
              <div className="text-xs text-[var(--color-text-secondary)]">DISM 组件清理需管理员授权</div>
            </div>
            <div className="flex gap-2">
              <button
                onClick={() => exec(() => api_slim.cleanup_winsxs(), "WinSxS 清理")}
                disabled={busy !== null}
                className="flex items-center gap-1 rounded-lg bg-[var(--color-primary)] px-3 py-1.5 text-sm text-white disabled:opacity-50"
              >
                {busy === "WinSxS 清理" ? <RotateCw size={14} className="animate-spin" /> : <Trash2 size={14} />}
                组件清理
              </button>
              <button
                onClick={() => exec(() => api_slim.cleanup_winsxs_resetbase(), "WinSxS 清理+重置基线")}
                disabled={busy !== null}
                className="rounded-lg bg-[var(--color-danger-bg-soft)] px-3 py-1.5 text-sm text-[var(--color-danger-text)] disabled:opacity-50"
              >清理+重置基线</button>
            </div>
          </div>
        </section>

        {/* 快捷入口 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <ExternalLink size={14} /> 系统设置
          </h2>
          <div className="flex gap-2">
            {[
              { fn: () => api_slim.open_virtual_memory_settings(), label: "虚拟内存" },
              { fn: () => api_slim.open_storage_settings(), label: "存储感知" },
              { fn: () => api_slim.open_disk_cleanup(), label: "磁盘清理" },
              { fn: () => api_slim.rebuild_search_index(), label: "重建搜索索引" },
              { fn: () => api_slim.restart_explorer(), label: "重启资源管理器" },
            ].map((b) => (
              <button
                key={b.label}
                onClick={() => exec(b.fn, b.label)}
                disabled={busy !== null}
                className="rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-1.5 text-sm disabled:opacity-50"
              >
                {b.label}
              </button>
            ))}
          </div>
        </section>

        {/* 旧驱动 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <HardDrive size={14} /> 旧驱动（{drivers.length}）
          </h2>
          {drivers.length > 0 ? (
            <>
              <div className="mb-2 flex items-center gap-2">
                <button
                  onClick={toggleAllDrivers}
                  className="text-xs text-[var(--color-text-secondary)] hover:text-[var(--color-text)]"
                >
                  {driverSelected.size === drivers.length ? "取消全选" : "全选"}
                </button>
                <button
                  onClick={deleteSelectedDrivers}
                  disabled={driverSelected.size === 0 || busy !== null}
                  className="flex items-center gap-1 rounded-lg bg-[var(--color-danger)] px-2.5 py-1 text-xs text-white disabled:opacity-40"
                >
                  <Trash2 size={12} />
                  删除所选（{driverSelected.size}）
                </button>
                <button
                  onClick={restoreDrivers}
                  disabled={busy !== null}
                  className="flex items-center gap-1 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2.5 py-1 text-xs disabled:opacity-40"
                >
                  <Undo2 size={12} />
                  恢复全部备份
                </button>
                <button
                  onClick={openDriverBackup}
                  className="flex items-center gap-1 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-2.5 py-1 text-xs text-[var(--color-text-secondary)]"
                >
                  <FolderOpen size={12} />
                  备份目录
                </button>
              </div>
              <div className="space-y-1">
                {drivers.slice(0, 30).map((d) => (
                  <div key={d.id} className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
                    <input
                      type="checkbox"
                      checked={driverSelected.has(d.id)}
                      onChange={() => toggleDriver(d.id)}
                      className="accent-[var(--color-primary)]"
                    />
                    <div className="flex-1">
                      <div className="text-sm font-medium">{d.name}</div>
                      <div className="text-xs text-[var(--color-text-secondary)]">日期：{d.date}</div>
                    </div>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">无旧驱动</div>
          )}
        </section>

        {/* AI 模型 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <Bot size={14} /> AI 模型资产（{aiModels.length}，共 {fmt(aiModels.reduce((s, m) => s + m.size, 0))}）
          </h2>
          {aiModels.length > 0 ? (
            <div className="space-y-1">
              {aiModels.map((m) => (
                <div key={m.id} className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
                  <HardDrive size={16} className="text-[var(--color-primary)]" />
                  <div className="flex-1">
                    <div className="text-sm font-medium">{m.name}</div>
                    <div className="truncate font-mono text-xs text-[var(--color-text-secondary)]">{m.path}</div>
                  </div>
                  <span className="text-sm tabular-nums text-[var(--color-text-secondary)]">{fmt(m.size)}</span>
                  <button
                    onClick={() => exec(() => api_slim.delete_ai_model(m.id), `删除 ${m.name}`)}
                    disabled={busy !== null}
                    className="rounded-lg p-1 text-[var(--color-danger)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              ))}
            </div>
          ) : (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">未发现 AI 模型</div>
          )}
        </section>
        {/* 右键菜单 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <MousePointer2 size={14} /> 右键菜单
            {ctxMenu !== null && (
              <span className="text-xs font-normal">
                （{ctxMenu.entries.length} 项，{ctxMenu.invalidCount} 个无效）
              </span>
            )}
            <span className="flex-1" />
            {ctxSelected.size > 0 && (
              <button
                onClick={deleteCtxSelected}
                disabled={busy !== null}
                className="flex items-center gap-1 rounded-lg bg-[var(--color-danger-bg-soft)] px-2 py-0.5 text-xs text-[var(--color-danger-text)] disabled:opacity-50"
              >
                <Trash2 size={12} /> 删除选中（{ctxSelected.size}）
              </button>
            )}
            <button
              onClick={scanCtxMenu}
              disabled={ctxMenuLoading || busy !== null}
              className="rounded-lg bg-[var(--color-primary)] px-2 py-0.5 text-xs text-white disabled:opacity-50"
            >
              {ctxMenuLoading ? "扫描中…" : ctxMenu ? "重新扫描" : "扫描"}
            </button>
          </h2>

          {ctxMenu === null ? (
            <div className="py-2 text-xs text-[var(--color-text-secondary)]">
              扫描注册表右键菜单项，识别指向不存在程序的无效条目（删除前自动备份 .reg）
            </div>
          ) : ctxMenu.entries.length === 0 ? (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">未发现第三方右键菜单项</div>
          ) : (
            <div className="max-h-72 space-y-1 overflow-y-auto">
              {ctxMenu.entries.map((e: ContextMenuEntry) => (
                <div key={e.id}
                  className={`flex items-center gap-3 rounded-lg border px-3 py-2 ${
                    e.isSystemProtected
                      ? "border-[var(--color-line)] bg-[var(--color-surface-hover)] opacity-60"
                      : "border-[var(--color-line)] bg-[var(--color-surface)]"
                  }`}>
                  <input
                    type="checkbox"
                    checked={ctxSelected.has(e.id)}
                    disabled={e.isSystemProtected}
                    onChange={() => toggleCtx(e.id, e.isSystemProtected)}
                    className="shrink-0"
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 text-sm">
                      <span className="truncate font-medium">{e.displayName}</span>
                      <span className="shrink-0 rounded-full bg-[var(--color-surface-hover)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                        {e.scope}
                      </span>
                      {!e.exeExists && (
                        <span className="shrink-0 rounded-full bg-[var(--color-danger-bg-soft)] px-1.5 py-0.5 text-[10px] text-[var(--color-danger-text)]">
                          无效
                        </span>
                      )}
                      {e.needsAdmin && (
                        <span className="shrink-0 rounded-full bg-[var(--color-caution)]/10 px-1.5 py-0.5 text-[10px] text-[var(--color-caution)]">
                          需管理员
                        </span>
                      )}
                      {e.isSystemProtected && (
                        <span className="shrink-0 rounded-full bg-[var(--color-surface-hover)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-dim)]">
                          系统保护
                        </span>
                      )}
                    </div>
                    <div className="mt-0.5 truncate font-mono text-[10px] text-[var(--color-text-dim)]" title={e.exePath ?? ""}>
                      {e.exePath ?? e.registryPath}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* 注册表残留 */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <Database size={14} /> 注册表残留（文件关联）
            {regScan !== null && (
              <span className="text-xs font-normal">（{regScan.entries.length} 条）</span>
            )}
            <span className="flex-1" />
            {regSelected.size > 0 && (
              <button
                onClick={deleteRegSelected}
                disabled={busy !== null}
                className="flex items-center gap-1 rounded-lg bg-[var(--color-danger-bg-soft)] px-2 py-0.5 text-xs text-[var(--color-danger-text)] disabled:opacity-50"
              >
                <Trash2 size={12} /> 删除选中（{regSelected.size}）
              </button>
            )}
            <button
              onClick={() => api_slim.open_registry_backup_dir().catch((e: any) => setError(e?.message ?? String(e)))}
              title="打开备份目录"
              className="rounded p-1 hover:bg-[var(--color-surface-hover)]"
            >
              <FolderOpen size={13} />
            </button>
            <button
              onClick={scanRegistry}
              disabled={regLoading || busy !== null}
              className="rounded-lg bg-[var(--color-primary)] px-2 py-0.5 text-xs text-white disabled:opacity-50"
            >
              {regLoading ? "扫描中…" : regScan ? "重新扫描" : "扫描"}
            </button>
          </h2>

          {regScan === null ? (
            <div className="py-2 text-xs text-[var(--color-text-secondary)]">
              扫描 HKCR\Applications 文件关联残留——仅列出关联程序已不存在、且非系统路径的条目（铁证条件）
            </div>
          ) : regScan.entries.length === 0 ? (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">未发现注册表残留</div>
          ) : (
            <div className="max-h-72 space-y-1 overflow-y-auto">
              {regScan.entries.map((e: RegistryEntry) => (
                <div key={e.path} className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
                  <input
                    type="checkbox"
                    checked={regSelected.has(e.path)}
                    onChange={() => toggleReg(e.path)}
                    className="shrink-0"
                  />
                  <div className="flex-1 min-w-0">
                    <div className="truncate text-sm font-medium">{e.name}</div>
                    <div className="mt-0.5 truncate font-mono text-[10px] text-[var(--color-text-dim)]" title={e.associatedPath}>
                      {e.issue}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* 外壳入口（此电脑/导航窗格第三方图标） */}
        <section className="mb-6">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]">
            <Monitor size={14} /> 外壳入口（此电脑 / 导航窗格）
            {shellIcons !== null && <span className="text-xs font-normal">（{shellIcons.length}）</span>}
            <span className="flex-1" />
            <button
              onClick={() => api_slim.open_shell_icon_backup_dir().catch((e: any) => setError(e?.message ?? String(e)))}
              title="打开备份目录"
              className="rounded p-1 hover:bg-[var(--color-surface-hover)]"
            >
              <FolderOpen size={13} />
            </button>
            <button
              onClick={() => api_slim.open_shell_icon_log().catch((e: any) => setError(e?.message ?? String(e)))}
              title="打开操作记录"
              className="rounded p-1 hover:bg-[var(--color-surface-hover)]"
            >
              <FileText size={13} />
            </button>
          </h2>

          {shellIcons === null ? (
            <div className="py-4 text-center">
              <button
                onClick={scanShellIcons}
                disabled={shellIconsLoading || busy !== null}
                className="rounded-lg bg-[var(--color-primary)] px-4 py-1.5 text-sm text-white disabled:opacity-50"
              >
                {shellIconsLoading ? "扫描中…" : "扫描第三方入口"}
              </button>
              <div className="mt-2 text-xs text-[var(--color-text-secondary)]">
                识别此电脑与导航窗格中的第三方软件图标（百度网盘、WPS 等）
              </div>
            </div>
          ) : shellIcons.length === 0 ? (
            <div className="py-4 text-center text-sm text-[var(--color-text-secondary)]">未发现第三方外壳入口</div>
          ) : (
            <div className="space-y-1">
              {shellIcons.map((info) => (
                <div key={`${info.hive}:${info.registryView}:${info.entryKind}:${info.clsid}`}
                  className="flex items-center gap-3 rounded-lg border border-[var(--color-line)] bg-[var(--color-surface)] px-3 py-2">
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 text-sm font-medium">
                      {info.isLocked && <Lock size={12} className="text-[var(--color-caution)]" />}
                      <span className="truncate">{info.name}</span>
                      {info.applicationName && info.applicationName !== info.name && (
                        <span className="rounded-full bg-[var(--color-primary)]/10 px-1.5 py-0.5 text-[10px] text-[var(--color-primary)]">
                          {info.applicationName}
                        </span>
                      )}
                      <span className="rounded-full bg-[var(--color-surface-hover)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                        {info.entryKind === "myComputer" ? "此电脑" : "导航窗格"} · {info.hive}{info.registryView !== "default" && ` ${info.registryView}位`}
                      </span>
                    </div>
                    <div className="mt-0.5 truncate font-mono text-[10px] text-[var(--color-text-dim)]" title={info.riskReason}>
                      {info.regPath}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      title="打开注册表编辑器定位"
                      onClick={() => api_slim.open_shell_icon_registry(iconTarget(info)).catch((e: any) => setError(e?.message ?? String(e)))}
                      className="rounded p-1 hover:bg-[var(--color-surface-hover)]"
                    >
                      <ExternalLink size={13} />
                    </button>
                    {info.isLocked ? (
                      <>
                        <button
                          title="恢复节点（从备份导入）"
                          onClick={() => execIcon(() => api_slim.restore_shell_icon(iconTarget(info)), `恢复 ${info.name}`)}
                          disabled={busy !== null}
                          className="rounded p-1 text-[var(--color-primary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                        >
                          <Undo2 size={13} />
                        </button>
                        <button
                          title="解除防复活锁定（不恢复内容）"
                          onClick={() => execIcon(() => api_slim.unlock_shell_icon(iconTarget(info)), `解锁 ${info.name}`)}
                          disabled={busy !== null}
                          className="rounded p-1 text-[var(--color-caution)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                        >
                          <LockOpen size={13} />
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          title="删除（自动备份，可恢复）"
                          onClick={() => execIcon(() => api_slim.remove_shell_icon(iconTarget(info), 1), `删除 ${info.name}`)}
                          disabled={busy !== null}
                          className="rounded p-1 text-[var(--color-danger)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                        >
                          <Trash2 size={13} />
                        </button>
                        {info.entryKind === "myComputer" && (
                          <button
                            title="彻底删除+防复活（锁定父级，需管理员）"
                            onClick={() => execIcon(() => api_slim.remove_shell_icon(iconTarget(info), 2), `彻底删除 ${info.name}`)}
                            disabled={busy !== null}
                            className="rounded p-1 text-[var(--color-danger)] hover:bg-[var(--color-danger-bg-soft)] disabled:opacity-50"
                          >
                            <Lock size={13} />
                          </button>
                        )}
                      </>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
