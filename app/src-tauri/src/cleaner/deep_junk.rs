//! 深度垃圾扫描：移植自 light-c deep_junk + categories
//!
//! 类别资产完整移植（19 类扫描路径 + 文件模式 + glob 展开），
//! 扫描后端用 walkdir（light-c 的 MFT 后端依赖服务，留待后续）；
//! 删除走 R1 统一引擎（id 驱动 + 当场重扫）。
//!
//! 与快速清理的分工：快速清理是"已注册 46 项目录级"，
//! 深度扫描是"19 类文件级匹配"（全分区 + glob + 模式过滤）。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

static CANCELLED: AtomicBool = AtomicBool::new(false);

/// 单类扫描结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResult {
    pub id: String,
    pub name: String,
    pub description: String,
    pub risk: u8,           // 1 最安全 ~ 3 需谨慎
    pub size: u64,
    pub file_count: usize,
    pub system_managed: bool, // Windows.old：引导系统工具，不直接删
    pub top_files: Vec<FileEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepScanResult {
    pub categories: Vec<CategoryResult>,
    pub total_size: u64,
    pub total_files: usize,
    pub duration_ms: u64,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepDeleteReport {
    pub freed: u64,
    pub errors: Vec<String>,
}

// ---------- 路径规范 ----------

enum PathSpec {
    Env(&'static str, &'static str),       // 环境变量 + 子路径（"" 表示变量本身）
    Glob(&'static str, &'static str),      // 环境变量 + 含 * 的相对模式
    DriveRoot(&'static str),               // 系统盘根 + 子路径
    AllDrives(&'static str),               // 全部盘符 + 子路径
}

fn expand(spec: &PathSpec) -> Vec<PathBuf> {
    match spec {
        PathSpec::Env(var, sub) => {
            let base = std::env::var_os(var).map(PathBuf::from).unwrap_or_default();
            if base.as_os_str().is_empty() { vec![] } else if sub.is_empty() { vec![base] } else { vec![base.join(sub)] }
        }
        PathSpec::DriveRoot(sub) => {
            let d = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
            vec![PathBuf::from(d).join(sub)]
        }
        PathSpec::AllDrives(sub) => ('A'..='Z')
            .filter(|l| PathBuf::from(format!("{}:\\", l)).exists())
            .map(|l| PathBuf::from(format!("{}:\\", l)).join(sub))
            .collect(),
        PathSpec::Glob(var, pattern) => {
            let base = std::env::var_os(var).map(PathBuf::from).unwrap_or_default();
            if base.as_os_str().is_empty() { return vec![]; }
            expand_glob(&base, pattern)
        }
    }
}

/// 单层/多层 glob 展开（仅支持段内 * 通配）
fn expand_glob(base: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut current = vec![base.to_path_buf()];
    for seg in pattern.split('\\') {
        let mut next = Vec::new();
        if seg.contains('*') {
            for dir in &current {
                if let Ok(read) = std::fs::read_dir(dir) {
                    for e in read.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if glob_match(seg, &name) {
                            next.push(e.path());
                        }
                    }
                }
            }
        } else {
            for dir in &current {
                next.push(dir.join(seg));
            }
        }
        current = next;
    }
    current
}

/// 简单通配匹配（* 任意序列）
fn glob_match(pat: &str, name: &str) -> bool {
    let pat = pat.to_lowercase();
    let name = name.to_lowercase();
    let parts: Vec<&str> = pat.split('*').collect();
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        match name[pos..].find(part) {
            Some(idx) => {
                if i == 0 && idx != 0 { return false; } // 首段必须从开头
                pos += idx + part.len();
            }
            None => return false,
        }
    }
    // 末段非空时必须匹配到结尾
    if let Some(last) = parts.last() {
        if !last.is_empty() && !name.ends_with(last) { return false; }
    }
    true
}

// ---------- 类别定义（19 类，light-c categories.rs 完整移植） ----------

struct CatDef {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    risk: u8,
    system_managed: bool,
    paths: &'static [PathSpec],
    patterns: &'static [&'static str],
}

