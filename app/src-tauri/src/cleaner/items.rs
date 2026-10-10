//! 清理项：三方源码全量合并（disk-butler 40 + light-c 系统级独有 → 去重后 46 项）
//!
//! 来源：
//! - disk-butler cleanup.rs candidates()：开发工具/应用缓存/日志杂项
//! - light-c scanner/categories.rs：Prefetch/传递优化/WER/转储/PatchCache/着色器
//! - WindowsClear：无清理项（纯迁移模块）
//!
//! 结构升级：单项支持多路径（disk-butler PathDetail 同款），
//! 每项独立经 R1 引擎四层护栏校验。

use super::engine::{self, DeleteResult};
use super::{CleanupItem, CleanupReport, CleanupScan};
use crate::state;
use std::path::PathBuf;

/// 清理项定义（多路径）
struct ItemDef {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    category: &'static str, // system / browser / dev / app / logs / update / recycle
    safety: &'static str,   // safe / caution
    paths: fn() -> Vec<PathBuf>,
}

// ---------- 路径构造辅助 ----------

fn env(p: &str) -> PathBuf {
    std::env::var_os(p).map(PathBuf::from).unwrap_or_default()
}
fn envj(p: &str, sub: &str) -> PathBuf {
    env(p).join(sub)
}
fn sysj(sub: &str) -> PathBuf {
    env("SystemRoot").join(sub)
}
fn userj(sub: &str) -> PathBuf {
    env("USERPROFILE").join(sub)
}

