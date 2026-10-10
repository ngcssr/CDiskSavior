//! 应用数据目录解析（与 Tauri app_data_dir 对齐）+ 自定义目录 + 发行渠道检测
//!
//! Tauri 的 app_data_dir() 需要 AppHandle；无句柄的后端模块（注册表备份、日志等）
//! 通过本模块解析同一目录，避免层层传递句柄。
//! Windows 上 app_data_dir = %APPDATA%\<identifier>（identifier 见 tauri.conf.json）。
//!
//! 自定义数据目录：配置文件固定存于默认目录 `config\data-dir.txt`，
//! data_dir() 解析顺序：缓存 → 自定义配置 → 默认。light-c data_dir.rs 的简化版
//! （不实现便携版相对布局，便携判定走 manifest 标记）。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Bundle identifier（tauri.conf.json），勿硬编码盘符
const IDENTIFIER: &str = "com.cdisksavior.app";
/// 便携版 manifest 标记文件（位于 exe 同目录即判便携）
const PORTABLE_MANIFEST_FILE: &str = "cdisksavior-portable.json";

static DATA_DIR_CACHE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// 默认数据目录：%APPDATA%\com.cdisksavior.app
pub fn default_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(IDENTIFIER)
}

/// 自定义目录配置文件（永远位于默认目录，避免自身依赖 data_dir）
fn data_dir_config_file() -> PathBuf {
    default_dir().join("config").join("data-dir.txt")
}

fn read_configured_dir() -> Option<PathBuf> {
    let content = std::fs::read_to_string(data_dir_config_file()).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let p = PathBuf::from(trimmed);
    if p.is_absolute() { Some(p) } else { None }
}

/// 当前数据目录（缓存 → 自定义 → 默认）
pub fn data_dir() -> PathBuf {
    if let Some(cached) = DATA_DIR_CACHE.read().unwrap().as_ref() {
        return cached.clone();
    }
    let resolved = read_configured_dir().unwrap_or_else(default_dir);
    *DATA_DIR_CACHE.write().unwrap() = Some(resolved.clone());
    resolved
}

/// 发行渠道
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DistributionChannel {
    /// NSIS 安装版（perMachine）
    Installer,
    /// 便携版（exe 同目录含 manifest 标记）
    Portable,
}

/// 检测发行渠道：exe 同目录存在便携 manifest 即判便携，否则安装版
pub fn detect_distribution_channel(exe_path: &Path) -> DistributionChannel {
    if let Some(dir) = exe_path.parent() {
        if dir.join(PORTABLE_MANIFEST_FILE).is_file() {
            return DistributionChannel::Portable;
        }
    }
    DistributionChannel::Installer
}

pub fn current_channel() -> DistributionChannel {
    std::env::current_exe()
        .map(|exe| detect_distribution_channel(&exe))
        .unwrap_or(DistributionChannel::Installer)
}

/// 设置自定义数据目录并迁移现有数据
///
/// 校验：绝对路径 + 可创建 + 可写；迁移：复制现有数据目录内容（同名保留旧值），
/// 写配置，更新缓存。源数据保留（不删除），用户可手动清理。
pub fn set_data_dir(path: &Path) -> Result<String, String> {
    if !path.is_absolute() {
        return Err("数据目录必须是绝对路径".into());
    }
    std::fs::create_dir_all(path).map_err(|e| format!("无法创建数据目录 {}: {}", path.display(), e))?;
    // 可写性实测（不是仅检查存在）
    let probe = path.join(".cdisksavior-write-probe");
    std::fs::write(&probe, b"probe").map_err(|e| format!("数据目录不可写 {}: {}", path.display(), e))?;
    let _ = std::fs::remove_file(&probe);

    let old = data_dir();
    let mut migrated = 0usize;
    if old != path && old.is_dir() {
        for entry in walkdir::WalkDir::new(&old).follow_links(false).into_iter().flatten() {
            let rel = match entry.path().strip_prefix(&old) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rel.as_os_str().is_empty() { continue; }
            let dest = path.join(rel);
            if entry.file_type().is_dir() {
                let _ = std::fs::create_dir_all(&dest);
            } else if entry.file_type().is_file() && !dest.exists() {
                if let Some(parent) = dest.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::copy(entry.path(), &dest).is_ok() {
                    migrated += 1;
                }
            }
        }
    }

    let config = data_dir_config_file();
    if let Some(parent) = config.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&config, path.to_string_lossy().as_bytes())
        .map_err(|e| format!("写入数据目录配置失败: {}", e))?;
    *DATA_DIR_CACHE.write().unwrap() = Some(path.to_path_buf());

    Ok(format!(
        "数据目录已更改为: {}（迁移 {} 个文件，源数据保留）",
        path.display(),
        migrated
    ))
}

