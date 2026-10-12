import { lazy, Suspense } from "react";
import type { ModuleId } from "../store";
import {
  LayoutDashboard, ChartPie, Trash2, HardDriveDownload,
  Wrench, Rocket, ShieldCheck, Settings,
} from "lucide-react";

// 按需加载：启动只解析执行总览页 chunk（其余 7 个模块点开时才从
// 本地嵌入资源加载，毫秒级）。启动 JS 工作量降 60%+，显著降低
// "React 渲染完成时刻"对系统负载的敏感度（磁盘/索引忙时的抗性）。
const Dashboard = lazy(() => import("../pages/Dashboard"));
const Insight = lazy(() => import("../pages/Insight"));
const Relocate = lazy(() => import("../pages/Relocate"));
const Clean = lazy(() => import("../pages/Clean"));
const Slim = lazy(() => import("../pages/Slim"));
const Tune = lazy(() => import("../pages/Tune"));
const SettingsPage = lazy(() => import("../pages/Settings"));
const Advanced = lazy(() => import("../pages/Advanced"));

/** 模块 chunk 加载中的极简占位（非启动页——路由级 loading，毫秒级） */
function ChunkLoading() {
  return (
    <div style={{
      display: "flex", alignItems: "center", justifyContent: "center",
      height: "100%", width: "100%",
      background: "var(--color-bg)", color: "var(--color-text-dim)",
      fontSize: 13,
    }}>
      加载中…
    </div>
  );
}

const withSuspense = (Page: React.LazyExoticComponent<React.ComponentType>) => (
  <Suspense fallback={<ChunkLoading />}>
    <Page />
  </Suspense>
);

/** 设计文档 §2 的 8 模块注册表 */
export const MODULES = [
  { id: "dashboard", label: "总览", icon: LayoutDashboard, page: () => withSuspense(Dashboard) },
  { id: "insight", label: "磁盘透视", icon: ChartPie, page: () => withSuspense(Insight) },
  { id: "clean", label: "一键清理", icon: Trash2, page: () => withSuspense(Clean) },
  { id: "relocate", label: "空间腾挪", icon: HardDriveDownload, page: () => withSuspense(Relocate) },
  { id: "slim", label: "系统瘦身", icon: Wrench, page: () => withSuspense(Slim) },
  { id: "tune", label: "启动与内存", icon: Rocket, page: () => withSuspense(Tune) },
  { id: "advanced", label: "高级维护", icon: ShieldCheck, page: () => withSuspense(Advanced) },
  { id: "settings", label: "设置", icon: Settings, page: () => withSuspense(SettingsPage) },
] as const satisfies ReadonlyArray<{
  id: ModuleId; label: string; icon: any; page: any;
}>;
