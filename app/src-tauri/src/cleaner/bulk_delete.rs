//! 批量删除命令的支撑模块（light-c delete.rs 系移植，R1 四层护栏版）
//!
//! 与 light-c 的关键差异（按合并设计 R1）：
//! - light-c delete_engine 第 2 层"只 warn 不拒绝" → 本实现强制拒绝（V28）
//! - 前端传路径，但允许集合在后端**当场重扫**（深度垃圾 18 类根 / 卸载残留清单），
//!   落在集合外的路径一律 Skipped，不做"尽力删除"
//! - 回收站删除（trash crate）与永久删除（R1 唯一原语）明确分离

use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletePathsReport {
    pub success_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub freed: u64,
    pub errors: Vec<String>,
    pub skipped: Vec<String>,
}

impl DeletePathsReport {
    fn new() -> Self {
        Self {
            success_count: 0, failed_count: 0, skipped_count: 0,
            freed: 0, errors: vec![], skipped: vec![],
        }
    }
}

/// 深度垃圾 18 类的全部展开根（delete_files / enhanced_delete_files 的允许集合）
fn deep_junk_roots() -> Vec<PathBuf> {
    super::deep_junk::all_scan_roots()
}

/// 卸载残留扫描的当前清单（delete_leftover_* 的允许集合，当场重扫）
fn leftover_paths() -> Vec<PathBuf> {
    let scan = super::leftovers::scan();
    scan.high
        .iter()
        .chain(scan.suspicious.iter())
        .map(|e| PathBuf::from(&e.path))
        .collect()
}

/// 路径是否命中允许集合（自身或任一祖先等于集合根；canonicalize 对齐）
fn in_allowed_set(path: &std::path::Path, allowed: &[PathBuf]) -> bool {
    super::engine::is_safe_path(path, allowed)
}

/// 回收站删除文件/目录（四层护栏 + trash crate）
fn trash_delete(path: &PathBuf, allowed: &[PathBuf]) -> (bool, Option<String>) {
    if super::engine::is_reparse_point(path) {
        return (false, Some(format!("跳过 reparse point: {}", path.display())));
    }
    if !in_allowed_set(path, allowed) {
        return (false, Some(format!("跳过允许集合外路径: {}", path.display())));
    }
    match trash::delete(path) {
        Ok(()) => (true, None),
        Err(e) => (false, Some(format!("移入回收站失败 {}: {}", path.display(), e))),
    }
}

/// delete_files：深度垃圾文件列表移入回收站
pub fn delete_files(paths: &[String]) -> DeletePathsReport {
    let _lock = crate::state::acquire_write_lock();
    let allowed = deep_junk_roots();
    let mut r = DeletePathsReport::new();
    for p in paths {
        let path = PathBuf::from(p);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        match trash_delete(&path, &allowed) {
            (true, _) => {
                r.success_count += 1;
                r.freed += size;
            }
            (false, Some(msg)) => {
                if msg.starts_with("跳过") {
                    r.skipped_count += 1;
                    r.skipped.push(msg);
                } else {
                    r.failed_count += 1;
                    r.errors.push(msg);
                }
            }
            (false, None) => {}
        }
    }
    r
}

/// enhanced_delete_files：永久删除 + 占用重试（3 次 ×200ms）+ 释放统计
pub fn enhanced_delete_files(paths: &[String]) -> DeletePathsReport {
    let _lock = crate::state::acquire_write_lock();
    let allowed = deep_junk_roots();
    let mut r = DeletePathsReport::new();
    for p in paths {
        let path = PathBuf::from(p);
        let mut attempts = 0u8;
        loop {
            attempts += 1;
            match super::engine::safe_delete_file(&path, &allowed) {
                super::engine::DeleteResult::Deleted(s) => {
                    r.success_count += 1;
                    r.freed += s;
                    break;
                }
                super::engine::DeleteResult::Skipped(m) => {
                    r.skipped_count += 1;
                    r.skipped.push(m);
                    break;
                }
                super::engine::DeleteResult::Error(m) => {
                    if attempts < 3 {
                        // 占用中的文件短暂等待后重试（杀软/索引服务瞬时占用常见）
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        continue;
                    }
                    r.failed_count += 1;
                    r.errors.push(m);
                    break;
                }
            }
        }
    }
    r
}

