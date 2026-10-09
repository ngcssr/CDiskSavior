//! M4 扫描器：枚举扫描源下的大文件夹（>阈值）
//!
//! 移植自 WindowsClear 的 core/scanner.rs，改造为 Tauri 命令。
//! 设计文档 §4.3 scan_relocate_targets。
//!
//! V6 修复：reparse point（含 junction）在扫描结果中标记为 skipped，
//! 不静默跳过——UI 需展示"该目录已是链接，需先恢复原迁移"。

use crate::relocate::junction;
use serde::Serialize;
use std::path::PathBuf;

/// 扫描结果：一个可迁移的大文件夹
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateTarget {
    pub label: String,       // 来源标签（如 "LocalAppData"）
    pub source: String,      // 源绝对路径
    pub target_subdir: String, // 目标子目录（如 "Local"）
    pub size: u64,           // 占用字节
    pub path: String,        // 路径（与 source 相同，前端用）
    pub is_junction: bool,   // 是否已是 junction（需提示用户先恢复）
}

/// 默认扫描源（设计文档 §6 C8）
pub fn default_scan_sources() -> Vec<ScanSource> {
    let mut sources = Vec::new();
    for (env_name, label, subdir) in [
        ("LOCALAPPDATA", "LocalAppData", "Local"),
        ("APPDATA", "RoamingAppData", "Roaming"),
    ] {
        if let Ok(p) = std::env::var(env_name) {
            sources.push(ScanSource {
                enabled: true,
                label: label.into(),
                path: p,
                target_subdir: subdir.into(),
            });
        }
    }
    sources
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSource {
    pub enabled: bool,
    pub label: String,
    pub path: String,
    pub target_subdir: String,
}

/// 计算目录大小（不跟随 junction/symlink）
pub fn get_dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path)
        .max_depth(10)
        .follow_links(false) // 不跟随符号链接/junction
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 扫描扫描源下的大文件夹
///
/// 阈值：源所在盘总大小的 1% 以上（与 WindowsClear 的 10% 不同——更灵敏，
/// 让用户有更多选择）。也可传自定义阈值（字节）。
pub fn scan_large_folders(
    sources: &[ScanSource],
    threshold_bytes: Option<u64>,
) -> Vec<RelocateTarget> {
    let mut targets = Vec::new();

    for src in sources.iter().filter(|s| s.enabled) {
        let src_path = PathBuf::from(&src.path);
        let read = match std::fs::read_dir(&src_path) {
            Ok(r) => r,
            Err(_) => continue,
        };

        for entry in read.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }

            let is_jct = junction::is_junction(&p);

            // junction 目录：标记但不跳过（UI 提示用户先恢复原迁移）
            let size = if is_jct {
                // junction 指向的数据不计入迁移目标（迁移它无意义——已经是链接了）
                // 但保留 0 大小 + is_junction=true，UI 显示"已是链接"
                0
            } else {
                get_dir_size(&p)
            };

            let dir_name = entry.file_name().to_string_lossy().to_string();

            // 阈值过滤（junction 的 size=0 会被过滤，除非用户明确要看）
            if let Some(t) = threshold_bytes {
                if size < t && !is_jct {
                    continue;
                }
            } else {
                // 默认阈值：源盘总大小的 1%
                // P2 简化：用固定 100MB（后续按盘大小动态算）
                if size < 100 * 1024 * 1024 && !is_jct {
                    continue;
                }
            }

            targets.push(RelocateTarget {
                label: src.label.clone(),
                source: p.to_string_lossy().to_string(),
                target_subdir: src.target_subdir.clone(),
                size,
                path: p.to_string_lossy().to_string(),
                is_junction: is_jct,
            });
        }
    }

    // 按大小降序
    targets.sort_by(|a, b| b.size.cmp(&a.size));
    targets
}
