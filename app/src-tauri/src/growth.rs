//! 热点目录 + 磁盘增长对比：移植自 light-c hotspot + disk_growth
//!
//! hotspot：AppData 大目录排行（大小/文件数/缓存标记/智能下钻）
//! disk_growth：快照基线 → 对比差异（significant/fast/new/decreased 分级）

use serde::Serialize;
use std::path::{Path, PathBuf};

// ---------- 热点目录 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotspotEntry {
    pub path: String,
    pub name: String,
    pub total_size: u64,
    pub file_count: usize,
    pub last_modified: i64,
    pub is_cache: bool,
    pub children: Vec<HotspotEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotspotReport {
    pub entries: Vec<HotspotEntry>,
    pub scanned_folders: usize,
    pub duration_ms: u64,
}

const CACHE_KEYWORDS: &[&str] = &["cache", "caches", "tmp", "temp", "log", "logs", "download", "thumb", "webview"];

fn is_cache_dir(name: &str) -> bool {
    let n = name.to_lowercase();
    CACHE_KEYWORDS.iter().any(|k| n.contains(k))
}

/// 目录统计（大小 + 文件数 + 最晚修改时间）
fn dir_stats(path: &Path) -> (u64, usize, i64) {
    let mut size = 0u64;
    let mut count = 0usize;
    let mut latest = 0i64;
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            let m = entry.metadata();
            if let Ok(meta) = m {
                size += meta.len();
                count += 1;
                if let Ok(modified) = meta.modified() {
                    if let Ok(d) = modified.duration_since(std::time::UNIX_EPOCH) {
                        latest = latest.max(d.as_millis() as i64);
                    }
                }
            }
        }
    }
    (size, count, latest)
}

fn make_entry(path: &Path, depth: u8) -> HotspotEntry {
    let name = path.file_name().map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let (size, count, latest) = dir_stats(path);
    let mut children = Vec::new();
    // 智能下钻：>5GB 且 >1000 文件时展示前 3 大子目录
    if depth == 0 && size > 5 * 1024 * 1024 * 1024 && count > 1000 {
        if let Ok(read) = std::fs::read_dir(path) {
            let mut subs: Vec<HotspotEntry> = read.flatten()
                .filter(|e| e.path().is_dir())
                .filter(|e| !super::cleaner::engine::is_reparse_point(&e.path()))
                .map(|e| make_entry(&e.path(), depth + 1))
                .collect();
            subs.sort_by(|a, b| b.total_size.cmp(&a.total_size));
            subs.truncate(3);
            children = subs;
        }
    }
    HotspotEntry {
        path: path.to_string_lossy().to_string(),
        name: name.clone(),
        total_size: size,
        file_count: count,
        last_modified: latest,
        is_cache: is_cache_dir(&name),
        children,
    }
}

/// 热点扫描（AppData 三根 + 可选指定路径）
pub fn scan_hotspot(custom_root: Option<&str>) -> HotspotReport {
    let t0 = std::time::Instant::now();
    let roots: Vec<PathBuf> = match custom_root {
        Some(r) => vec![PathBuf::from(r)],
        None => {
            let mut v = Vec::new();
            for var in &["LOCALAPPDATA", "APPDATA"] {
                if let Some(p) = std::env::var_os(var) { v.push(PathBuf::from(p)); }
            }
            if let Some(u) = std::env::var_os("USERPROFILE") {
                v.push(PathBuf::from(u).join("AppData").join("LocalLow"));
            }
            v
        }
    };

    let mut entries = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        let Ok(read) = std::fs::read_dir(&root) else { continue };
        for e in read.flatten() {
            let p = e.path();
            if !p.is_dir() { continue; }
            if super::cleaner::engine::is_reparse_point(&p) { continue; }
            scanned += 1;
            let entry = make_entry(&p, 0);
            if entry.total_size > 10 * 1024 * 1024 { // >10MB 才上榜
                entries.push(entry);
            }
        }
    }

    entries.sort_by(|a, b| b.total_size.cmp(&a.total_size));
    entries.truncate(30);
    HotspotReport { entries, scanned_folders: scanned, duration_ms: t0.elapsed().as_millis() as u64 }
}

