//! 热点目录 + 磁盘增长对比：移植自 light-c hotspot + disk_growth
//!
//! hotspot：AppData 大目录排行（大小/文件数/缓存标记/智能下钻）
//! disk_growth：快照基线 → 对比差异（significant/fast/new/decreased 分级）

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// ---------- 统一 MFT 数据缓存（一次读取，热点/增长/大文件共享） ----------
// 防止并发 $MFT 读盘争抢（TreeMap 后台扫 + 用户点 tab 触发第二路读 = I/O 互抢）。
// 缓存有效期 5 分钟；扫描锁保证同盘同时只有一路 $MFT 读取。

struct MftDataCache {
    drive: char,
    timestamp: std::time::Instant,
    stats: crate::lightc_mft::FullDriveStats,
}

/// 全盘 MFT 加载互斥锁：scan_mft（目录树）与 get_mft_stats（热点/
/// 大文件/增长）共用的读盘锁——并发时后到者等待并复用先到者的
/// raw 缓存，而不是各读各的盘（双份 MFT 并发读实测互拖 24-29s）。
pub static MFT_SCAN_LOCK: Mutex<()> = Mutex::new(());
static MFT_DATA_CACHE: Mutex<Option<MftDataCache>> = Mutex::new(None);

const MFT_CACHE_TTL_SECS: u64 = 300;

/// 获取（或复用）全盘 MFT 统计——热点/增长/大文件统一入口。
/// 三级路径：① stats 缓存命中 → ② raw 缓存派生（TreeMap 已加载）→ ③ 加载解析。
pub fn get_mft_stats(drive: char) -> Result<crate::lightc_mft::FullDriveStats, String> {
    let t_all = std::time::Instant::now();
    let drive = drive.to_ascii_uppercase();
    let root_norm = format!("{}:\\", drive);
    crate::scan_log(&format!("get_mft_stats start drive={}", drive));
    // ① stats 缓存快路径
    if let Ok(guard) = MFT_DATA_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if cache.drive == drive && cache.timestamp.elapsed().as_secs() < MFT_CACHE_TTL_SECS {
                crate::scan_log(&format!("get_mft_stats ①stats-cache hit {:?}", t_all.elapsed()));
                return Ok(cache.stats.clone());
            }
        }
    }
    // ② raw 缓存（TreeMap 已加载过原始数据）→ 直接派生 stats
    if let Some((mft, rec)) = get_cached_mft_raw(drive) {
        let t_d = std::time::Instant::now();
        let stats = crate::mft_scan::derive_stats_from_records(&mft, &rec, &root_norm);
        crate::scan_log(&format!("get_mft_stats ②raw-cache derive {:?} total {:?}", t_d.elapsed(), t_all.elapsed()));
        cache_mft_stats(drive, stats.clone());
        return Ok(stats);
    }
    // ③ 服务管道（SYSTEM 权限代做 MFT）——仅【未提权】时走。
    //    提权时进程内单遍更快（省去服务进程启动 + 管道往返开销），且避免
    //    旧版侧车不识别 scan_stats 命令导致的无谓重试。
    let t_elev = std::time::Instant::now();
    let elevated = crate::lightc_mft::is_elevated();
    let svc_installed = crate::svc_client::service_installed();
    crate::scan_log(&format!(
        "get_mft_stats elevated={} svc_installed={} (checks {:?})",
        elevated, svc_installed, t_elev.elapsed()
    ));
    if !elevated && svc_installed {
        let t_svc = std::time::Instant::now();
        if let Ok(stats) = crate::svc_client::scan_stats_via_service(&root_norm) {
            crate::scan_log(&format!("get_mft_stats ③service {:?} total {:?}", t_svc.elapsed(), t_all.elapsed()));
            cache_mft_stats(drive, stats.clone());
            return Ok(stats);
        }
    }
    // ④ 慢路径：拿扫描锁，进程内加载（需提权；便携版 UAC 开启机器走这里或回退 jwalk）
    let _scan_guard = MFT_SCAN_LOCK.lock().map_err(|_| "扫描锁中毒")?;
    // 双检 stats
    if let Ok(guard) = MFT_DATA_CACHE.lock() {
        if let Some(cache) = guard.as_ref() {
            if cache.drive == drive && cache.timestamp.elapsed().as_secs() < MFT_CACHE_TTL_SECS {
                return Ok(cache.stats.clone());
            }
        }
    }
    // 双检 raw
    if let Some((mft, rec)) = get_cached_mft_raw(drive) {
        let stats = crate::mft_scan::derive_stats_from_records(&mft, &rec, &root_norm);
        cache_mft_stats(drive, stats.clone());
        return Ok(stats);
    }
    // 加载 + 解析 + 双缓存（raw + stats）
    let t_load = std::time::Instant::now();
    let (mft, rec) = crate::mft_scan::load_and_parse(drive, &mut |_, _, _, _| {})?;
    crate::scan_log(&format!("get_mft_stats ④load_and_parse {:?}", t_load.elapsed()));
    let (mft, rec) = take_and_cache_mft_raw(drive, mft, rec);
    let t_d = std::time::Instant::now();
    let stats = crate::mft_scan::derive_stats_from_records(&mft, &rec, &root_norm);
    crate::scan_log(&format!("get_mft_stats ④derive {:?} total {:?}", t_d.elapsed(), t_all.elapsed()));
    cache_mft_stats(drive, stats.clone());
    Ok(stats)
}

