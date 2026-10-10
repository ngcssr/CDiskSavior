//! 卸载残留扫描：移植自 light-c leftovers（置信度评分引擎）
//!
//! 加权评分模型（替代布尔判断）：
//!   注册表在装映射 → 每目录加权评分（0~1）→ 阈值分级
//!   ≥0.75 高置信度（默认勾选）/ 0.40~0.75 可疑（仅展示）/ <0.40 丢弃
//!
//! 白名单结构化（Exact/Prefix），持久化 JSON（R8 原子写）。

use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

// ---------- 数据类型 ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub score: f64,
    pub reasons: Vec<String>,
    pub last_modified_days: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverScan {
    pub high: Vec<LeftoverEntry>,
    pub suspicious: Vec<LeftoverEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverSafety {
    pub path: String,
    pub score: f64,
    pub verdict: String, // high / suspicious / protected / not_leftover
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeftoverDeleteReport {
    pub freed: u64,
    pub errors: Vec<String>,
    pub skipped: Vec<String>,
}

// ---------- 注册表在装应用映射 ----------

struct InstalledMap {
    /// 规范化 DisplayName 集合
    display_names: HashSet<String>,
    /// InstallLocation 末级目录名集合（规范化）
    install_leaf_names: HashSet<String>,
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn load_installed_map() -> InstalledMap {
    let mut display_names = HashSet::new();
    let mut install_leaf_names = HashSet::new();

    let keys: [(winreg::HKEY, &str); 3] = [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];

    for (hive, path) in keys {
        let Ok(root) = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ) else { continue };
        for sub_name in root.enum_keys().flatten() {
            let Ok(sub) = root.open_subkey_with_flags(&sub_name, KEY_READ) else { continue };
            if let Ok(dn) = sub.get_value::<String, _>("DisplayName") {
                let n = normalize(&dn);
                if !n.is_empty() { display_names.insert(n); }
            }
            if let Ok(loc) = sub.get_value::<String, _>("InstallLocation") {
                let leaf = Path::new(loc.trim_matches('"'))
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                let n = normalize(&leaf);
                if !n.is_empty() { install_leaf_names.insert(n); }
            }
        }
    }

    InstalledMap { display_names, install_leaf_names }
}

// ---------- 白名单（结构化，持久化） ----------

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WhitelistData {
    entries: Vec<String>,
}

fn whitelist_file() -> Option<PathBuf> {
    let dir = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?.join("CDiskSavior");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("leftover-whitelist.json"))
}

pub fn list_whitelist() -> Vec<String> {
    whitelist_file()
        .and_then(|f| crate::state::read_json_self_heal::<WhitelistData>(&f).ok().flatten())
        .map(|(d, _)| d.entries)
        .unwrap_or_default()
}

pub fn add_whitelist(entry: &str) -> Result<(), String> {
    let mut entries = list_whitelist();
    let e = entry.trim().to_lowercase();
    if e.is_empty() { return Err("白名单条目不能为空".into()); }
    if !entries.contains(&e) {
        entries.push(e);
        save_whitelist(&entries)
    } else {
        Ok(())
    }
}

pub fn remove_whitelist(entry: &str) -> Result<(), String> {
    let mut entries = list_whitelist();
    let e = entry.trim().to_lowercase();
    entries.retain(|x| x != &e);
    save_whitelist(&entries)
}

fn save_whitelist(entries: &[String]) -> Result<(), String> {
    let f = whitelist_file().ok_or("无法定位白名单存储目录")?;
    let data = WhitelistData { entries: entries.to_vec() };
    crate::state::atomic_write_json(&f, &data).map_err(|e| e.to_string())
}

/// 白名单命中：Exact / Prefix（"foo" 命中 "foo" 与 "foo bar" 前缀，不做全局 contains）
fn whitelisted(name_norm: &str, wl: &[String]) -> bool {
    wl.iter().any(|w| name_norm == w || name_norm.starts_with(w))
}

// ---------- 共享厂商目录（永不判残留） ----------

fn is_shared_vendor(norm: &str) -> bool {
    const VENDORS: &[&str] = &[
        "microsoft", "google", "tencent", "kingsoft", "mozilla", "adobe",
        "apple", "intel", "nvidia", "amd", "realtek", "commonfiles",
        "windows", "dotnet", "oracle", "java", "python", "nodejs",
    ];
    VENDORS.iter().any(|v| norm == *v)
}

/// 通用目录名（非应用专属，降权）
fn is_generic_name(norm: &str) -> bool {
    const GENERIC: &[&str] = &[
        "cache", "caches", "logs", "log", "temp", "tmp", "data", "config",
        "settings", "backup", "backups", "download", "downloads", "update", "updates",
    ];
    GENERIC.iter().any(|g| norm == *g)
}

/// 包名格式（com.xxx.yyy）
fn looks_like_package(name: &str) -> bool {
    let dots = name.matches('.').count();
    dots >= 2 && name.chars().all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// 纯版本号（1.2.3.4 / v2.0）
fn looks_like_version(name: &str) -> bool {
    let s = name.strip_prefix('v').unwrap_or(name);
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.') && s.chars().any(|c| c.is_ascii_digit())
}

// ---------- 评分引擎 ----------

fn scan_roots() -> Vec<(PathBuf, bool)> {
    // (根目录, 是否 ProgramData)
    let mut roots = Vec::new();
    if let Some(p) = std::env::var_os("LOCALAPPDATA") { roots.push((PathBuf::from(p), false)); }
    if let Some(p) = std::env::var_os("APPDATA") { roots.push((PathBuf::from(p), false)); }
    if let Some(u) = std::env::var_os("USERPROFILE") {
        roots.push((PathBuf::from(u).join("AppData").join("LocalLow"), false));
    }
    if let Some(p) = std::env::var_os("PROGRAMDATA") { roots.push((PathBuf::from(p), true)); }
    roots
}

/// 目录文件探测（有限深度：.exe/.dll/uninstall*.exe）
fn probe(dir: &Path, max_files: usize) -> (bool, bool) {
    // (has_exe_or_dll, has_uninstaller)
    let mut exe = false;
    let mut uninst = false;
    let mut seen = 0usize;
    for entry in walkdir::WalkDir::new(dir).max_depth(2).follow_links(false).into_iter().flatten() {
        if !entry.file_type().is_file() { continue; }
        seen += 1;
        if seen > max_files { break; }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if name.ends_with(".exe") || name.ends_with(".dll") { exe = true; }
        if name.starts_with("uninstall") || name.starts_with("uninst") { uninst = true; }
        if exe && uninst { break; }
    }
    (exe, uninst)
}

fn score_dir(dir: &Path, name: &str, map: &InstalledMap, is_programdata: bool, days_old: u64) -> (f64, Vec<String>) {
    let mut score = 0.0f64;
    let mut reasons = Vec::new();
    let norm = normalize(name);

    // 正向
    let (has_exe, has_uninst) = probe(dir, 300);
    if has_uninst {
        score += 0.35;
        reasons.push("含 uninstall*.exe 卸载器".into());
    }
    if map.install_leaf_names.contains(&norm) && !map.display_names.contains(&norm) {
        score += 0.25;
        reasons.push("匹配安装目录末级名但应用已不在注册表".into());
    }
    if has_exe {
        score += 0.20;
        reasons.push("含可执行文件（疑似程序本体残留）".into());
    }
    if days_old > 30 {
        score += 0.10;
        reasons.push(format!("{} 天未修改", days_old));
    }

    // 负向（保护性降权）
    if map.display_names.contains(&norm) {
        score -= 0.45;
        reasons.push("精确匹配在装应用名".into());
    }
    if map.install_leaf_names.contains(&norm) && map.display_names.contains(&norm) {
        score -= 0.60;
        reasons.push("匹配在装应用安装目录".into());
    }
    if is_generic_name(&norm) {
        score -= 0.40;
        reasons.push("通用目录名".into());
    }
    if is_programdata {
        score -= 0.30;
        reasons.push("位于 ProgramData".into());
    }
    if days_old < 7 {
        score -= 0.20;
        reasons.push("7 天内有修改".into());
    }
    if looks_like_package(name) {
        score -= 0.15;
        reasons.push("包名格式目录".into());
    }
    if looks_like_version(name) {
        score -= 0.15;
        reasons.push("纯版本号目录".into());
    }
    if is_shared_vendor(&norm) {
        score -= 0.50;
        reasons.push("共享厂商目录".into());
    }

    (score, reasons)
}

// ---------- 扫描 ----------

const MIN_SIZE: u64 = 1024 * 1024; // 忽略 <1MB

pub fn scan() -> LeftoverScan {
    let map = load_installed_map();
    let wl = list_whitelist();
    let mut high = Vec::new();
    let mut suspicious = Vec::new();

    for (root, is_pd) in scan_roots() {
        let Ok(read) = std::fs::read_dir(&root) else { continue };
        for entry in read.flatten() {
            let dir = entry.path();
            if !dir.is_dir() { continue; }
            // C1 互斥：reparse point 前置过滤（junction 不当残留）
            if super::engine::is_reparse_point(&dir) { continue; }

            let name = entry.file_name().to_string_lossy().to_string();
            let norm = normalize(&name);
            if norm.is_empty() || is_shared_vendor(&norm) { continue; }
            if whitelisted(&norm, &wl) { continue; }

            // 大小
            let size = dir_size(&dir);
            if size < MIN_SIZE { continue; }

            // 修改时间
            let days_old = entry.metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .map(|d| d.as_secs() / 86400)
                .unwrap_or(0);

            let (score, reasons) = score_dir(&dir, &name, &map, is_pd, days_old);
            let e = LeftoverEntry {
                path: dir.to_string_lossy().to_string(),
                name, size, score, reasons, last_modified_days: days_old,
            };
            if score >= 0.75 {
                high.push(e);
            } else if score >= 0.40 {
                suspicious.push(e);
            }
        }
    }

    high.sort_by(|a, b| b.size.cmp(&a.size));
    suspicious.sort_by(|a, b| b.size.cmp(&a.size));
    LeftoverScan { high, suspicious }
}

/// 单路径安全性检查
pub fn check_safety(path: &str) -> LeftoverSafety {
    let p = Path::new(path);
    let name = p.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
    let norm = normalize(&name);
    let wl = list_whitelist();

    if whitelisted(&norm, &wl) || is_shared_vendor(&norm) {
        return LeftoverSafety { path: path.into(), score: 0.0, verdict: "protected".into(), reasons: vec!["白名单或共享厂商目录".into()] };
    }
    let map = load_installed_map();
    let is_pd = path.to_lowercase().contains("programdata");
    let days_old = std::fs::metadata(p).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() / 86400)
        .unwrap_or(0);
    let (score, reasons) = score_dir(p, &name, &map, is_pd, days_old);
    let verdict = if score >= 0.75 { "high" } else if score >= 0.40 { "suspicious" } else { "not_leftover" };
    LeftoverSafety { path: path.into(), score, verdict: verdict.into(), reasons }
}

/// 删除残留（当场重扫 + 白名单 + R1 引擎）
pub fn delete(paths: &[String]) -> LeftoverDeleteReport {
    let _lock = crate::state::acquire_write_lock();
    // 当场重扫得到允许集合（高置信度 + 可疑均可由用户显式选择删除）
    let scan = scan();
    let mut allowed: HashMap<String, ()> = HashMap::new();
    for e in scan.high.iter().chain(scan.suspicious.iter()) {
        allowed.insert(e.path.to_lowercase(), ());
    }
    let roots: Vec<PathBuf> = scan_roots().into_iter().map(|(p, _)| p).collect();

    let mut freed = 0u64;
    let mut errors = Vec::new();
    let mut skipped = Vec::new();

    for p in paths {
        if !allowed.contains_key(&p.to_lowercase()) {
            skipped.push(format!("不在可删除名单（可能已不存在或评分变化）: {}", p));
            continue;
        }
        match super::engine::safe_delete_dir(Path::new(p), &roots) {
            super::engine::DeleteResult::Deleted(s) => freed += s,
            super::engine::DeleteResult::Skipped(m) => skipped.push(m),
            super::engine::DeleteResult::Error(m) => errors.push(m),
        }
    }

    LeftoverDeleteReport { freed, errors, skipped }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_non_alnum() {
        assert_eq!(normalize("WPS Office 2019"), "wpsoffice2019");
        assert_eq!(normalize("Steam"), "steam");
    }

    #[test]
    fn name_classifiers() {
        assert!(looks_like_package("com.tencent.mobileqq"));
        assert!(!looks_like_package("Steam"));
        assert!(looks_like_version("1.2.3.4"));
        assert!(looks_like_version("v2.0"));
        assert!(!looks_like_version("Steam"));
        assert!(is_generic_name("cache"));
        assert!(!is_generic_name("obsidian"));
    }

    #[test]
    fn whitelist_prefix_match() {
        let wl = vec!["adobe".to_string()];
        assert!(whitelisted("adobe", &wl));
        assert!(whitelisted("adobecreativecloud", &wl));
        assert!(!whitelisted("myadobe", &wl), "不做全局 contains");
    }
}