// ---------- 磁盘增长（快照对比） ----------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    created_at: u64,
    drive: String,
    /// path → size
    entries: Vec<(String, u64)>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthEntry {
    pub path: String,
    pub name: String,
    pub old_size: u64,
    pub new_size: u64,
    pub diff: i64,
    pub diff_percent: f64,
    pub level: String, // significant / fast / new / decreased / stable
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthReport {
    pub entries: Vec<GrowthEntry>,
    pub total_growth: i64,
    pub baseline_at: u64,
    pub time_span_hours: f64,
    pub has_baseline: bool,
}

fn snapshot_file(drive: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("CDiskSavior");
    let _ = std::fs::create_dir_all(&dir);
    let safe = drive.replace(['\\', ':', '/'], "_");
    Some(dir.join(format!("snapshot-{}.json", safe)))
}

/// 扫描盘符根 + 一级子目录的大小（快照数据点）
fn scan_drive_levels(root: &str) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    let root_path = Path::new(root);
    if let Ok(read) = std::fs::read_dir(root_path) {
        for e in read.flatten() {
            let p = e.path();
            if !p.is_dir() { continue; }
            if super::cleaner::engine::is_reparse_point(&p) { continue; }
            let (size, _, _) = dir_stats(&p);
            out.push((p.to_string_lossy().to_string(), size));
        }
    }
    out
}

/// 创建基线快照
pub fn create_snapshot(drive: &str) -> Result<u64, String> {
    let entries = scan_drive_levels(drive);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let snap = Snapshot { created_at: now, drive: drive.to_string(), entries };
    let file = snapshot_file(drive).ok_or("无法定位快照目录")?;
    crate::state::atomic_write_json(&file, &snap).map_err(|e| e.to_string())?;
    Ok(now)
}

/// 对比当前与基线
pub fn compare_growth(drive: &str) -> Result<GrowthReport, String> {
    let file = snapshot_file(drive).ok_or("尚未创建基线快照")?;
    let (snap, _) = crate::state::read_json_self_heal::<Snapshot>(&file)
        .map_err(|e| e.to_string())?
        .ok_or("基线快照损坏")?;

    let current = scan_drive_levels(drive);
    let old_map: std::collections::HashMap<String, u64> = snap.entries.into_iter().collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs()).unwrap_or(0);
    let span_hours = (now.saturating_sub(snap.created_at)) as f64 / 3600.0;

    let mut entries = Vec::new();
    let mut total_growth = 0i64;

    for (path, new_size) in &current {
        let old_size = old_map.get(path).copied().unwrap_or(0);
        let diff = *new_size as i64 - old_size as i64;
        if diff == 0 { continue; }
        let diff_percent = if old_size > 0 { diff as f64 / old_size as f64 * 100.0 } else { 100.0 };
        let name = Path::new(path).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let level = if old_size == 0 && *new_size > 0 {
            "new"
        } else if diff < 0 {
            "decreased"
        } else if diff > 1024 * 1024 * 1024 || diff_percent > 50.0 {
            "significant"
        } else if diff > 100 * 1024 * 1024 || diff_percent > 20.0 {
            "fast"
        } else {
            "stable"
        };
        if level != "stable" {
            total_growth += diff.max(0);
            entries.push(GrowthEntry {
                path: path.clone(), name, old_size, new_size: *new_size,
                diff, diff_percent, level: level.to_string(),
            });
        }
    }

    entries.sort_by(|a, b| b.diff.cmp(&a.diff));
    entries.truncate(50);
    Ok(GrowthReport { entries, total_growth, baseline_at: snap.created_at, time_span_hours: span_hours, has_baseline: true })
}

/// 是否已有基线
pub fn has_baseline(drive: &str) -> bool {
    snapshot_file(drive).map(|f| f.exists()).unwrap_or(false)
}
