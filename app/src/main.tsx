import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";

// 启动页已移除（用户指定）：窗口出现即主题浅色背景（backgroundColor），
// React 立即挂载尽早渲染 UI，无需再给启动页让出主线程。
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
