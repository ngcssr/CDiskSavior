import { useEffect, useState } from "react";
import { api_settings } from "../api/commands";
import { useAppStore } from "../store";
import { Settings as SettingsIcon, ShieldCheck, Database, Sun, Moon } from "lucide-react";

export default function Settings() {
  const { theme, toggleTheme } = useAppStore();
  const [isAdmin, setIsAdmin] = useState(false);
  const [dataDir, setDataDir] = useState("");
  const [serviceAvail, setServiceAvail] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const [admin, dir, svc] = await Promise.all([
          api_settings.check_admin_privilege(),
          api_settings.get_data_directory(),
          api_settings.scan_service_available(),
        ]);
        setIsAdmin(admin);
        setDataDir(dir);
        setServiceAvail(svc);
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

        {/* 检查更新（Q1：updater 已移除，保留灰置入口，接入自有发布仓库后恢复） */}
        <section className="rounded-2xl border border-[var(--color-line)] bg-[var(--color-surface)] p-4">
          <h2 className="mb-3 text-sm font-semibold text-[var(--color-text-secondary)]">检查更新</h2>
          <div className="flex items-center gap-3">
            <button
              disabled
              title="预留接口：接入自有发布仓库后恢复（当前版本零网络设计）"
              className="cursor-not-allowed rounded-lg bg-[var(--color-surface-hover)] px-4 py-2 text-sm opacity-40"
            >
              检查更新（暂未开放）
            </button>
            <span className="text-xs text-[var(--color-text-secondary)]">
              当前版本零网络设计，更新能力预留接口
            </span>
          </div>
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
