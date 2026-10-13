//! M4 扫描器：枚举扫描源下的大文件夹（>阈值）
//!
//! 移植自 WindowsClear 的 core/scanner.rs，改造为 Tauri 命令。
//! 设计文档 §4.3 scan_relocate_targets。
//!
//! V6 修复：reparse point（含 junction）在扫描结果中标记为 skipped，
//! 不静默跳过——UI 需展示"该目录已是链接，需先恢复原迁移"。

use crate::relocate::junction;
use serde::Serialize;
use std::collections::HashMap;
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

/// 默认扫描源（设计文档 §6 C8 + disk-butler 成品 config.json 对齐 5 组）
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
    // 固定路径三组（AppData Mover 同款默认全勾）：Program Files
    // 是空间大户（本机 22.3+9.9GB）。迁移高风险由 UI 提示承担，
    // 默认关闭会让扫描结果缺 WindowsApps 等大项（用户已反馈）。
    for (path, label, subdir) in [
        (r"C:\Program Files", "Program Files", "Program Files"),
        (r"C:\Program Files (x86)", "Program Files (x86)", "Program Files (x86)"),
        (r"C:\ProgramData", "ProgramData", "ProgramData"),
    ] {
        if std::path::Path::new(path).is_dir() {
            sources.push(ScanSource {
                enabled: true,
                label: label.into(),
                path: path.into(),
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

/// 扫描扫描源下的大文件夹（walkdir 慢路径——MFT 不可用时的兜底）
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

            // junction 不进结果（对齐 AppData Mover：已是链接的目录无迁移价值，
            // 用户反馈 0 B 链接项干扰列表）
            if is_jct {
                continue;
            }

            let size = get_dir_size(&p);

            // 阈值过滤
            let t = threshold_bytes.unwrap_or(100 * 1024 * 1024); // 默认 100MB
            if size < t {
                continue;
            }

            targets.push(RelocateTarget {
                label: src.label.clone(),
                source: p.to_string_lossy().to_string(),
                target_subdir: src.target_subdir.clone(),
                size,
                path: p.to_string_lossy().to_string(),
                is_junction: false,
            });
        }
    }

    // 按大小降序
    targets.sort_by(|a, b| b.size.cmp(&a.size));
    targets
}

/// 从 MFT 树聚合扫描结果（快路径：零文件遍历，直接读树上聚合大小）——
/// 对齐 AppData Mover 的扫描速度（MFT 直读 vs walkdir 逐文件遍历，
/// 后者 5 个源串行要数分钟，前者缓存命中秒回/首扫 11-25s）。
/// junction 项不进结果。树里下钻不到的源由调用方回退 walkdir 版。
pub fn scan_large_folders_from_tree(
    sources: &[ScanSource],
    trees: &HashMap<char, crate::scan::TreeNode>,
    threshold_bytes: Option<u64>,
) -> Vec<RelocateTarget> {
    let mut targets = Vec::new();
    let t = threshold_bytes.unwrap_or(100 * 1024 * 1024);

    for src in sources.iter().filter(|s| s.enabled) {
        let letter = match src.path.chars().next() {
            Some(c) if c.is_ascii_alphabetic() => c,
            _ => continue,
        };
        let tree = match trees.get(&letter) {
            Some(x) => x,
            None => continue, // 该盘无树 → 调用方对这类源回退 walkdir
        };
        let node = match drill_to(tree, &src.path) {
            Some(n) => n,
            None => continue,
        };
        for child in &node.children {
            if !child.is_dir {
                continue;
            }
            // MFT 树的"其他 (N 项)"溢出节点不是真实目录，跳过
            if child.name.starts_with("其他 (") {
                continue;
            }
            let child_path = if src.path.ends_with('\\') {
                format!("{}{}", src.path, child.name)
            } else {
                format!("{}\\{}", src.path, child.name)
            };
            // junction 不进结果（对齐 AppData Mover）
            if junction::is_junction(std::path::Path::new(&child_path)) {
                continue;
            }
            if child.size < t {
                continue;
            }
            targets.push(RelocateTarget {
                label: src.label.clone(),
                source: child_path.clone(),
                target_subdir: src.target_subdir.clone(),
                size: child.size,
                path: child_path,
                is_junction: false,
            });
        }
    }

    targets.sort_by(|a, b| b.size.cmp(&a.size));
    targets
}

/// 在 MFT 树中按路径逐段下钻（大小写不敏感）
fn drill_to<'a>(root: &'a crate::scan::TreeNode, path: &str) -> Option<&'a crate::scan::TreeNode> {
    let root_norm = root.path.trim_end_matches(['\\', '/']).to_lowercase();
    let path_lower = path.trim_end_matches(['\\', '/']).to_lowercase();
    let rel = path_lower.strip_prefix(&root_norm)?;
    let mut cur = root;
    for seg in rel.split('\\').filter(|s| !s.is_empty()) {
        cur = cur
            .children
            .iter()
            .find(|c| c.is_dir && c.name.to_lowercase() == seg)?;
    }
    Some(cur)
}
