//! 社交软件缓存扫描：移植自 light-c social_scanner
//!
//! 风险四级分类（核心安全资产）：
//!   CRITICAL 聊天记录数据库（Msg/Database/nt_msg/db_storage + .db/.sqlite）→ 永不提供删除
//!   MEDIUM   接收文件（FileStorage/File/FileRecv/MsgAttach）→ 显式选择才删
//!   LOW      加密图片视频（Image/Video/Pic/Ptt + .dat）→ 建议清理
//!   NONE     临时缓存日志（Cache/Temp/Log/WebView/Thumb/sdk_storage）→ 安全清理
//!
//! 数据目录定位：注册表（微信 3.x/4.x 两代键）+ 文档目录回退 + 常见安装路径

use serde::Serialize;
use std::path::{Path, PathBuf};
use winreg::enums::*;
use winreg::RegKey;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialCategory {
    pub category: String,  // database / files / media / cache
    pub risk: String,      // critical / medium / low / none
    pub size: u64,
    pub file_count: usize,
    pub deletable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialApp {
    pub app: String,       // 微信 / QQ / 钉钉 / 飞书 / 企业微信
    pub data_dirs: Vec<String>,
    pub categories: Vec<SocialCategory>,
    pub total_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialScan {
    pub apps: Vec<SocialApp>,
    pub total_size: u64,
    pub deletable_size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SocialDeleteReport {
    pub freed: u64,
    pub errors: Vec<String>,
}

// ---------- 风险分类引擎 ----------

#[derive(Debug, PartialEq, Clone, Copy)]
enum Risk { Critical, Medium, Low, None }

const DB_DIRS: &[&str] = &["msg\\database", "msg\\multi", "nt_msg", "nt_db", "db_storage", "tdata", "database"];
const FILE_DIRS: &[&str] = &["filestorage\\file", "filerecv", "msgattach", "file_storage", "file_storage\\file"];
const MEDIA_DIRS: &[&str] = &["image", "video", "pic", "ptt", "shortvideo"];
const CACHE_DIRS: &[&str] = &["cache", "temp", "log", "webview", "thumb", "sdk_storage", "cdn", "avatar"];

fn classify(path: &Path) -> (Risk, &'static str) {
    let lower = path.to_string_lossy().to_lowercase().replace('/', "\\");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();

    // 数据库：目录特征或后缀特征任一命中即 CRITICAL
    let in_db_dir = DB_DIRS.iter().any(|d| lower.contains(d));
    let is_db_ext = matches!(ext.as_str(), "db" | "db-wal" | "db-shm" | "sqlite" | "sqlite-wal" | "sqlite-shm" | "ldb");
    if in_db_dir || is_db_ext {
        return (Risk::Critical, "database");
    }
    if FILE_DIRS.iter().any(|d| lower.contains(d)) {
        return (Risk::Medium, "files");
    }
    if MEDIA_DIRS.iter().any(|d| lower.contains(d)) || ext == "dat" {
        return (Risk::Low, "media");
    }
    (Risk::None, "cache")
}

fn risk_name(r: Risk) -> &'static str {
    match r { Risk::Critical => "critical", Risk::Medium => "medium", Risk::Low => "low", Risk::None => "none" }
}

// ---------- 数据目录定位 ----------

/// 微信数据目录（注册表 3.x/4.x 两代 + 文档目录回退）
fn locate_wechat() -> Vec<PathBuf> {
    let mut out = Vec::new();
    // 3.x: HKCU\Software\Tencent\WeChat -> FileSavePath
    // 4.x: HKCU\Software\Tencent\Weixin -> FileSavePath（或 InstallPath 同级）
    for key_path in &[r"Software\Tencent\WeChat", r"Software\Tencent\Weixin"] {
        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(key_path, KEY_READ) {
            for value in &["FileSavePath", "InstallPath"] {
                if let Ok(p) = key.get_value::<String, _>(value) {
                    let pb = PathBuf::from(p);
                    if pb.exists() { out.push(pb); }
                }
            }
        }
    }
    // 回退：文档目录两代结构
    if let Some(doc) = dirs_documents() {
        for name in &["WeChat Files", "xwechat_files", "wxwork_files"] {
            let p = doc.join(name);
            if p.exists() { out.push(p); }
        }
    }
    dedup_paths(out)
}

/// QQ/NTQQ 数据目录
fn locate_qq() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(doc) = dirs_documents() {
        for name in &["Tencent Files", "nt_qq"] {
            let p = doc.join(name);
            if p.exists() { out.push(p); }
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        for name in &["Tencent\\QQ", "Tencent\\nt_qq"] {
            let p = PathBuf::from(&appdata).join(name);
            if p.exists() { out.push(p); }
        }
    }
    dedup_paths(out)
}

fn locate_dingtalk() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(appdata) = std::env::var_os("APPDATA") {
        for name in &["DingTalk\\storage", "DingTalk\\cache", "DingTalk"] {
            let p = PathBuf::from(&appdata).join(name);
            if p.exists() { out.push(p); }
        }
    }
    dedup_paths(out)
}

fn locate_feishu() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for var in &["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(var) {
            for name in &["Feishu", "LarkShell", "Lark"] {
                let p = PathBuf::from(&base).join(name);
                if p.exists() { out.push(p); }
            }
        }
    }
    dedup_paths(out)
}

fn locate_wxwork() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(doc) = dirs_documents() {
        let p = doc.join("WXWork");
        if p.exists() { out.push(p); }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let p = PathBuf::from(&appdata).join("WXWork");
        if p.exists() { out.push(p); }
    }
    dedup_paths(out)
}

