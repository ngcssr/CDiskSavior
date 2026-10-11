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
    scan_large_files_with_progress(root, min_size, &|_| {})
}

/// 带进度回调的大文件扫描（每 2000 个文件回调一次已扫描数）
pub fn scan_large_files_with_progress(root: &str, min_size: u64, on_progress: &dyn Fn(usize)) -> LargeFileReport {
    // MFT 快路径（light-c 引擎）：目标是盘符根 + NTFS + 已提权时秒级完成；
    // 任意失败/取消/非适用条件回退 jwalk 遍历路径（行为与原路径完全一致）。
    if let Some(drive) = drive_root_letter(root) {
        if crate::lightc_mft::mft_available(drive) {
            match scan_large_files_mft(drive, min_size, on_progress) {
                Ok(report) => return report,
                Err(_) => { /* 回退遍历 */ }
            }
        }
    }
    scan_large_files_jwalk(root, min_size, on_progress)
}

/// 提取盘符根（"C:\\" / "C:" / "c:\\xxx" 中的盘符；仅当 root 恰为盘符根时返回 Some）
fn drive_root_letter(root: &str) -> Option<char> {
    let t = root.trim_end_matches(['\\', '/']);
    if t.len() == 2 && t.as_bytes()[1] == b':' {
        let c = t.chars().next()?;
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase());
        }
    }
    None
}

/// light-c MFT 引擎大文件快路径：USN 枚举 + $MFT 顺序读 + Top-200
fn scan_large_files_mft(drive: char, min_size: u64, on_progress: &dyn Fn(usize)) -> Result<LargeFileReport, String> {
    CANCELLED.store(false, Ordering::Relaxed);
    let t0 = std::time::Instant::now();
    let cancelled_check = |_stage: &str, processed: usize| {
        if CANCELLED.load(Ordering::Relaxed) {
            return false;
        }
        on_progress(processed);
        true
    };

    // 阈值过滤在 MFT Top-N 之后做（Top-N 取 200 上限，通常远大于阈值筛选结果）
    let mut files = crate::lightc_mft::scan_top_files(drive, MAX_RESULTS, &cancelled_check)
        .map_err(|e| e)?;
    files.retain(|(_, size, _)| *size >= min_size);

    let entries: Vec<LargeFileEntry> = files
        .into_iter()
        .map(|(path, size, modified)| {
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let dir = std::path::Path::new(&path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            LargeFileEntry { path, name, size, modified, dir }
        })
        .collect();
    let total_found = entries.len();

    Ok(LargeFileReport {
        entries,
        total_found,
        scanned_files: 0, // MFT 路径不逐一统计（全盘记录数无意义暴露）
        duration_ms: t0.elapsed().as_millis() as u64,
        cancelled: false,
        truncated: false,
    })
}

/// jwalk 遍历路径（原实现）
fn scan_large_files_jwalk(root: &str, min_size: u64, on_progress: &dyn Fn(usize)) -> LargeFileReport {
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
        if scanned % 2000 == 0 { on_progress(scanned); }
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
