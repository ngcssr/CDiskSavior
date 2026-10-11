import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import "./styles/tokens.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);

// 启动诊断：React 渲染完成标记
(window as any).__perf?.("react rendered");

// 窗口延迟显示：WebView 渲染完成后才 show，消除启动白屏（出现即内容）
// 用双 rAF 确保首帧已绘制，避免 show 瞬间仍有一帧空白
requestAnimationFrame(() => {
  requestAnimationFrame(() => {
    getCurrentWindow().show().catch(() => {});
  });
});