fn categories() -> Vec<CatDef> {
    use PathSpec::*;
    vec![
        CatDef { id: "windows-temp", name: "Windows 临时文件", desc: "%TEMP% 与系统 Temp 的全部文件", risk: 1, system_managed: false,
            paths: &[Env("TEMP", ""), Env("TMP", ""), Env("SYSTEMROOT", "Temp")], patterns: &["*"] },
        CatDef { id: "system-cache", name: "系统缓存", desc: "Prefetch 预读取 + 应用缓存", risk: 3, system_managed: false,
            paths: &[Env("SYSTEMROOT", "Prefetch"), Env("LOCALAPPDATA", "Microsoft\\Windows\\Caches")], patterns: &["*"] },
        CatDef { id: "browser-cache", name: "浏览器缓存（全品牌）", desc: "Chrome/Edge/Firefox/Brave/Opera/QQ/360/UC/Vivaldi/Yandex/百分，含多 Profile", risk: 2, system_managed: false,
            paths: &[
                Env("LOCALAPPDATA", "Microsoft\\Windows\\INetCache"),
                Glob("LOCALAPPDATA", "Google\\Chrome\\User Data\\*\\Cache"),
                Glob("LOCALAPPDATA", "Google\\Chrome\\User Data\\*\\Code Cache"),
                Glob("LOCALAPPDATA", "Google\\Chrome\\User Data\\*\\GPUCache"),
                Glob("LOCALAPPDATA", "Google\\Chrome\\User Data\\ShaderCache"),
                Glob("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\*\\Cache"),
                Glob("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\*\\Code Cache"),
                Glob("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\*\\GPUCache"),
                Glob("LOCALAPPDATA", "Microsoft\\Edge\\User Data\\ShaderCache"),
                Glob("LOCALAPPDATA", "Mozilla\\Firefox\\Profiles\\*\\cache2"),
                Env("LOCALAPPDATA", "BraveSoftware\\Brave-Browser\\User Data\\Default\\Cache"),
                Env("APPDATA", "Opera Software\\Opera Stable\\Cache"),
                Glob("LOCALAPPDATA", "Tencent\\QQBrowser\\User Data\\Default\\Cache"),
                Glob("LOCALAPPDATA", "360Chrome\\Chrome\\User Data\\Default\\Cache"),
                Glob("LOCALAPPDATA", "360se6\\User Data\\Default\\Cache"),
                Glob("LOCALAPPDATA", "UCBrowser\\User Data\\Default\\Cache"),
                Glob("LOCALAPPDATA", "Vivaldi\\User Data\\Default\\Cache"),
                Glob("LOCALAPPDATA", "CentBrowser\\User Data\\Default\\Cache"),
            ], patterns: &["*"] },
        CatDef { id: "recycle-bin-deep", name: "回收站（全部盘符）", desc: "所有盘的 $Recycle.Bin", risk: 3, system_managed: false,
            paths: &[AllDrives("$Recycle.Bin")], patterns: &["*"] },
        CatDef { id: "windows-update", name: "Windows 更新缓存", desc: "SoftwareDistribution\\Download", risk: 2, system_managed: false,
            paths: &[Env("SYSTEMROOT", "SoftwareDistribution\\Download")], patterns: &["*"] },
        CatDef { id: "delivery-opt", name: "传递优化缓存", desc: "新旧两代服务目录均覆盖", risk: 2, system_managed: false,
            paths: &[
                Env("SYSTEMROOT", "SoftwareDistribution\\DeliveryOptimization"),
                Env("SYSTEMROOT", "ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache"),
            ], patterns: &["*"] },
        CatDef { id: "defender", name: "Defender 非关键缓存", desc: "LocalCopy/Support/扫描历史（与系统清理向导一致）", risk: 2, system_managed: false,
            paths: &[
                Env("PROGRAMDATA", "Microsoft\\Windows Defender\\LocalCopy"),
                Env("PROGRAMDATA", "Microsoft\\Windows Defender\\Support"),
                Env("PROGRAMDATA", "Microsoft\\Windows Defender\\Scans\\History\\Service"),
            ], patterns: &["*"] },
        CatDef { id: "thumbnails", name: "缩略图缓存（全用户）", desc: "thumbcache/iconcache 数据库，含其他用户", risk: 1, system_managed: false,
            paths: &[
                Env("LOCALAPPDATA", "Microsoft\\Windows\\Explorer"),
                Glob("SYSTEMDRIVE", "Users\\*\\AppData\\Local\\Microsoft\\Windows\\Explorer"),
            ], patterns: &["thumbcache_*.db", "iconcache_*.db"] },
        CatDef { id: "log-files", name: "日志文件", desc: "Windows\\Logs + 用户崩溃转储（*.log/*.etl/*.evtx）", risk: 2, system_managed: false,
            paths: &[Env("SYSTEMROOT", "Logs"), Env("LOCALAPPDATA", "CrashDumps")], patterns: &["*.log", "*.etl", "*.evtx", "*.dmp"] },
        CatDef { id: "memory-dump", name: "内存转储", desc: "Minidump + MEMORY.DMP（可能达数 GB）", risk: 3, system_managed: false,
            paths: &[Env("SYSTEMROOT", "Minidump"), Env("SYSTEMROOT", "MEMORY.DMP")], patterns: &["*.dmp", "MEMORY.DMP"] },
        CatDef { id: "windows-old", name: "旧版 Windows 安装", desc: "Windows.old / $Windows.~BT / ~WS —— 系统托管，请在「磁盘清理」中处理", risk: 3, system_managed: true,
            paths: &[DriveRoot("Windows.old"), DriveRoot("$Windows.~BT"), DriveRoot("$Windows.~WS")], patterns: &["*"] },
        CatDef { id: "app-cache", name: "应用 Web 缓存", desc: "WebView/WebCache（不泛扫 Packages，避免误删应用数据）", risk: 3, system_managed: false,
            paths: &[Env("LOCALAPPDATA", "Microsoft\\Windows\\WebCache")], patterns: &["*"] },
        CatDef { id: "third-party-cache", name: "第三方应用缓存", desc: "Discord/Slack/Teams/Steam/Epic/网易云/QQ音乐 等 Chromium 缓存", risk: 2, system_managed: false,
            paths: &[
                Glob("APPDATA", "discord\\Cache"), Glob("APPDATA", "discord\\Code Cache"),
                Glob("APPDATA", "discord\\GPUCache"), Glob("APPDATA", "discord\\ShaderCache"),
                Glob("APPDATA", "Slack\\Cache"),
                Glob("APPDATA", "Microsoft\\Teams\\Cache"), Glob("APPDATA", "Microsoft\\Teams\\GPUCache"),
                Glob("LOCALAPPDATA", "Steam\\htmlcache"),
                Glob("LOCALAPPDATA", "EpicGamesLauncher\\Saved\\webcache"),
                Glob("LOCALAPPDATA", "Netease\\CloudMusic\\Cache"),
                Glob("APPDATA", "Tencent\\QQMusic\\Cache"),
            ], patterns: &["*"] },
        CatDef { id: "font-cache", name: "字体缓存", desc: "系统字体缓存（重启后重建）", risk: 1, system_managed: false,
            paths: &[Env("SYSTEMROOT", "ServiceProfiles\\LocalService\\AppData\\Local\\FontCache")], patterns: &["*"] },
        CatDef { id: "wer-deep", name: "Windows 错误报告", desc: "用户 + 系统两处 WER 队列", risk: 2, system_managed: false,
            paths: &[Env("LOCALAPPDATA", "Microsoft\\Windows\\WER"), Env("PROGRAMDATA", "Microsoft\\Windows\\WER")], patterns: &["*"] },
        CatDef { id: "installer-temp", name: "安装程序临时文件", desc: "MSI 补丁缓存 + Downloaded Installations", risk: 2, system_managed: false,
            paths: &[Env("SYSTEMROOT", "Installer\\$PatchCache$"), Env("LOCALAPPDATA", "Downloaded Installations")], patterns: &["*"] },
        CatDef { id: "clipboard-deep", name: "剪贴板缓存", desc: "剪贴板历史（Win+V 将被清空）", risk: 1, system_managed: false,
            paths: &[Env("LOCALAPPDATA", "Microsoft\\Windows\\Clipboard")], patterns: &["*"] },
        CatDef { id: "shader-deep", name: "着色器缓存（全厂商）", desc: "D3D/NVIDIA/AMD/Intel 编译缓存", risk: 1, system_managed: false,
            paths: &[
                Env("SYSTEMROOT", "System32\\d3d_cache"),
                Env("LOCALAPPDATA", "D3DSCache"),
                Env("LOCALAPPDATA", "AMD\\DxCache"),
                Env("LOCALAPPDATA", "NVIDIA\\DXCache"),
                Env("LOCALAPPDATA", "Intel\\ShaderCache"),
            ], patterns: &["*"] },
    ]
}