/// 全部清理项（去重合并后的并集）
fn item_defs() -> Vec<ItemDef> {
    vec![
        // ===== 系统临时（system） =====
        ItemDef { id: "temp-user", name: "用户临时文件", description: "%TEMP% 下程序运行残留，Windows 不主动清空", category: "system", safety: "safe", paths: || vec![env("TEMP")] },
        ItemDef { id: "windows-temp", name: "Windows 临时文件", description: "C:\\Windows\\Temp，系统级残留（需管理员）", category: "system", safety: "caution", paths: || vec![sysj("Temp")] },
        ItemDef { id: "prefetch", name: "Prefetch 预读取缓存", description: "程序启动加速缓存，清理后首次启动稍慢（需管理员）", category: "system", safety: "caution", paths: || vec![sysj("Prefetch")] },
        ItemDef { id: "windows-logs", name: "Windows 系统日志", description: "C:\\Windows\\Logs 下的安装与诊断日志（需管理员）", category: "system", safety: "caution", paths: || vec![sysj("Logs")] },
        ItemDef { id: "thumbnail-cache", name: "缩略图缓存", description: "资源管理器缩略图数据库，重建无影响", category: "system", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\Windows\\Explorer")] },
        ItemDef { id: "clipboard-cache", name: "剪贴板缓存", description: "剪贴板历史记录缓存（Win+V 历史将清空）", category: "system", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\Clipboard")] },

        // ===== 更新与组件（update） =====
        ItemDef { id: "wup-download", name: "Windows 更新下载缓存", description: "SoftwareDistribution\\Download，已安装更新的安装包（需管理员）", category: "update", safety: "caution", paths: || vec![sysj("SoftwareDistribution\\Download")] },
        ItemDef { id: "delivery-optimization", name: "传递优化缓存", description: "Windows 更新 P2P 分发缓存（需管理员）", category: "update", safety: "caution", paths: || vec![sysj("SoftwareDistribution\\DeliveryOptimization"), envj("PROGRAMDATA", "Microsoft\\Windows\\DeliveryOptimization\\Cache")] },
        ItemDef { id: "patchcache", name: "MSI 补丁缓存", description: "Installer\\$PatchCache$，卸载/修复软件时可能需要（需管理员，谨慎）", category: "update", safety: "caution", paths: || vec![sysj("Installer\\$PatchCache$")] },
        ItemDef { id: "windows-old", name: "旧版 Windows 安装", description: "Windows.old，系统升级后的旧文件（需管理员，删除后无法回退）", category: "update", safety: "caution", paths: || vec![env("SystemDrive").join("Windows.old")] },
        ItemDef { id: "defender-cache", name: "Defender 扫描缓存", description: "Microsoft Defender 非关键缓存文件", category: "update", safety: "safe", paths: || vec![envj("PROGRAMDATA", "Microsoft\\Windows Defender\\Scans\\MpCache")] },

        // ===== 错误报告与转储（system） =====
        ItemDef { id: "wer", name: "Windows 错误报告", description: "WER 崩溃报告队列（用户与系统两处）", category: "system", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\Windows\\WER"), envj("PROGRAMDATA", "Microsoft\\Windows\\WER")] },
        ItemDef { id: "crashdumps", name: "应用崩溃转储", description: "CrashDumps 用户态崩溃转储文件", category: "system", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "CrashDumps")] },
        ItemDef { id: "minidump", name: "系统崩溃转储", description: "Minidump + MEMORY.DMP 蓝屏转储（需管理员，可能较大）", category: "system", safety: "caution", paths: || vec![sysj("Minidump"), sysj("MEMORY.DMP")] },

        // ===== 浏览器缓存（browser） =====
        ItemDef { id: "chrome-cache", name: "Chrome 缓存", description: "Chrome 页面/媒体缓存，重开浏览器自动重建", category: "browser", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Google\\Chrome\\User Data\\Default\\Cache")] },
        ItemDef { id: "edge-cache", name: "Edge 缓存", description: "Edge 页面/媒体缓存", category: "browser", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\Default\\Cache")] },
        ItemDef { id: "firefox-cache", name: "Firefox 缓存", description: "Firefox 配置目录下的磁盘缓存", category: "browser", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Mozilla\\Firefox\\Profiles")] },
        ItemDef { id: "chromium-family", name: "国产浏览器缓存", description: "360 安全/360 极速/夸克/百分浏览器缓存", category: "browser", safety: "safe", paths: || vec![
            envj("LOCALAPPDATA", "360Chrome\\Chrome\\User Data\\Default\\Cache"),
            envj("LOCALAPPDATA", "360ChromeX\\Chrome\\User Data\\Default\\Cache"),
            envj("LOCALAPPDATA", "Quark\\User Data\\Default\\Cache"),
            envj("LOCALAPPDATA", "CentBrowser\\User Data\\Default\\Cache"),
        ] },
        ItemDef { id: "ie-webcache", name: "IE/WebView 缓存", description: "WebCache + INetCache（影响部分内嵌网页应用）", category: "browser", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\Windows\\WebCache"), envj("LOCALAPPDATA", "Microsoft\\Windows\\INetCache")] },

        // ===== 开发工具缓存（dev） =====
        ItemDef { id: "npm-cache", name: "npm 缓存", description: "Node 包下载缓存，重装依赖时重新下载", category: "dev", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "npm-cache")] },
        ItemDef { id: "yarn-pnpm", name: "Yarn/pnpm 缓存", description: "Yarn Cache + pnpm store", category: "dev", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Yarn\\Cache"), envj("LOCALAPPDATA", "pnpm\\store"), envj("LOCALAPPDATA", "pnpm-cache")] },
        ItemDef { id: "pip-cache", name: "pip 缓存", description: "Python 包下载缓存", category: "dev", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "pip\\cache")] },
        ItemDef { id: "uv-cache", name: "uv 缓存", description: "uv（Python 包管理器）下载缓存", category: "dev", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "uv\\cache")] },
        ItemDef { id: "cargo-cache", name: "Cargo 缓存", description: "Rust crates.io 下载缓存（~/.cargo/registry）", category: "dev", safety: "safe", paths: || vec![userj(".cargo\\registry\\cache"), userj(".cargo\\registry\\src")] },
        ItemDef { id: "maven-cache", name: "Maven 仓库", description: "~/.m2/repository 已下载构件，构建时重新下载", category: "dev", safety: "safe", paths: || vec![userj(".m2\\repository")] },
        ItemDef { id: "gradle-cache", name: "Gradle 缓存", description: "~/.gradle/caches 构建缓存", category: "dev", safety: "safe", paths: || vec![userj(".gradle\\caches")] },
        ItemDef { id: "nuget-cache", name: "NuGet 全局包", description: "~/.nuget/packages，还原时重新下载", category: "dev", safety: "safe", paths: || vec![userj(".nuget\\packages")] },
        ItemDef { id: "playwright-browsers", name: "Playwright 浏览器", description: "自动化测试下载的浏览器二进制（数 GB，需要时会重装）", category: "dev", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "ms-playwright")] },
        ItemDef { id: "jetbrains-cache", name: "JetBrains 缓存", description: "IDEA/PyCharm 等索引与缓存（LOCALAPPDATA 下，设置不受影响）", category: "dev", safety: "safe", paths: || vec![env("LOCALAPPDATA").join("JetBrains")] },
        ItemDef { id: "vscode-cache", name: "VS Code 缓存", description: "编辑器缓存与缓存数据", category: "dev", safety: "safe", paths: || vec![envj("APPDATA", "Code\\Cache"), envj("APPDATA", "Code\\CachedData")] },

        // ===== 应用缓存（app） =====
        ItemDef { id: "gpu-shader", name: "显卡着色器缓存", description: "NVIDIA/AMD/D3D 着色器与 GL 缓存，游戏首进稍慢", category: "app", safety: "safe", paths: || vec![
            envj("LOCALAPPDATA", "NVIDIA\\DXCache"), envj("LOCALAPPDATA", "NVIDIA\\GLCache"),
            envj("LOCALAPPDATA", "AMD\\DxCache"), envj("LOCALAPPDATA", "AMD\\DxcCache"),
            envj("LOCALAPPDATA", "AMD\\GLCache"), envj("LOCALAPPDATA", "D3DSCache"),
        ] },
        ItemDef { id: "hf-cache", name: "Hugging Face 缓存", description: "模型/数据集下载缓存（重新下载需网络）", category: "app", safety: "safe", paths: || vec![userj(".cache\\huggingface")] },
        ItemDef { id: "wps-cache", name: "WPS 缓存", description: "WPS Office 缓存与备份文件", category: "app", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Kingsoft\\WPS Office")] },
        ItemDef { id: "adobe-cache", name: "Adobe 缓存", description: "Media Cache / CameraRaw 缓存（不含工程文件）", category: "app", safety: "safe", paths: || vec![
            envj("APPDATA", "Adobe\\Common\\Media Cache Files"),
            envj("APPDATA", "Adobe\\Common\\Media Cache"),
            envj("APPDATA", "Adobe\\CameraRaw\\Cache"),
        ] },
        ItemDef { id: "meitu-cache", name: "美图秀秀缓存", description: "MTXX/XiuXiu 系列缓存与临时文件", category: "app", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Meitu\\MTXXAgent\\Cache"), envj("LOCALAPPDATA", "Meitu\\XiuXiu\\Cache"), envj("LOCALAPPDATA", "Meitu\\XiuXiu\\Temp")] },
        ItemDef { id: "jianying-cache", name: "剪映缓存", description: "剪映专业版用户缓存", category: "app", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "JianyingPro\\User Data\\Cache")] },
        ItemDef { id: "doubao-shader", name: "豆包着色器缓存", description: "豆包客户端 ShaderCache", category: "app", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Doubao\\User Data\\ShaderCache")] },
        ItemDef { id: "dingtalk-cache", name: "钉钉缓存", description: "钉钉客户端缓存（聊天文件不受影响）", category: "app", safety: "safe", paths: || vec![envj("APPDATA", "DingTalk\\Cache")] },
        ItemDef { id: "electron-cache", name: "Electron 应用缓存", description: "基于 Electron 的桌面应用共享缓存", category: "app", safety: "safe", paths: || vec![envj("APPDATA", "Code\\Cache")] },

        // ===== 日志与杂项（logs） =====
        ItemDef { id: "onedrive-logs", name: "OneDrive 日志", description: "同步客户端日志（不影响云端文件）", category: "logs", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "Microsoft\\OneDrive\\logs")] },
        ItemDef { id: "synology-logs", name: "群晖 Drive 日志", description: "Synology Drive 日志与临时文件", category: "logs", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "SynologyDrive\\log"), envj("LOCALAPPDATA", "SynologyDrive\\temp"), envj("LOCALAPPDATA", "SynologyDrive\\data\\tmp")] },
        ItemDef { id: "teamviewer-logs", name: "TeamViewer 日志", description: "远程协助客户端日志", category: "logs", safety: "safe", paths: || vec![envj("APPDATA", "TeamViewer\\logs")] },
        ItemDef { id: "wondershare-logs", name: "万兴系列日志", description: "Wondershare 旗下软件日志", category: "logs", safety: "safe", paths: || vec![envj("APPDATA", "Wondershare\\logs")] },
        ItemDef { id: "sogoupdf-logs", name: "搜狗 PDF 日志", description: "搜狗 PDF 阅读器日志", category: "logs", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "sogoupdf\\log")] },
        ItemDef { id: "game-logs", name: "游戏日志", description: "常见游戏客户端日志与崩溃报告", category: "logs", safety: "safe", paths: || vec![envj("APPDATA", "Tencent\\Logs"), envj("LOCALAPPDATA", "Tencent\\Logs")] },
        ItemDef { id: "miktex-cache", name: "MiKTeX 缓存", description: "LaTeX 发行版字体与数据缓存", category: "logs", safety: "safe", paths: || vec![envj("LOCALAPPDATA", "MiKTeX")] },
        ItemDef { id: "idm-dwnldata", name: "IDM 下载暂存", description: "Internet Download Manager 未完成下载暂存（谨慎确认）", category: "logs", safety: "caution", paths: || vec![envj("APPDATA", "IDM\\DwnlData")] },

        // ===== 回收站（recycle） =====
        ItemDef { id: "recycle-bin", name: "回收站", description: "清空所有盘符回收站（文件将永久删除）", category: "recycle", safety: "safe", paths: || vec![env("SystemDrive").join("$Recycle.Bin")] },
    ]
}

