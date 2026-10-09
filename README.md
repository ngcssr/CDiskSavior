# C盘清理工具合并工程

将三个开源/源码可见的 Windows C 盘清理工具的功能合并为单一应用。

## 三方来源

| 项目 | 仓库 | 版本 | 技术栈 | 许可 |
| --- | --- | --- | --- | --- |
| C盘管家 / disk-butler | https://github.com/YGTQ3/disk-butler | v0.7.8 (`2b91e99283`) | Tauri 2 + Rust + React 19 + TS | MIT |
| LightC | https://github.com/Chunyu33/light-c | main (`2.16.15`) | Tauri 2 + Rust + React 19 + TS | LightC Source Available v1.0（非开源） |
| WindowsClear / 愚公移盘 | https://github.com/tanaer/WindowsClear | v0.1.3 | Rust + eframe/egui | MIT |

**合并底座**：disk-butler（`com.diskbutler.app`）。

## 目录结构

```
├─ 合并方案/                 # 设计文档与接口映射（主交付物）
│  ├─ 合并设计.md
│  ├─ 命令映射表.csv
│  └─ 命令映射表.md
├─ 源码/                     # 三方源码快照（vendored）
│  ├─ src078/                #   disk-butler v0.7.8
│  ├─ src_lightc/            #   LightC main
│  └─ src_wclear/            #   WindowsClear v0.1.3
├─ rev/                      # disk-butler 二进制逆向分析
│  ├─ 报告_逆向分析.md
│  ├─ frontend/              #   从 exe 还原的前端源码（Brotli 解包）
│  ├─ icons/                 #   从 exe 提取的应用图标
│  ├─ out/                   #   字符串提取结果
│  ├─ cat_*.txt              #   从 exe 提取的知识库/协议文本
│  └─ mine*.py               #   分析脚本
└─ .gitignore
```

## 当前状态

- **P0 已完成**：设计对齐（`合并方案/合并设计.md` + 119 条命令映射 → 116 个统一命令）
- **P1 未开始**：工程骨架

## 重新获取三方源码

`源码/` 下的第三方快照体积较大，且不属于本工程的原创内容。若需重新拉取：

```bash
# disk-butler v0.7.8
curl -sSL -o v078.tar.gz https://codeload.github.com/YGTQ3/disk-butler/tar.gz/refs/tags/v0.7.8
mkdir -p 源码/src078 && tar -xzf v078.tar.gz -C 源码/src078 --strip-components=1

# light-c main
curl -sSL -o lightc.tar.gz https://codeload.github.com/Chunyu33/light-c/tar.gz/refs/heads/main
mkdir -p 源码/src_lightc && tar -xzf lightc.tar.gz -C 源码/src_lightc --strip-components=1

# WindowsClear v0.1.3
curl -sSL -o wclear.tar.gz https://codeload.github.com/tanaer/WindowsClear/tar.gz/refs/tags/v0.1.3
mkdir -p 源码/src_wclear && tar -xzf wclear.tar.gz -C 源码/src_wclear --strip-components=1
```

## 许可提示

`源码/src_lightc` 采用 LightC Source Available License v1.0，其 §2(b)(c) 与 §7 禁止分发衍生二进制，
仅授权个人/教育/研究/评估/非商业开发用途。本工程当前为本地自用性质；若需对外发布，
须先剥离 LightC 代码（见 `合并方案/合并设计.md` §0）。
另外两个项目（disk-butler、WindowsClear）均为 MIT，可自由分发。