/// 深度垃圾 18 类的全部展开根（批量删除允许集合，当场重扫）
pub fn all_scan_roots() -> Vec<PathBuf> {
    categories()
        .iter()
        .flat_map(|c| c.paths.iter().flat_map(expand))
        .collect()
}

// ---------- 扫描 ----------

pub fn cancel_scan() {
    CANCELLED.store(true, Ordering::Relaxed);
}

/// 分类信息（get_categories 用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub risk_level: u8,
    pub system_managed: bool,
}

/// 全部分类的静态信息（不等扫描）
pub fn category_infos() -> Vec<CategoryInfo> {
    categories()
        .into_iter()
        .map(|c| CategoryInfo {
            id: c.id.into(),
            name: c.name.into(),
            description: c.desc.into(),
            risk_level: c.risk,
            system_managed: c.system_managed,
        })
        .collect()
}

/// 扫描单个分类（内部）：collect_all=true 时保留全部文件（分页会话用）
fn scan_one(cat: &CatDef, collect_all: bool) -> (u64, usize, Vec<FileEntry>) {
    let (mut size, mut count) = (0u64, 0usize);
    let mut files: Vec<FileEntry> = Vec::new();

    for spec in cat.paths {
        for dir in expand(spec) {
            if !dir.exists() { continue; }
            for entry in walkdir::WalkDir::new(&dir).follow_links(false).into_iter().flatten() {
                if CANCELLED.load(Ordering::Relaxed) { break; }
                if !entry.file_type().is_file() { continue; }
                let name = entry.file_name().to_string_lossy();
                if !cat.patterns.iter().any(|p| glob_match(p, &name)) { continue; }
                let s = entry.metadata().map(|m| m.len()).unwrap_or(0);
                size += s;
                count += 1;
                if collect_all {
                    // 分页会话：保留全部，最后按大小降序；上限 10 万条防 IPC 失控
                    if files.len() < 100_000 {
                        files.push(FileEntry { path: entry.path().to_string_lossy().to_string(), size: s });
                    }
                } else {
                    // 常规路径：只保留 Top 10 大文件
                    if files.len() < 10 || s > files.last().map(|f| f.size).unwrap_or(0) {
                        files.push(FileEntry { path: entry.path().to_string_lossy().to_string(), size: s });
                        files.sort_by(|a, b| b.size.cmp(&a.size));
                        files.truncate(10);
                    }
                }
            }
        }
    }
    if collect_all {
        files.sort_by(|a, b| b.size.cmp(&a.size));
    }
    (size, count, files)
}

