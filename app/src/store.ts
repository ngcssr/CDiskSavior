import { create } from "zustand";
import { persist } from "zustand/middleware";

export type ModuleId =
  | "dashboard"
  | "insight"
  | "clean"
  | "relocate"
  | "slim"
  | "tune"
  | "advanced"
  | "settings";

export interface AppState {
  /** 当前激活模块 */
  active: ModuleId;
  /** 主题：light / dark */
  theme: "light" | "dark";
  setActive: (m: ModuleId) => void;
  toggleTheme: () => void;
}

// 一次性迁移：旧默认（深色）可能被 persist 持久化到了本机，
// 直接清掉旧键，让"默认浅色"对已有用户同样生效。
// persist 键同步换 v2，避免再次命中旧值。
if (localStorage.getItem("cdisk-savior-theme") === "dark") {
  localStorage.removeItem("cdisk-savior-theme");
}
const legacy = localStorage.getItem("cdisk-savior-app");
if (legacy && legacy.includes('"dark"')) {
  localStorage.removeItem("cdisk-savior-app");
}

const stored = localStorage.getItem("cdisk-savior-theme");
// 默认浅色（用户指定）；显式切换后持久化
const initialTheme: "light" | "dark" =
  stored === "dark" || stored === "light" ? stored : "light";

export const useAppStore = create<AppState>()(
  persist(
    (set) => ({
      active: "dashboard",
      theme: initialTheme,
      setActive: (m) => set({ active: m }),
      toggleTheme: () =>
        set((s) => {
          const theme = s.theme === "dark" ? "light" : "dark";
          localStorage.setItem("cdisk-savior-theme", theme);
          return { theme };
        }),
    }),
    {
      name: "cdisk-savior-app-v2",
      partialize: (s) => ({ theme: s.theme }),
    }
  )
);
