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

const stored = localStorage.getItem("cdisk-savior-theme");
// 默认深色（Refined Industrial 方向以深底为设计基准）；
// 用户显式切换后持久化，不再跟随系统
const initialTheme: "light" | "dark" =
  stored === "dark" || stored === "light" ? stored : "dark";

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
      name: "cdisk-savior-app",
      partialize: (s) => ({ theme: s.theme }),
    }
  )
);
