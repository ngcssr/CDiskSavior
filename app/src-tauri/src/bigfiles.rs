//! 大文件扫描：全盘大文件榜单（light-c big_files 思路，jwalk 后端）
//!
//! 阈值过滤 + 大小降序 + 取消支持 + 大文件直通打开所在目录

use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: i64,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LargeFileReport {
    pub entries: Vec<LargeFileEntry>,
    pub total_found: usize,
    pub scanned_files: usize,
    pub duration_ms: u64,
    pub cancelled: bool,
    pub truncated: bool,
}

const MAX_RESULTS: usize = 200;

pub fn cancel() {
    CANCELLED.store(true, Ordering::Relaxed);
}

/// 扫描大文件（min_size 字节以上的文件，降序 Top 200）
pub fn scan_large_files(root: &str, min_size: u64) -> LargeFileReport {
    CANCELLED.store(false, Ordering::Relaxed);
    let t0 = std::time::Instant::now();
    let root_path = Path::new(root);
    let mut entries = Vec::new();
    let mut total_found = 0usize;
    let mut scanned = 0usize;

    for entry in jwalk::WalkDir::new(root_path).follow_links(false).into_iter().flatten() {
        if CANCELLED.load(Ordering::Relaxed) {
            return LargeFileReport {
                entries, total_found, scanned_files: scanned,
                duration_ms: t0.elapsed().as_millis() as u64,
                cancelled: true, truncated: false,
            };
        }
        if !entry.file_type().is_file() { continue; }
        scanned += 1;
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size < min_size { continue; }
        total_found += 1;
        let path = entry.path();
        let modified = entry.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        entries.push(LargeFileEntry {
            path: path.to_string_lossy().to_string(),
            name: entry.file_name().to_string_lossy().to_string(),
            size,
            modified,
            dir: path.parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        });
        // 保持有序并截断（避免海量结果撑爆 IPC）
        if entries.len() > MAX_RESULTS * 2 {
            entries.sort_by(|a, b| b.size.cmp(&a.size));
            entries.truncate(MAX_RESULTS);
        }
    }

    entries.sort_by(|a, b| b.size.cmp(&a.size));
    let truncated = entries.len() > MAX_RESULTS;
    entries.truncate(MAX_RESULTS);
    LargeFileReport {
        entries, total_found, scanned_files: scanned,
        duration_ms: t0.elapsed().as_millis() as u64,
        cancelled: false, truncated,
    }
}