/// delete_leftover_folders：卸载残留文件夹移入回收站
pub fn delete_leftover_folders(paths: &[String]) -> DeletePathsReport {
    let _lock = crate::state::acquire_write_lock();
    let allowed = leftover_paths();
    let mut r = DeletePathsReport::new();
    for p in paths {
        let path = PathBuf::from(p);
        let size = dir_size(&path);
        match trash_delete(&path, &allowed) {
            (true, _) => {
                r.success_count += 1;
                r.freed += size;
            }
            (false, Some(msg)) => {
                if msg.starts_with("跳过") {
                    r.skipped_count += 1;
                    r.skipped.push(msg);
                } else {
                    r.failed_count += 1;
                    r.errors.push(msg);
                }
            }
            (false, None) => {}
        }
    }
    r
}

/// delete_leftovers_permanent：卸载残留文件夹永久删除（R1 唯一原语）
pub fn delete_leftovers_permanent(paths: &[String]) -> DeletePathsReport {
    let _lock = crate::state::acquire_write_lock();
    let allowed = leftover_paths();
    let mut r = DeletePathsReport::new();
    for p in paths {
        let path = PathBuf::from(p);
        match super::engine::safe_delete_dir(&path, &allowed) {
            super::engine::DeleteResult::Deleted(s) => {
                r.success_count += 1;
                r.freed += s;
            }
            super::engine::DeleteResult::Skipped(m) => {
                r.skipped_count += 1;
                r.skipped.push(m);
            }
            super::engine::DeleteResult::Error(m) => {
                r.failed_count += 1;
                r.errors.push(m);
            }
        }
    }
    r
}

/// cleanup_directory_contents：清空深度垃圾类别目录内容（保留目录本身）
/// 允许集合 = 深度垃圾 18 类展开根中的**目录本身**（精确命中，不是祖先）
pub fn cleanup_directory_contents(path: &str) -> DeletePathsReport {
    let _lock = crate::state::acquire_write_lock();
    let allowed = deep_junk_roots();
    let target = PathBuf::from(path);
    let mut r = DeletePathsReport::new();

    // 精确命中校验：目标目录必须本身就是某个类别根（canonicalize 后相等）
    let canon_target = std::fs::canonicalize(&target).unwrap_or_else(|_| target.clone());
    let exact_hit = allowed.iter().any(|root| {
        std::fs::canonicalize(root).unwrap_or_else(|_| root.clone()) == canon_target
    });
    if !exact_hit {
        r.errors.push(format!("拒绝清空非深度垃圾类别目录: {}", target.display()));
        r.failed_count = 1;
        return r;
    }
    if super::engine::is_reparse_point(&target) {
        r.skipped.push(format!("跳过 reparse point: {}", target.display()));
        r.skipped_count = 1;
        return r;
    }

    let size = dir_size(&target);
    match super::engine::clear_dir_contents(&target) {
        Ok(()) => {
            r.success_count = 1;
            r.freed = size;
        }
        Err(e) => {
            r.failed_count = 1;
            r.errors.push(format!("清空目录失败 {}: {}", target.display(), e));
        }
    }
    r
}

fn dir_size(path: &std::path::Path) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_files_rejects_outside_allowed_set() {
        // System32 显然不在深度垃圾类别根内
        let r = delete_files(&[r"C:\Windows\System32\kernel32.dll".to_string()]);
        assert_eq!(r.success_count, 0);
        assert!(r.skipped_count > 0 || r.failed_count > 0);
    }

    #[test]
    fn cleanup_directory_contents_rejects_arbitrary_dir() {
        let r = cleanup_directory_contents(r"C:\Windows");
        assert_eq!(r.success_count, 0);
        assert_eq!(r.failed_count, 1);
        assert!(r.errors[0].contains("拒绝清空"));
    }
}
