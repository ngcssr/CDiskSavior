# CDisk Savior

磁盘与内存维护助手 —— 基于 disk-butler + light-c + WindowsClear 三方源码合并。

## 技术栈

- **前端**：React 19 + TypeScript 5.8 + Tailwind CSS 4 + Vite 7 + zustand v5
- **后端**：Tauri 2.12 + Rust 1.99 + windows-sys 0.59
- **打包**：NSIS

## 功能模块（8 个）

| 模块 | 功能 |
|---|---|
| M1 总览 | 磁盘容量卡片 / 内存概览 |
| M2 磁盘透视 | jwalk 并行目录扫描 / 面包屑下钻 |
| M3 一键清理 | R1 四层护栏删除引擎 / 8 内置清理项 / 孤儿目录扫描 |
| M4 空间腾挪 | 大文件夹迁移 + FSCTL Junction / 暂停取消回滚 / 原子历史 |
| M5 系统瘦身 | 休眠开关 / WinSxS DISM / 旧驱动 / AI 模型资产 |
| M6 启动与内存 | StartupApproved 可逆切换 + 建议引擎 |
| M7 高级维护 | 系统空间分析 / UAC 深度清理 |
| M8 设置 | 权限诊断 / 主题 / 数据目录 / 自检 |

## 安全设计

- **R1 统一删除引擎**：id 模型 → canonicalize 校验 → reparse 属性拒绝 → 唯一原语集合
- **R2 迁移安全网**：同卷持久备份（非 %TEMP%）/ 空间预检 ×1.1 / FSCTL Junction / 取消清理
- **R4 按操作提权**：UTF-16LE+Base64 内联，不落地 .ps1，UAC 取消判 1223
- **R8 状态原子写**：临时文件+rename / .bak 损坏自愈 / 单实例互斥
- **R9 路径校验**：target_root 本地固定盘 / 非 OneDrive / `\\?\` 长路径

## 开发

```bash
cd app
npm install          # 前端依赖（首次）
npm run dev          # 开发模式（Vite + Tauri）
npm run build        # 前端构建（tsc + vite）
cd src-tauri
cargo test           # 15 个单元测试
cargo build          # 后端构建
```

## 打包

```bash
cd app
npx @tauri-apps/cli build --bundles nsis
# 产出：src-tauri/target/release/bundle/nsis/CDisk Savior_0.1.0_x64-setup.exe
```

## 已知限制（v0.1.0）

- Restart Manager API 为 stub（windows-sys 签名不兼容，待换 windows crate 0.61）
- TreeMap 为条形图简化版（d3-hierarchy 待接入）
- MFT 扫描服务未实现（当前用 jwalk 慢速路径）
- 注册表冗余扫描 / 图标缓存 / 上下文菜单为占位（返回空列表）