/// 深度扫描（在 spawn_blocking 线程中调用；可取消）
pub fn scan() -> DeepScanResult {
    scan_filtered(None)
}

/// 常规垃圾扫描（scan_junk_files）：可选分类名过滤
pub fn scan_filtered(category_names: Option<Vec<String>>) -> DeepScanResult {
    scan_filtered_with_progress(category_names, &|_, _| {})
}

/// 带进度回调的分类扫描（每完成一个分类回调 (分类名, 已完成数/总数)）
pub fn scan_filtered_with_progress(category_names: Option<Vec<String>>, on_progress: &dyn Fn(&str, (usize, usize))) -> DeepScanResult {
    CANCELLED.store(false, Ordering::Relaxed);
    let t0 = std::time::Instant::now();
    let mut out = Vec::new();
    let (mut total_size, mut total_files) = (0u64, 0usize);
    let cats = categories();
    let selected: Vec<&CatDef> = cats
        .iter()
        .filter(|c| category_names.as_ref().is_none_or(|names| names.iter().any(|n| n == c.id || n == c.name)))
        .collect();
    let total_cats = selected.len();

    for (idx, cat) in selected.into_iter().enumerate() {
        if CANCELLED.load(Ordering::Relaxed) {
            return DeepScanResult { categories: out, total_size, total_files, duration_ms: t0.elapsed().as_millis() as u64, cancelled: true };
        }
        on_progress(cat.name, (idx, total_cats));
        let (size, count, top) = scan_one(cat, false);

        if size > 0 {
            total_size += size;
            total_files += count;
            out.push(CategoryResult {
                id: cat.id.into(), name: cat.name.into(), description: cat.desc.into(),
                risk: cat.risk, size, file_count: count, system_managed: cat.system_managed,
                top_files: top,
            });
        }
    }
    on_progress("完成", (total_cats, total_cats));

    out.sort_by(|a, b| b.size.cmp(&a.size));
    DeepScanResult { categories: out, total_size, total_files, duration_ms: t0.elapsed().as_millis() as u64, cancelled: false }
}

// ---------- 分页会话（get_deep_junk_category_page） ----------
// 最近一次单分类完整扫描的全量文件列表，分页从这里切片

static PAGE_SESSION: std::sync::Mutex<Option<(String, Vec<FileEntry>)>> =
    std::sync::Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPage {
    pub category: String,
    pub total_count: usize,
    pub total_size: u64,
    pub offset: usize,
    pub limit: usize,
    pub files: Vec<FileEntry>,
}