/// 使 MFT 缓存失效（用户点"重新扫描"时调用）
pub fn invalidate_mft_cache() {
    if let Ok(mut guard) = MFT_DATA_CACHE.lock() {
        *guard = None;
    }
    if let Ok(mut guard) = MFT_RAW_CACHE.lock() {
        *guard = None;
    }
}

// ---------- 跨 tab 原始 MFT 数据缓存（TreeMap 与 stats 共享一次加载解析） ----------
pub struct MftRawData {
    pub mft: std::sync::Arc<ntfs_reader::mft::Mft>,
    pub rec: std::sync::Arc<crate::mft_scan::Records>,
}

struct MftRawCacheEntry {
    drive: char,
    timestamp: std::time::Instant,
    data: MftRawData,
}

static MFT_RAW_CACHE: Mutex<Option<MftRawCacheEntry>> = Mutex::new(None);

/// 缓存原始 MFT 数据（加载解析后调用）
pub fn cache_mft_raw(drive: char, mft: &ntfs_reader::mft::Mft, rec: &crate::mft_scan::Records) {
    // Mft 无法 clone（含裸卷句柄语义的数据），只能整体转移——调用方需放弃所有权。
    // 此处改为缓存共享快照的轻量方案：仅当无既有缓存时由首个 tab 存入。
    // 由于 Mft 持有 300MB data Vec，为避免复制，只在缓存为空时写入 Arc。
    let _ = (drive, mft, rec); // 见 get_or_take_mft_raw
}

/// 取缓存的原始 MFT 数据（命中返回 (Arc<Mft>, Arc<Records>)）
pub fn get_cached_mft_raw(drive: char) -> Option<(std::sync::Arc<ntfs_reader::mft::Mft>, std::sync::Arc<crate::mft_scan::Records>)> {
    let guard = MFT_RAW_CACHE.lock().ok()?;
    let entry = guard.as_ref()?;
    if entry.drive == drive && entry.timestamp.elapsed().as_secs() < MFT_CACHE_TTL_SECS {
        Some((entry.data.mft.clone(), entry.data.rec.clone()))
    } else {
        None
    }
}

/// 存入原始 MFT 数据（首个 tab 加载后调用；取得所有权入 Arc）
pub fn take_and_cache_mft_raw(
    drive: char,
    mft: ntfs_reader::mft::Mft,
    rec: crate::mft_scan::Records,
) -> (std::sync::Arc<ntfs_reader::mft::Mft>, std::sync::Arc<crate::mft_scan::Records>) {
    let data = MftRawData {
        mft: std::sync::Arc::new(mft),
        rec: std::sync::Arc::new(rec),
    };
    let out = (data.mft.clone(), data.rec.clone());
    if let Ok(mut guard) = MFT_RAW_CACHE.lock() {
        *guard = Some(MftRawCacheEntry {
            drive,
            timestamp: std::time::Instant::now(),
            data,
        });
    }
    out
}