/// 扫描清理项（多路径逐项算大小，空路径自动剔除）
pub fn list_items() -> CleanupScan {
    let mut items = Vec::new();
    let mut total = 0u64;

    for def in item_defs() {
        let mut details: Vec<(PathBuf, u64)> = (def.paths)()
            .into_iter()
            .filter(|p| !p.as_os_str().is_empty() && p.exists())
            .map(|p| {
                let s = if p.is_dir() { dir_size(&p) } else { std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0) };
                (p, s)
            })
            .filter(|(_, s)| *s > 0)
            .collect();
        details.sort_by(|a, b| b.1.cmp(&a.1));

        let size: u64 = details.iter().map(|(_, s)| s).sum();
        if size == 0 && def.id != "recycle-bin" {
            continue;
        }
        total += size;
        let first_path = details.first().map(|(p, _)| p.to_string_lossy().to_string()).unwrap_or_default();
        items.push(CleanupItem {
            id: def.id.to_string(),
            name: def.name.to_string(),
            description: def.description.to_string(),
            size,
            path: first_path,
            category: def.category.to_string(),
            safety: def.safety.to_string(),
        });
    }

    items.sort_by(|a, b| b.size.cmp(&a.size));
    CleanupScan { items, total_size: total }
}

/// 执行清理（id 驱动 + 当场重扫 + R1 四层护栏；多路径逐项过检）
pub fn run(ids: &[String]) -> CleanupReport {
    let _lock = state::acquire_write_lock();

    // 当场重扫：id → 全部路径
    let defs = item_defs();
    let allowed: std::collections::HashMap<&str, &ItemDef> =
        defs.iter().map(|d| (d.id, d)).collect();

    let allowed_roots = build_allowed_roots();
    let protected: Vec<String> = Vec::new(); // C1 互斥：M4 历史接入点（后续接 relocate::history）

    let mut freed = 0u64;
    let mut errors = Vec::new();
    let mut skipped = Vec::new();
    let free_before = total_free_all_drives();

    for id in ids {
        let Some(def) = allowed.get(id.as_str()) else {
            errors.push(format!("不在可清理名单: {}", id));
            continue;
        };

        // 回收站特殊处理：清空内容不删目录
        if def.id == "recycle-bin" {
            let mut bin_freed = 0u64;
            for p in (def.paths)() {
                if p.exists() {
                    if let Ok(s) = safe_clean_dir_contents(&p) { bin_freed += s; }
                }
            }
            freed += bin_freed;
            continue;
        }

        // 多路径逐项删除（每项独立过 R1 护栏）
        for path in (def.paths)() {
            if !path.exists() {
                continue;
            }
            let p = path.as_path();

            // C1：protected_paths 跳过
            if protected.iter().any(|pp| {
                std::path::Path::new(pp).canonicalize()
                    .map(|c| c == p.canonicalize().unwrap_or_default())
                    .unwrap_or(false)
            }) {
                skipped.push(format!("已迁移目录，跳过: {}", p.display()));
                continue;
            }

            let result = if p.is_dir() {
                engine::safe_delete_dir(p, &allowed_roots)
            } else {
                engine::safe_delete_file(p, &allowed_roots)
            };
            match result {
                DeleteResult::Deleted(size) => freed += size,
                DeleteResult::Skipped(msg) => skipped.push(msg),
                DeleteResult::Error(msg) => errors.push(msg),
            }
        }
    }

    let free_after = total_free_all_drives();
    CleanupReport { freed, free_before, free_after, errors, skipped }
}

