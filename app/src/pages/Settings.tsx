import { useEffect, useState } from "react";
import { api_settings, type UpdateInfo } from "../api/commands";
import { useAppStore } from "../store";
import { Settings as SettingsIcon, ShieldCheck, Database, Sun, Moon, RefreshCw } from "lucide-react";

export default function Settings() {
  const { theme, toggleTheme } = useAppStore();
  const [isAdmin, setIsAdmin] = useState(false);
  const [dataDir, setDataDir] = useState("");
  const [serviceAvail, setServiceAvail] = useState(false);
  const [currentVersion, setCurrentVersion] = useState(0);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updating, setUpdating] = useState(false);

  const checkUpdate = async () => {
    setUpdating(true);
    try {
      setUpdateInfo(await api_settings.check_update());
    } catch (e: any) {
      setUpdateInfo(null);
    } finally {
      setUpdating(false);
    }
  };

  useEffect(() => {
    (async () => {
      try {
        const [admin, dir, svc, ver] = await Promise.all([
          api_settings.check_admin_privilege(),
          api_settings.get_data_directory(),
          api_settings.scan_service_available(),
          api_settings.get_current_version(),
        ]);
        setIsAdmin(admin);
        setDataDir(dir);
        setServiceAvail(svc);
        setCurrentVersion(ver);
      } catch {}
    })();
  }, []);

  return (
    <div className="flex h-full flex-col">
      <div className="border-b border-[var(--color-line)] px-6 py-4">
        <div className="flex items-center gap-2">
          <SettingsIcon size={20} className="text-[var(--color-primary)]" />
          <h1 className="text-xl font-bold">设置与工具</h1>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        {/* 权限状态 */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]"><ShieldCheck size={14} /> 权限状态</h2>
          <div className="flex items-center gap-2">
            <span className={`rounded-full px-3 py-1 text-sm ${isAdmin ? "bg-[var(--color-safe)]/10 text-[var(--color-safe)]" : "bg-[var(--color-caution)]/10 text-[var(--color-caution)]"}`}>
              {isAdmin ? "管理员模式" : "普通用户模式"}
            </span>
            <span className="text-xs text-[var(--color-text-secondary)]">
              {isAdmin ? "所有功能可用" : "部分操作（DISM/注册表 HKLM）需管理员"}
            </span>
          </div>
        </section>

        {/* 主题 */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">外观</h2>
          <button onClick={toggleTheme} className="flex items-center gap-2 rounded-lg bg-[var(--color-surface-hover)] px-4 py-2 text-sm">
            {theme === "dark" ? <Moon size={16} /> : <Sun size={16} />}
            切换主题（当前：{theme === "dark" ? "深色" : "浅色"}）
          </button>
        </section>

        {/* 数据目录 */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 flex items-center gap-1.5 text-sm font-semibold text-[var(--color-text-secondary)]"><Database size={14} /> 数据目录</h2>
          <div className="font-mono text-sm text-[var(--color-primary)]">{dataDir || "未确定"}</div>
          <div className="mt-1 text-xs text-[var(--color-text-secondary)]">迁移历史、清理统计、配置文件均存放于此</div>
        </section>

        {/* 检查更新（GitHub Releases 更新源） */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">检查更新</h2>
          <div className="flex items-center gap-3">
            <button
              onClick={checkUpdate}
              disabled={updating}
              className="flex items-center gap-2 rounded-lg bg-[var(--color-primary)] px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
            >
              {updating ? <RefreshCw size={14} className="animate-spin" /> : <RefreshCw size={14} />}
              检查更新
            </button>
            <span className="text-xs text-[var(--color-text-secondary)]">
              当前版本 v{currentVersion}
              {updateInfo && updateInfo.has_update && (
                <span className="ml-1 text-[var(--color-primary)]">
                  有新版 v{updateInfo.latest_version}{updateInfo.force ? "（强制更新）" : ""}
                </span>
              )}
            </span>
          </div>
          {updateInfo && updateInfo.has_update && (
            <div className="mt-3 rounded-xl bg-[var(--color-primary-soft)] px-4 py-3 text-sm">
              {updateInfo.notes && <div className="mb-2 text-[var(--color-text)]">{updateInfo.notes}</div>}
              <button
                onClick={() => api_settings.open_file(updateInfo.url).catch(() => {})}
                className="rounded-lg bg-[var(--color-primary)] px-3 py-1.5 text-xs font-medium text-white"
              >
                前往下载更新
              </button>
            </div>
          )}
        </section>

        {/* 扫描服务 */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">MFT 扫描服务</h2>
          <div className="flex items-center gap-2 text-sm">
            <span className={`rounded-full px-3 py-1 ${serviceAvail ? "bg-[var(--color-safe)]/10 text-[var(--color-safe)]" : "bg-[var(--color-text-secondary)]/10 text-[var(--color-text-secondary)]"}`}>
              {serviceAvail ? "已注册" : "未注册"}
            </span>
            {!serviceAvail && (
              <>
                <span className="text-xs">秒级扫描未启用，回退到 jwalk 慢速路径</span>
                <button
                  onClick={async () => {
                    try {
                      await api_settings.repair_scan_service();
                      setServiceAvail(await api_settings.scan_service_available());
                    } catch (e: any) {
                      alert(e?.message ?? String(e));
                    }
                  }}
                  className="rounded-lg bg-[var(--color-primary)] px-3 py-1 text-xs text-white"
                >
                  一键修复（需 UAC）
                </button>
              </>
            )}
          </div>
        </section>

      </div>
    </div>
  );
}
