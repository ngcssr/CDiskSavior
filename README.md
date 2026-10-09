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
│  ├─ 合并设计.md            #   架构 / 依赖收敛 / 冲突消解 / 路线（v1.1）
│  ├─ 漏洞评审与方案修订.md    #   17 项缺陷 + 8 项实证测试 + 修订项 R1~R7
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
│  ├─ junction_test*.py      #   NTFS Junction 删除行为实证脚本
│  └─ mine*.py               #   二进制分析脚本
└─ .gitignore
```

## 当前状态

- **P0 已完成**：
  - 设计对齐（`合并方案/合并设计.md` v1.1）
  - 命令映射（119 条 → 116 个统一命令）
  - 漏洞评审（`合并方案/漏洞评审与方案修订.md`）：17 项缺陷、8 项删除原语实证测试
  - disk-butler 源码 ↔ 二进制交叉核验（管道协议逐字段吻合、feature 门控预测 10/10 命中）
- **P1 未开始**：工程骨架（需先确定待确认项 Q1/Q2）

## 关键风险提示

合并后**必须最高优先级处理**的两条（详见漏洞评审文档）：

1. **V1 —— 迁移备份落在 `%TEMP%`**：一旦 junction 创建与回滚双双失败，用户数据会滞留在临时目录，
   而本工具自身的清理功能会把 `%TEMP%` 当垃圾清掉，造成永久数据丢失。
   **该缺陷由"合并"这一行为放大**（单独使用 WindowsClear 时清理能力有限）。
2. **V2 —— 孤儿目录清理会删掉迁移留下的 Junction**：正确的顺序是先做迁移（P2）、再做清理（P4），
   让保护逻辑在最开始就被真实数据验证。

## 安全规范

本项目采用 `源码/src078/docs/RULES-SECURITY.md`（disk-butler 自带的安全排查 SOP）作为治理规范：
七维度审计、四级分级+触发前提、5 条信任边界红线、四层留痕。

**全局禁用原语**（实测会穿透 NTFS Junction 删除目标盘数据，链接还残留）：

| 禁用 | 实测结果 |
| --- | --- |
| `del /s` | ✘ 目标盘数据被删除 |
| `robocopy /MIR`、`xcopy /E` 等镜像同步 | ✘ 目标盘数据被删除 |

允许：`RemoveDirectoryW` / `std::fs::remove_dir*` / shell 的 `Remove-Item -Recurse`（实测均安全）。

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