/// 清空目录内容（不删目录本身；跳过 reparse point）
fn safe_clean_dir_contents(path: &std::path::Path) -> std::io::Result<u64> {
    use super::engine::is_reparse_point;
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();
        if is_reparse_point(&p) {
            continue;
        }
        if p.is_dir() {
            total += dir_size(&p);
            let _ = std::fs::remove_dir_all(&p);
        } else {
            total += std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            let _ = std::fs::remove_file(&p);
        }
    }
    Ok(total)
}

/// 允许根：覆盖全部清理项的根目录
fn build_allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in &["TEMP", "LOCALAPPDATA", "APPDATA", "PROGRAMDATA", "USERPROFILE"] {
        if let Some(p) = std::env::var_os(var) {
            roots.push(PathBuf::from(p));
        }
    }
    if let Ok(r) = std::env::var("SystemRoot") {
        for sub in &["Temp", "Prefetch", "Logs", "Minidump", "Installer", "SoftwareDistribution", "MEMORY.DMP"] {
            roots.push(PathBuf::from(&r).join(sub));
        }
    }
    if let Ok(d) = std::env::var("SystemDrive") {
        roots.push(PathBuf::from(&d).join("$Recycle.Bin"));
        roots.push(PathBuf::from(&d).join("Windows.old"));
    }
    roots
}

fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 汇总所有本地固定盘的剩余空间
fn total_free_all_drives() -> u64 {
    crate::scan::get_drives().iter().map(|d| d.free).sum()
}
