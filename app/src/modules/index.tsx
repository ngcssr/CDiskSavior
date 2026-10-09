import type { ModuleId } from "../store";
import ModulePlaceholder from "./ModulePlaceholder";
import Dashboard from "../pages/Dashboard";
import Insight from "../pages/Insight";
import Relocate from "../pages/Relocate";
import Clean from "../pages/Clean";
import Slim from "../pages/Slim";
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
  { id: "tune", label: "启动与内存", icon: Rocket, page: () => <ModulePlaceholder id="tune" title="启动与内存" subtitle="启动项治理 / 内存排行" /> },
  { id: "advanced", label: "高级维护", icon: ShieldCheck, page: () => <ModulePlaceholder id="advanced" title="高级维护" subtitle="DISM 深度清理（分析→确认）/ 规则采集 / 扫描服务修复" /> },
  { id: "settings", label: "设置", icon: Settings, page: () => <ModulePlaceholder id="settings" title="设置与工具" subtitle="外观 / 数据目录 / 权限诊断 / 关于 / 日志" /> },
] as const satisfies ReadonlyArray<{
  id: ModuleId; label: string; icon: any; page: any;
}>;