/// 扫描单分类（scan_category）：返回汇总并建立分页会话
pub fn scan_category(category: &str) -> Result<CategoryResult, String> {
    let cat = categories()
        .into_iter()
        .find(|c| c.id == category || c.name == category)
        .ok_or_else(|| format!("未知分类: {}", category))?;
    CANCELLED.store(false, Ordering::Relaxed);
    let (size, count, files) = scan_one(&cat, true);
    *PAGE_SESSION.lock().unwrap() = Some((cat.id.to_string(), files.clone()));
    let top = files.into_iter().take(10).collect();
    Ok(CategoryResult {
        id: cat.id.into(), name: cat.name.into(), description: cat.desc.into(),
        risk: cat.risk, size, file_count: count, system_managed: cat.system_managed,
        top_files: top,
    })
}

/// 分页读取某分类的文件列表（会话未建立时自动重扫一次）
pub fn category_page(category: &str, offset: usize, limit: usize) -> Result<CategoryPage, String> {
    let cat = categories()
        .into_iter()
        .find(|c| c.id == category || c.name == category)
        .ok_or_else(|| format!("未知分类: {}", category))?;
    let limit = limit.clamp(1, 2000);

    let files = {
        let session = PAGE_SESSION.lock().unwrap();
        session
            .as_ref()
            .filter(|(id, _)| id == cat.id)
            .map(|(_, files)| files.clone())
    };
    let files = match files {
        Some(f) => f,
        None => {
            let _ = scan_category(cat.id)?;
            PAGE_SESSION.lock().unwrap().as_ref().map(|(_, f)| f.clone()).unwrap_or_default()
        }
    };

    let total_count = files.len();
    let total_size = files.iter().map(|f| f.size).sum();
    let page: Vec<FileEntry> = files.iter().skip(offset).take(limit).cloned().collect();
    Ok(CategoryPage {
        category: cat.id.into(),
        total_count, total_size, offset, limit,
        files: page,
    })
}

/// 删除指定类别（当场重扫 + R1 引擎逐文件过检）
pub fn delete(category_ids: &[String]) -> DeepDeleteReport {
    let _lock = crate::state::acquire_write_lock();
    let defs = categories();
    let mut freed = 0u64;
    let mut errors = Vec::new();

    for id in category_ids {
        let Some(cat) = defs.iter().find(|c| c.id == id.as_str()) else {
            errors.push(format!("未知类别: {}", id));
            continue;
        };
        if cat.system_managed {
            errors.push(format!("{} 由系统托管，请用「磁盘清理」处理", cat.name));
            continue;
        }
        // 允许根 = 该类别的全部展开路径
        let roots: Vec<PathBuf> = cat.paths.iter().flat_map(expand).collect();
        for root in &roots {
            if !root.exists() { continue; }
            for entry in walkdir::WalkDir::new(root).follow_links(false).into_iter().flatten() {
                if !entry.file_type().is_file() { continue; }
                let name = entry.file_name().to_string_lossy();
                if !cat.patterns.iter().any(|p| glob_match(p, &name)) { continue; }
                match super::engine::safe_delete_file(entry.path(), &roots) {
                    super::engine::DeleteResult::Deleted(s) => freed += s,
                    super::engine::DeleteResult::Error(m) => errors.push(m),
                    super::engine::DeleteResult::Skipped(_) => {}
                }
            }
        }
    }

    DeepDeleteReport { freed, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_match_basic() {
        assert!(glob_match("thumbcache_*.db", "thumbcache_32.db"));
        assert!(glob_match("*.log", "setup.log"));
        assert!(glob_match("*", "anything.exe"));
        assert!(!glob_match("*.log", "setup.txt"));
        assert!(glob_match("Profile *", "Profile 3"));
        assert!(!glob_match("Profile *", "Default"));
        assert!(glob_match("MEMORY.DMP", "MEMORY.DMP"));
    }

    #[test]
    fn glob_expands_profile_star() {
        // 用一个临时目录模拟 User Data
        let tmp = std::env::temp_dir().join(format!("cdisk-glob-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("Default\\Cache")).unwrap();
        std::fs::create_dir_all(tmp.join("Profile 2\\Cache")).unwrap();
        std::fs::create_dir_all(tmp.join("Guest\\Cache")).unwrap();
        let old = std::env::var("CDISK_TEST_BASE").ok();
        std::env::set_var("CDISK_TEST_BASE", &tmp);
        let out = expand(&PathSpec::Glob("CDISK_TEST_BASE", "*\\Cache"));
        assert_eq!(out.len(), 3, "应展开 3 个 Profile 的 Cache");
        if let Some(v) = old { std::env::set_var("CDISK_TEST_BASE", v); } else { std::env::remove_var("CDISK_TEST_BASE"); }
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
