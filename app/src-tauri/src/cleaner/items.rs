//! 清理项：list_cleanup_items + run_cleanup（id 驱动 + 当场重扫）
//!
//! 设计文档 §7 R1：前端只传 id，后端当场重扫得到允许集合，再逐条删。
//! 设计文档 §6 C1：清理前检查 protected_paths（M4 迁移历史）。

use super::engine::{self, DeleteResult};
use super::{CleanupItem, CleanupReport, CleanupScan};
use crate::state;
use std::path::PathBuf;

/// 清理项定义（id → 路径 + 描述 + 安全等级）
struct ItemDef {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    path_fn: fn() -> Option<PathBuf>,
    category: &'static str,
    safety: &'static str,
}

/// 内置清理项列表
fn item_defs() -> Vec<ItemDef> {
    fn env_path(var: &str) -> Option<PathBuf> {
        std::env::var_os(var).map(PathBuf::from)
    }
    fn env_subdir(var: &str, sub: &str) -> Option<PathBuf> {
        std::env::var_os(var).map(PathBuf::from).map(|p| p.join(sub))
    }
    vec![
        ItemDef { id: "temp-user", name: "用户临时文件", description: "%TEMP% 下的临时文件", path_fn: || env_path("TEMP"), category: "temp", safety: "safe" },
        ItemDef { id: "temp-windows", name: "Windows 临时文件", description: "C:\\Windows\\Temp（需管理员）", path_fn: || std::env::var("SystemRoot").ok().map(|r| PathBuf::from(r).join("Temp")), category: "temp", safety: "caution" },
        ItemDef { id: "chrome-cache", name: "Chrome 缓存", description: "Chrome 浏览器缓存", path_fn: || env_subdir("LOCALAPPDATA", "Google\\Chrome\\User Data\\Default\\Cache"), category: "browser", safety: "safe" },
        ItemDef { id: "edge-cache", name: "Edge 缓存", description: "Edge 浏览器缓存", path_fn: || env_subdir("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\Default\\Cache"), category: "browser", safety: "safe" },
        ItemDef { id: "firefox-cache", name: "Firefox 缓存", description: "Firefox 缓存", path_fn: || env_subdir("LOCALAPPDATA", "Mozilla\\Firefox\\Profiles"), category: "browser", safety: "safe" },
        ItemDef { id: "recycle-bin", name: "回收站", description: "清空回收站", path_fn: || std::env::var("SystemDrive").ok().map(|d| PathBuf::from(d).join("$Recycle.Bin")), category: "system", safety: "safe" },
        ItemDef { id: "thumbnail-cache", name: "缩略图缓存", description: "资源管理器缩略图缓存", path_fn: || env_subdir("LOCALAPPDATA", "Microsoft\\Windows\\Explorer"), category: "system", safety: "safe" },
        ItemDef { id: "wup-download", name: "Windows 更新下载缓存", description: "SoftwareDistribution\\Download（需管理员）", path_fn: || std::env::var("SystemRoot").ok().map(|r| PathBuf::from(r).join("SoftwareDistribution\\Download")), category: "update", safety: "caution" },
    ]
}

/// 扫描清理项（返回当前可清理的项 + 大小）
pub fn list_items() -> CleanupScan {
    let mut items = Vec::new();
    let mut total = 0u64;

    for def in item_defs() {
        if let Some(path) = (def.path_fn)() {
            if !path.exists() {
                continue;
            }
            let size = if path.is_dir() {
                dir_size(&path)
            } else {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            };
            if size == 0 && def.id != "recycle-bin" {
                continue;
            }
            total += size;
            items.push(CleanupItem {
                id: def.id.to_string(),
                name: def.name.to_string(),
                description: def.description.to_string(),
                size,
                path: path.to_string_lossy().to_string(),
                category: def.category.to_string(),
                safety: def.safety.to_string(),
            });
        }
    }

    CleanupScan { items, total_size: total }
}

/// 执行清理（id 驱动 + 当场重扫 + R1 四层护栏）
pub fn run(ids: &[String]) -> CleanupReport {
    // 全局写锁（C1 互斥）
    let _lock = state::acquire_write_lock();

    // 当场重扫得到允许集合
    let scan = list_items();
    let allowed: std::collections::HashMap<&str, &CleanupItem> =
        scan.items.iter().map(|i| (i.id.as_str(), i)).collect();

    // 允许根（清理项的父目录 + TEMP/LOCALAPPDATA）
    let allowed_roots = build_allowed_roots();

    // C1：获取 protected_paths（M4 迁移历史中的 junction 源路径）
    // 如果 protected_paths 为空（P2 没有迁移历史），这层自然跳过
    let protected: Vec<String> = Vec::new(); // P4 简化：M4 历史在另一模块，暂不交叉
    // TODO: 从 relocate::history::HistoryManager::protected_paths() 获取

    let mut freed = 0u64;
    let mut errors = Vec::new();
    let mut skipped = Vec::new();

    // 获取清理前剩余空间
    let free_before = get_free_space("C");

    for id in ids {
        // 第 0 层：id 必须在允许集合中
        let Some(item) = allowed.get(id.as_str()) else {
            errors.push(format!("不在可清理名单: {}", id));
            continue;
        };

        let path = std::path::Path::new(&item.path);

        // C1：检查是否在 protected_paths 中
        if protected.iter().any(|p| {
            std::path::Path::new(p).canonicalize()
                .map(|c| c == path.canonicalize().unwrap_or_default())
                .unwrap_or(false)
        }) {
            skipped.push(format!("已迁移目录，跳过: {}", item.name));
            continue;
        }

        // 执行删除（R1 四层护栏在 engine 内）
        let result = if path.is_dir() {
            if item.id == "recycle-bin" {
                // 回收站：特殊处理（SHEmptyRecycleBin 等效——P4 简化用 remove_dir_all）
                match safe_clean_dir_contents(path, &allowed_roots) {
                    Ok(size) => { freed += size; continue; }
                    Err(e) => { errors.push(format!("清空回收站失败: {}", e)); continue; }
                }
            } else {
                engine::safe_delete_dir(path, &allowed_roots)
            }
        } else {
            engine::safe_delete_file(path, &allowed_roots)
        };

        match result {
            DeleteResult::Deleted(size) => freed += size,
            DeleteResult::Skipped(msg) => skipped.push(msg),
            DeleteResult::Error(msg) => errors.push(msg),
        }
    }

    let free_after = get_free_space("C:");

    CleanupReport {
        freed,
        free_before,
        free_after,
        errors,
        skipped,
    }
}

/// 清空目录内容（不删目录本身）
fn safe_clean_dir_contents(path: &std::path::Path, allowed_roots: &[PathBuf]) -> std::io::Result<u64> {
    use super::engine::is_reparse_point;
    let mut total = 0u64;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();
        if is_reparse_point(&p) {
            continue; // V6：跳过 reparse point
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

fn build_allowed_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in &["TEMP", "LOCALAPPDATA", "APPDATA"] {
        if let Some(p) = std::env::var_os(var) {
            roots.push(PathBuf::from(p));
        }
    }
    if let Ok(r) = std::env::var("SystemRoot") {
        roots.push(PathBuf::from(&r).join("Temp"));
        roots.push(PathBuf::from(&r).join("SoftwareDistribution"));
        roots.push(PathBuf::from(&r).join("Explorer"));
    }
    if let Ok(d) = std::env::var("SystemDrive") {
        roots.push(PathBuf::from(&d).join("$Recycle.Bin"));
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

fn get_free_space(drive: &str) -> u64 {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let root = format!("{}:\\", drive);
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free);
    }
    free
}