fn dirs_documents() -> Option<PathBuf> {
    // 文档目录可能重定向（KFM），用注册表 Personal 项比 USERPROFILE\Documents 可靠
    if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Shell Folders", KEY_READ)
    {
        if let Ok(p) = key.get_value::<String, _>("Personal") {
            return Some(PathBuf::from(p));
        }
    }
    std::env::var_os("USERPROFILE").map(PathBuf::from).map(|p| p.join("Documents"))
}

fn dedup_paths(v: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    v.into_iter().filter(|p| seen.insert(p.to_string_lossy().to_lowercase())).collect()
}

// ---------- 扫描 ----------

pub fn scan() -> SocialScan {
    let apps_def: Vec<(&str, fn() -> Vec<PathBuf>)> = vec![
        ("微信", locate_wechat),
        ("QQ", locate_qq),
        ("钉钉", locate_dingtalk),
        ("飞书", locate_feishu),
        ("企业微信", locate_wxwork),
    ];

    let mut apps = Vec::new();
    let mut total_all = 0u64;
    let mut deletable_all = 0u64;

    for (app_name, locate) in apps_def {
        let dirs = locate();
        if dirs.is_empty() { continue; }

        // 按风险级聚合
        let mut agg: std::collections::HashMap<&'static str, (u64, usize, Risk)> = std::collections::HashMap::new();
        for dir in &dirs {
            for entry in walkdir::WalkDir::new(dir).follow_links(false).into_iter().flatten() {
                if !entry.file_type().is_file() { continue; }
                let (risk, cat) = classify(entry.path());
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let e = agg.entry(cat).or_insert((0, 0, risk));
                e.0 += size;
                e.1 += 1;
            }
        }

        let mut categories: Vec<SocialCategory> = agg.into_iter().map(|(cat, (size, count, risk))| {
            SocialCategory {
                category: cat.to_string(),
                risk: risk_name(risk).to_string(),
                size,
                file_count: count,
                deletable: risk != Risk::Critical,
            }
        }).collect();
        categories.sort_by(|a, b| b.size.cmp(&a.size));

        let total: u64 = categories.iter().map(|c| c.size).sum();
        if total == 0 { continue; }
        total_all += total;
        deletable_all += categories.iter().filter(|c| c.deletable).map(|c| c.size).sum::<u64>();
        apps.push(SocialApp {
            app: app_name.to_string(),
            data_dirs: dirs.iter().map(|d| d.to_string_lossy().to_string()).collect(),
            categories,
            total_size: total,
        });
    }

    apps.sort_by(|a, b| b.total_size.cmp(&a.total_size));
    SocialScan { apps, total_size: total_all, deletable_size: deletable_all }
}

/// 删除指定应用的指定类别（CRITICAL 硬拒绝）
pub fn delete(app_name: &str, categories: &[String]) -> SocialDeleteReport {
    let _lock = crate::state::acquire_write_lock();
    let locate: fn() -> Vec<PathBuf> = match app_name {
        "微信" => locate_wechat,
        "QQ" => locate_qq,
        "钉钉" => locate_dingtalk,
        "飞书" => locate_feishu,
        "企业微信" => locate_wxwork,
        _ => return SocialDeleteReport { freed: 0, errors: vec![format!("未知应用: {}", app_name)] },
    };

    let dirs = locate();
    let mut freed = 0u64;
    let mut errors = Vec::new();

    for dir in &dirs {
        for entry in walkdir::WalkDir::new(dir).follow_links(false).into_iter().flatten() {
            if !entry.file_type().is_file() { continue; }
            let (risk, cat) = classify(entry.path());
            // 硬护栏：CRITICAL 永不删除（即使前端传错也不删）
            if risk == Risk::Critical { continue; }
            if !categories.contains(&cat.to_string()) { continue; }
            match super::engine::safe_delete_file(entry.path(), &dirs) {
                super::engine::DeleteResult::Deleted(s) => freed += s,
                super::engine::DeleteResult::Error(m) => errors.push(m),
                super::engine::DeleteResult::Skipped(_) => {}
            }
        }
    }

    SocialDeleteReport { freed, errors }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_chat_database_critical() {
        let (r, _) = classify(Path::new(r"D:\xwechat_files\wxid_123\db_storage\msg\MSG0.db"));
        assert_eq!(r, Risk::Critical);
        let (r2, _) = classify(Path::new(r"D:\QQ\nt_data\nt_msg\nt_msg.db-wal"));
        assert_eq!(r2, Risk::Critical);
        // 任何路径下的 .db 后缀都算数据库
        let (r3, _) = classify(Path::new(r"D:\random\file.db"));
        assert_eq!(r3, Risk::Critical);
    }

    #[test]
    fn classify_media_and_cache() {
        let (r, c) = classify(Path::new(r"D:\WeChat Files\wxid\FileStorage\Image\2024-01\abc.dat"));
        assert_eq!(r, Risk::Low);
        assert_eq!(c, "media");
        let (r2, _) = classify(Path::new(r"D:\app\Cache\gpu\123.bin"));
        assert_eq!(r2, Risk::None);
    }

    #[test]
    fn classify_received_files_medium() {
        let (r, c) = classify(Path::new(r"D:\WeChat Files\wxid\FileStorage\File\2024-01\report.docx"));
        assert_eq!(r, Risk::Medium);
        assert_eq!(c, "files");
    }
}
