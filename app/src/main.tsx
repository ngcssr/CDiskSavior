import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles/tokens.css";

// 关键：延迟 React 挂载（双 rAF = 首帧真正上屏后再启动）。
// 立即 render 会让 React concurrent 渲染切片占满主线程，
// splash 的 style/layout/paint 被饿死——合成器只能画"初始布局
// 快照"（元素停在文档流左上角 + 动画 from 帧），欢迎页布局损坏。
// 先让浏览器完成欢迎页绘制并合成（合成走 GPU 进程不受主线程影响），
// 再启动 React，欢迎页完整居中显示。
requestAnimationFrame(() =>
  requestAnimationFrame(() => {
    createRoot(document.getElementById("root")!).render(
      <StrictMode>
        <App />
      </StrictMode>
    );
  })
);
