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
const prefersDark = window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;
const initialTheme: "light" | "dark" =
  stored === "dark" || stored === "light" ? stored : prefersDark ? "dark" : "light";

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