// ============ 存储位置信息 / 写入诊断 ============

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageLocationInfo {
    pub channel: DistributionChannel,
    pub default_dir: String,
    pub data_dir: String,
    pub is_custom_dir: bool,
    pub config_file: String,
}

pub fn storage_location_info() -> StorageLocationInfo {
    let default = default_dir();
    let current = data_dir();
    StorageLocationInfo {
        channel: current_channel(),
        default_dir: default.to_string_lossy().to_string(),
        data_dir: current.to_string_lossy().to_string(),
        is_custom_dir: current != default,
        config_file: data_dir_config_file().to_string_lossy().to_string(),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageWriteDiagnostic {
    pub data_dir_writable: bool,
    pub config_dir_writable: bool,
    pub errors: Vec<String>,
}

pub fn storage_write_diagnostic() -> StorageWriteDiagnostic {
    let mut errors = Vec::new();
    let data_ok = probe_writable(&data_dir(), &mut errors);
    let config_ok = probe_writable(&default_dir(), &mut errors);
    StorageWriteDiagnostic {
        data_dir_writable: data_ok,
        config_dir_writable: config_ok,
        errors,
    }
}

fn probe_writable(dir: &Path, errors: &mut Vec<String>) -> bool {
    if let Err(e) = std::fs::create_dir_all(dir) {
        errors.push(format!("创建目录失败 {}: {}", dir.display(), e));
        return false;
    }
    let probe = dir.join(".cdisksavior-write-probe");
    match std::fs::write(&probe, b"probe") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(e) => {
            errors.push(format!("写入测试失败 {}: {}", dir.display(), e));
            false
        }
    }
}

// ============ 本地数据清理（clear_local_data 系） ============

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearableDataItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size: u64,
    pub file_count: usize,
}

/// 可清理项清单（relocate/config.json 迁移配置与 data-dir.txt 永不入列）
pub fn list_clearable_items() -> Vec<ClearableDataItem> {
    let dir = data_dir();
    let mut items = Vec::new();

    // 文件型：扫描缓存 + 增长基线 + 清理统计
    let mut scan_cache_size = 0u64;
    let mut scan_cache_count = 0usize;
    let mut snapshot_size = 0u64;
    let mut snapshot_count = 0usize;
    if let Ok(read) = std::fs::read_dir(&dir) {
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("scan-cache-") && name.ends_with(".json") {
                scan_cache_size += e.metadata().map(|m| m.len()).unwrap_or(0);
                scan_cache_count += 1;
            } else if name.starts_with("snapshot-") && name.ends_with(".json") {
                snapshot_size += e.metadata().map(|m| m.len()).unwrap_or(0);
                snapshot_count += 1;
            }
        }
    }
    if scan_cache_count > 0 {
        items.push(ClearableDataItem {
            id: "scan-cache".into(),
            name: "扫描缓存".into(),
            description: format!("scan-cache-*.json × {}", scan_cache_count),
            size: scan_cache_size,
            file_count: scan_cache_count,
        });
    }
    if snapshot_count > 0 {
        items.push(ClearableDataItem {
            id: "growth-snapshot".into(),
            name: "增长基线快照".into(),
            description: format!("snapshot-*.json × {}（清除后需重建基线）", snapshot_count),
            size: snapshot_size,
            file_count: snapshot_count,
        });
    }
    let stats_file = dir.join("cleanup-stats.json");
    if stats_file.is_file() {
        items.push(ClearableDataItem {
            id: "cleanup-stats".into(),
            name: "清理统计台账".into(),
            description: "cleanup-stats.json（累计释放统计将归零）".into(),
            size: std::fs::metadata(&stats_file).map(|m| m.len()).unwrap_or(0),
            file_count: 1,
        });
    }

    // 目录型：日志 / 注册表备份 / 驱动备份
    for (id, name, desc, sub) in [
        ("logs", "操作日志", "logs/（shell_icons、cleanup-actions 等）", "logs"),
        ("reg-backups", "注册表备份", "reg_backups/（清除后已删注册表项不可恢复）", "reg_backups"),
        ("driver-backups", "驱动备份", "driver_backups/（清除后已删驱动不可恢复）", "driver_backups"),
    ] {
        let sub_dir = dir.join(sub);
        if sub_dir.is_dir() {
            let (size, count) = dir_stats(&sub_dir);
            if count > 0 {
                items.push(ClearableDataItem {
                    id: id.into(),
                    name: name.into(),
                    description: desc.into(),
                    size,
                    file_count: count,
                });
            }
        }
    }
    items
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearLocalDataResult {
    pub cleared_items: usize,
    pub freed: u64,
    pub errors: Vec<String>,
}