/// 直接存入已构建的 stats（scan_mft 建树后顺带入缓存）
pub fn cache_mft_stats(drive: char, stats: crate::lightc_mft::FullDriveStats) {
    if let Ok(mut guard) = MFT_DATA_CACHE.lock() {
        *guard = Some(MftDataCache {
            drive,
            timestamp: std::time::Instant::now(),
            stats,
        });
    }
}

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

static HOTSPOT_CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 取消热点目录扫描
pub fn cancel_hotspot_scan() {
    HOTSPOT_CANCELLED.store(true, std::sync::atomic::Ordering::Relaxed);
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
/// 热点目录扫描（可取消：cancel_hotspot_scan）
pub fn scan_hotspot(custom_root: Option<&str>) -> HotspotReport {
    use std::sync::atomic::Ordering;
    HOTSPOT_CANCELLED.store(false, Ordering::Relaxed);
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

    // MFT 快路径（light-c 引擎）：全部根同盘 + NTFS + 提权时全盘秒级聚合
    if let Some(report) = scan_hotspot_mft(&roots, &t0) {
        return report;
    }

    let mut entries: Vec<HotspotEntry> = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        let Ok(read) = std::fs::read_dir(&root) else { continue };
        for e in read.flatten() {
            if HOTSPOT_CANCELLED.load(Ordering::Relaxed) {
                entries.sort_by(|a, b| b.total_size.cmp(&a.total_size));
                entries.truncate(30);
                return HotspotReport { entries, scanned_folders: scanned, duration_ms: t0.elapsed().as_millis() as u64 };
            }
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

/// light-c MFT 引擎热点快路径：全盘目录聚合 → 取各根的一级子目录
fn scan_hotspot_mft(roots: &[PathBuf], t0: &std::time::Instant) -> Option<HotspotReport> {
    // 全部根须同盘
    let drive = roots.first()?.to_string_lossy().chars().next()?;
    if !drive.is_ascii_alphabetic()
        || !roots.iter().all(|r| {
            r.to_string_lossy()
                .chars()
                .next()
                .map(|c| c.eq_ignore_ascii_case(&drive))
                .unwrap_or(false)
        })
    {
        return None;
    }
    if !crate::lightc_mft::mft_available(drive) {
        return None;
    }
    let stats = get_mft_stats(drive).ok()?;

    let mut entries: Vec<HotspotEntry> = Vec::new();
    let mut scanned = 0usize;
    for root in roots {
        for (path, (size, count, latest_ms)) in &stats.dir_stats {
            // 根的一级子目录
            let Ok(rel) = path.strip_prefix(root) else { continue };
            if rel.components().count() != 1 {
                continue;
            }
            scanned += 1;
            if *size <= 10 * 1024 * 1024 {
                continue;
            }
            let name = rel.to_string_lossy().to_string();
            // 智能下钻（与 make_entry 同规则：>5GB 且 >1000 文件展示前 3 大子目录）
            let mut children = Vec::new();
            if *size > 5 * 1024 * 1024 * 1024 && *count > 1000 {
                let mut subs: Vec<HotspotEntry> = stats
                    .dir_stats
                    .iter()
                    .filter(|(p2, _)| {
                        p2.strip_prefix(path)
                            .map(|r2| r2.components().count() == 1)
                            .unwrap_or(false)
                    })
                    .map(|(p2, (s2, c2, m2))| HotspotEntry {
                        path: p2.to_string_lossy().to_string(),
                        name: p2.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                        total_size: *s2,
                        file_count: *c2,
                        last_modified: *m2,
                        is_cache: is_cache_dir(&p2.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default()),
                        children: Vec::new(),
                    })
                    .collect();
                subs.sort_by(|a, b| b.total_size.cmp(&a.total_size));
                subs.truncate(3);
                children = subs;
            }
            entries.push(HotspotEntry {
                path: path.to_string_lossy().to_string(),
                name: name.clone(),
                total_size: *size,
                file_count: *count,
                last_modified: *latest_ms,
                is_cache: is_cache_dir(&name),
                children,
            });
        }
    }

    entries.sort_by(|a, b| b.total_size.cmp(&a.total_size));
    entries.truncate(30);
    Some(HotspotReport {
        entries,
        scanned_folders: scanned,
        duration_ms: t0.elapsed().as_millis() as u64,
    })
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
    // 便携模式：数据自包含（exe 旁）；安装版：保持既有 LOCALAPPDATA\CDiskSavior
    let dir = if crate::appdata::portable_mode() {
        crate::appdata::data_dir().join("snapshots")
    } else {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("CDiskSavior")
    };
    let _ = std::fs::create_dir_all(&dir);
    let safe = drive.replace(['\\', ':', '/'], "_");
    Some(dir.join(format!("snapshot-{}.json", safe)))
}

/// 扫描盘符根 + 一级子目录的大小（快照数据点）
fn scan_drive_levels(root: &str) -> Vec<(String, u64)> {
    // MFT 快路径（light-c 引擎）：NTFS + 提权时全盘秒级聚合，取一级子目录
    if let Some(levels) = scan_drive_levels_mft(root) {
        return levels;
    }
    scan_drive_levels_jwalk(root)
}

/// light-c MFT 引擎：全盘目录聚合 → 过滤一级子目录
fn scan_drive_levels_mft(root: &str) -> Option<Vec<(String, u64)>> {
    let letter = root.chars().next().filter(|c| c.is_ascii_alphabetic())?;
    if !crate::lightc_mft::mft_available(letter) {
        return None;
    }
    let stats = get_mft_stats(letter).ok()?;

    let root_path = PathBuf::from(root);
    let mut out: Vec<(String, u64)> = stats
        .dir_stats
        .iter()
        .filter(|(path, _)| {
            path.strip_prefix(&root_path)
                .map(|rel| rel.components().count() == 1)
                .unwrap_or(false)
        })
        .map(|(path, (size, _, _))| (path.to_string_lossy().to_string(), *size))
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    if out.is_empty() { None } else { Some(out) }
}

/// jwalk 遍历路径（原实现）
fn scan_drive_levels_jwalk(root: &str) -> Vec<(String, u64)> {
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

// ============ 全盘变化分析（scan_disk_growth 系） ============

static GROWTH_CANCELLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn cancel_growth_scan() {
    GROWTH_CANCELLED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskScanAndAnalyzeResponse {
    pub growth: GrowthReport,
    pub total_files_scanned: usize,
    pub scan_duration_ms: u64,
    pub drive_letter: String,
}

/// 全盘空间变化分析：无基线时自动创建基线（首轮为空报告），否则对比并返回明细
pub fn scan_disk_growth(drive: &str, max_change_entries: Option<usize>) -> Result<DiskScanAndAnalyzeResponse, String> {
    scan_disk_growth_with_progress(drive, max_change_entries, &|_| {})
}

/// 带进度回调的全盘变化分析（每 1000 个文件回调一次已扫描数）
pub fn scan_disk_growth_with_progress(drive: &str, max_change_entries: Option<usize>, on_progress: &dyn Fn(usize)) -> Result<DiskScanAndAnalyzeResponse, String> {
    use std::sync::atomic::Ordering;
    GROWTH_CANCELLED.store(false, Ordering::Relaxed);
    let t0 = std::time::Instant::now();
    let drive_root = format!("{}\\", drive.trim_end_matches(['\\', ':']).to_ascii_uppercase().trim_end_matches(':'));

    // 无基线：先建基线，返回空报告（与 UI 的"创建基线"流程一致）
    if !has_baseline(&drive_root) {
        create_snapshot(&drive_root)?;
        let (files, _) = count_files_with_progress(&drive_root, on_progress);
        return Ok(DiskScanAndAnalyzeResponse {
            growth: GrowthReport { entries: vec![], total_growth: 0, baseline_at: 0, time_span_hours: 0.0, has_baseline: false },
            total_files_scanned: files,
            scan_duration_ms: t0.elapsed().as_millis() as u64,
            drive_letter: drive_root,
        });
    }

    // 有基线：对比（不截断，max_change_entries 收敛）
    let file = snapshot_file(&drive_root).ok_or("尚未创建基线快照")?;
    let (snap, _) = crate::state::read_json_self_heal::<Snapshot>(&file)
        .map_err(|e| e.to_string())?
        .ok_or("基线快照损坏")?;
    let current = scan_drive_levels(&drive_root);
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
    let max = max_change_entries.unwrap_or(100).clamp(10, 2000);
    entries.truncate(max);
    let (files, _) = count_files_with_progress(&drive_root, on_progress);
    Ok(DiskScanAndAnalyzeResponse {
        growth: GrowthReport { entries, total_growth, baseline_at: snap.created_at, time_span_hours: span_hours, has_baseline: true },
        total_files_scanned: files,
        scan_duration_ms: t0.elapsed().as_millis() as u64,
        drive_letter: drive_root,
    })
}

/// 统计文件数（可取消 + 进度回调；每 1000 个文件回调一次）
fn count_files(root: &str) -> (usize, bool) {
    count_files_with_progress(root, &|_| {})
}

fn count_files_with_progress(root: &str, on_progress: &dyn Fn(usize)) -> (usize, bool) {
    use std::sync::atomic::Ordering;
    let mut n = 0usize;
    for entry in jwalk::WalkDir::new(root).follow_links(false).into_iter().flatten() {
        if GROWTH_CANCELLED.load(Ordering::Relaxed) { return (n, true); }
        if entry.file_type().is_file() {
            n += 1;
            if n % 1000 == 0 { on_progress(n); }
        }
    }
    (n, false)
}

// ---------- 文件级变化明细 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDetailEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: i64,
    pub is_new: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDetailsResponse {
    pub entries: Vec<FileDetailEntry>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
}

/// 文件级变化明细：指定目录下、基线时间之后修改/新增的文件（分页，大小降序）
pub fn get_file_change_details(path: &str, offset: usize, limit: usize, drive: &str) -> Result<FileDetailsResponse, String> {
    use std::sync::atomic::Ordering;
    GROWTH_CANCELLED.store(false, Ordering::Relaxed);
    let root = std::path::PathBuf::from(path);
    if !root.is_dir() {
        return Err("目标目录不存在".into());
    }
    let drive_root = format!("{}\\", drive.trim_end_matches(['\\', ':']).to_ascii_uppercase().trim_end_matches(':'));
    let baseline_at = snapshot_file(&drive_root)
        .and_then(|f| crate::state::read_json_self_heal::<Snapshot>(&f).ok().flatten())
        .map(|(s, _)| s.created_at as i64)
        .unwrap_or(0);

    // MFT 快路径（light-c 引擎）：目标在 NTFS 盘 + 提权时全盘秒级枚举，按 mtime 过滤
    if let Some(letter) = root.to_string_lossy().chars().next().filter(|c| c.is_ascii_alphabetic()) {
        if crate::lightc_mft::mft_available(letter) {
            if let Ok(stats) = get_mft_stats(letter) {
                let root_str = root.to_string_lossy().to_string();
                let mut entries: Vec<FileDetailEntry> = stats
                    .files
                    .into_iter()
                    .filter(|(p, _, mtime)| {
                        p.starts_with(&root_str) && (baseline_at <= 0 || *mtime >= baseline_at)
                    })
                    .map(|(p, size, mtime)| FileDetailEntry {
                        name: std::path::Path::new(&p)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or_default(),
                        path: p,
                        size,
                        modified: mtime,
                        is_new: true,
                    })
                    .collect();
                entries.sort_by(|a, b| b.size.cmp(&a.size));
                let total = entries.len();
                let limit = limit.clamp(1, 2000);
                let page: Vec<FileDetailEntry> = entries.into_iter().skip(offset).take(limit).collect();
                return Ok(FileDetailsResponse { entries: page, total_count: total, offset, limit });
            }
        }
    }

    let mut entries = Vec::new();
    for entry in jwalk::WalkDir::new(&root).follow_links(false).into_iter().flatten() {
        if GROWTH_CANCELLED.load(Ordering::Relaxed) { break; }
        if !entry.file_type().is_file() { continue; }
        let mtime = entry.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // 基线后修改/新增的文件才入列；无基线时全部入列
        if baseline_at > 0 && mtime < baseline_at { continue; }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        let p = entry.path();
        entries.push(FileDetailEntry {
            path: p.to_string_lossy().to_string(),
            name: entry.file_name().to_string_lossy().to_string(),
            size,
            modified: mtime,
            is_new: true,
        });
    }
    entries.sort_by(|a, b| b.size.cmp(&a.size));
    let total = entries.len();
    let limit = limit.clamp(1, 2000);
    let page: Vec<FileDetailEntry> = entries.into_iter().skip(offset).take(limit).collect();
    Ok(FileDetailsResponse { entries: page, total_count: total, offset, limit })
}

// ---------- 目录级变化明细 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirDetailEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub file_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirDetailsResponse {
    pub entries: Vec<DirDetailEntry>,
    pub total_count: usize,
    pub offset: usize,
    pub limit: usize,
}

/// 目录级明细：指定目录的直接子目录（大小+文件数，分页，大小降序）
pub fn get_directory_change_details(path: &str, offset: usize, limit: usize) -> Result<DirDetailsResponse, String> {
    let root = std::path::PathBuf::from(path);
    if !root.is_dir() {
        return Err("目标目录不存在".into());
    }
    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(&root) {
        for e in read.flatten() {
            let p = e.path();
            if !p.is_dir() { continue; }
            if super::cleaner::engine::is_reparse_point(&p) { continue; }
            let (size, count, _) = dir_stats(&p);
            entries.push(DirDetailEntry {
                path: p.to_string_lossy().to_string(),
                name: e.file_name().to_string_lossy().to_string(),
                size, file_count: count,
            });
        }
    }
    entries.sort_by(|a, b| b.size.cmp(&a.size));
    let total = entries.len();
    let limit = limit.clamp(1, 2000);
    let page: Vec<DirDetailEntry> = entries.into_iter().skip(offset).take(limit).collect();
    Ok(DirDetailsResponse { entries: page, total_count: total, offset, limit })
}

// ---------- HTML 导出目录树 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportNode {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub file_count: usize,
    pub children: Vec<ExportNode>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportTreeResponse {
    pub nodes: Vec<ExportNode>,
    pub max_depth: usize,
}

/// HTML 导出目录树：多根路径，限制深度（默认 3，1-6）
pub fn get_export_tree(paths: &[String], max_depth: Option<usize>) -> ExportTreeResponse {
    let depth = max_depth.unwrap_or(3).clamp(1, 6);
    let nodes = paths
        .iter()
        .map(|p| export_node(std::path::Path::new(p), depth))
        .collect();
    ExportTreeResponse { nodes, max_depth: depth }
}

fn export_node(path: &Path, depth: usize) -> ExportNode {
    let name = path.file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let mut children = Vec::new();
    if depth > 0 && path.is_dir() && !super::cleaner::engine::is_reparse_point(path) {
        if let Ok(read) = std::fs::read_dir(path) {
            let mut subs: Vec<_> = read.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect();
            // 每层最多 50 个子目录（按名称序，导出报告的可读性边界）
            subs.sort();
            subs.truncate(50);
            children = subs.into_iter().map(|p| export_node(&p, depth - 1)).collect();
        }
    }
    let (size, count) = if path.is_dir() { let (s, c, _) = dir_stats(path); (s, c) } else { (0, 0) };
    ExportNode {
        path: path.to_string_lossy().to_string(),
        name, size, file_count: count, children,
    }
}
