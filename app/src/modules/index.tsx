import type { ModuleId } from "../store";
import Dashboard from "../pages/Dashboard";
import Insight from "../pages/Insight";
import Relocate from "../pages/Relocate";
import Clean from "../pages/Clean";
import Slim from "../pages/Slim";
import Tune from "../pages/Tune";
import SettingsPage from "../pages/Settings";
import Advanced from "../pages/Advanced";
import {
  LayoutDashboard, ChartPie, Trash2, HardDriveDownload,
  Wrench, Rocket, ShieldCheck, Settings,
} from "lucide-react";

/** 设计文档 §2 的 8 模块注册表 */
export const MODULES = [
  { id: "dashboard", label: "总览", icon: LayoutDashboard, page: () => <Dashboard /> },
  { id: "insight", label: "磁盘透视", icon: ChartPie, page: () => <Insight /> },
  { id: "clean", label: "一键清理", icon: Trash2, page: () => <Clean /> },
  { id: "relocate", label: "空间腾挪", icon: HardDriveDownload, page: () => <Relocate /> },
  { id: "slim", label: "系统瘦身", icon: Wrench, page: () => <Slim /> },
  { id: "tune", label: "启动与内存", icon: Rocket, page: () => <Tune /> },
  { id: "advanced", label: "高级维护", icon: ShieldCheck, page: () => <Advanced /> },
  { id: "settings", label: "设置", icon: Settings, page: () => <SettingsPage /> },
] as const satisfies ReadonlyArray<{
  id: ModuleId; label: string; icon: any; page: any;
}>;