/// 按 id 清理指定项；空 ids = 全部清理
pub fn clear_local_data_items(ids: &[String]) -> ClearLocalDataResult {
    let dir = data_dir();
    let items = list_clearable_items();
    let targets: Vec<&ClearableDataItem> = if ids.is_empty() {
        items.iter().collect()
    } else {
        items.iter().filter(|i| ids.contains(&i.id)).collect()
    };

    let mut result = ClearLocalDataResult { cleared_items: 0, freed: 0, errors: vec![] };
    for item in targets {
        result.freed += item.size;
        let remove_result = match item.id.as_str() {
            "scan-cache" | "growth-snapshot" => {
                let prefix = if item.id == "scan-cache" { "scan-cache-" } else { "snapshot-" };
                remove_matching_files(&dir, prefix)
            }
            "cleanup-stats" => std::fs::remove_file(dir.join("cleanup-stats.json"))
                .map_err(|e| e.to_string()),
            "logs" => std::fs::remove_dir_all(dir.join("logs")).map_err(|e| e.to_string()),
            "reg-backups" => std::fs::remove_dir_all(dir.join("reg_backups")).map_err(|e| e.to_string()),
            "driver-backups" => std::fs::remove_dir_all(dir.join("driver_backups")).map_err(|e| e.to_string()),
            _ => Err(format!("未知清理项: {}", item.id)),
        };
        match remove_result {
            Ok(()) => result.cleared_items += 1,
            Err(e) => result.errors.push(format!("{}: {}", item.name, e)),
        }
    }
    result
}

fn remove_matching_files(dir: &Path, prefix: &str) -> Result<(), String> {
    if let Ok(read) = std::fs::read_dir(dir) {
        for e in read.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(".json") {
                std::fs::remove_file(e.path()).map_err(|err| format!("删除 {} 失败: {}", name, err))?;
            }
        }
    }
    Ok(())
}

fn dir_stats(path: &Path) -> (u64, usize) {
    walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .fold((0u64, 0usize), |(s, c), e| {
            (s + e.metadata().map(|m| m.len()).unwrap_or(0), c + 1)
        })
}

#[cfg(test)]
mod tests {
    use super::{data_dir, detect_distribution_channel, DistributionChannel};

    #[test]
    fn data_dir_uses_identifier_suffix() {
        assert!(data_dir().ends_with("com.cdisksavior.app"));
    }

    #[test]
    fn channel_defaults_to_installer_without_manifest() {
        // 当前 exe 目录无便携 manifest → 安装版
        let exe = std::env::current_exe().unwrap();
        assert_eq!(detect_distribution_channel(&exe), DistributionChannel::Installer);
    }
}
