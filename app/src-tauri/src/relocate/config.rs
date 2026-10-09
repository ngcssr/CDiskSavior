//! R3 legacy 导入 + 配置管理
//!
//! 设计文档 §6 C8：五级探测链 + 三条铁律
//! - config.json 与 history.json 可能不一致 → 以 history 为准
//! - target_root 新默认运行时解析（不继承旧值）
//! - scan_sources 原样导入

use crate::relocate::{scanner, validate};
use tauri::Manager;
use crate::state;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 合并后的迁移配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateConfig {
    pub version: u32,
    pub target_root: Option<String>,
    pub scan_sources: Vec<scanner::ScanSource>,
    /// 从 legacy 导入的路径（用于 UI 提示"检测到旧迁移数据"）
    pub legacy_imported: bool,
}

impl Default for RelocateConfig {
    fn default() -> Self {
        Self {
            version: 1,
            target_root: None, // 首次使用时运行时解析
            scan_sources: scanner::default_scan_sources(),
            legacy_imported: false,
        }
    }
}

/// 配置文件位置（Tauri appDataDir）
pub fn config_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("relocate-config.json"))
}

/// 读取配置（含损坏自愈）
pub fn load_config(app: &tauri::AppHandle) -> RelocateConfig {
    if let Some(file) = config_file(app) {
        match state::read_json_self_heal::<RelocateConfig>(&file) {
            Ok(Some((cfg, recovered))) => {
                if recovered {
                    log::warn!("迁移配置从 .bak 恢复");
                }
                cfg
            }
            _ => {
                // 首次使用或损坏 → 默认配置
                // 尝试 legacy 导入
                let (cfg, imported) = try_legacy_import();
                if imported {
                    // 保存导入的配置
                    let _ = save_config(app, &cfg);
                }
                cfg
            }
        }
    } else {
        RelocateConfig::default()
    }
}

/// 保存配置（原子写入 + R9 校验）
pub fn save_config(app: &tauri::AppHandle, cfg: &RelocateConfig) -> Result<(), String> {
    // R9：target_root 保存前校验
    if let Some(root) = &cfg.target_root {
        let root_path = std::path::Path::new(root);
        match validate::validate_target_root(root_path) {
            validate::TargetRootCheck::Ok => {}
            validate::TargetRootCheck::Rejected(msg) => return Err(msg),
        }
    }

    if let Some(file) = config_file(app) {
        state::atomic_write_json(&file, &cfg)
            .map_err(|e| format!("保存配置失败: {}", e))
    } else {
        Err("无法定位数据目录".into())
    }
}

// ============ R3 五级 legacy 探测链 ============

/// 尝试从旧版（WindowsClear / disk-butler）导入配置
///
/// 返回 (配置, 是否导入了 legacy 数据)
fn try_legacy_import() -> (RelocateConfig, bool) {
    let mut imported = false;
    let mut cfg = RelocateConfig::default();

    // ① WindowsClear 配置（exe 同级 config.json）
    if let Some(wc_cfg) = try_windowsclear_config() {
        cfg.scan_sources = wc_cfg.scan_sources.into_iter().map(|s| scanner::ScanSource {
            enabled: s.enabled, label: s.label, path: s.path, target_subdir: s.target_subdir,
        }).collect();
        // target_root 不继承旧值（用户定案），但记录 legacy 存在
        imported = true;
        log::info!("从 WindowsClear 导入了 scan_sources");
    }

    // ② disk-butler 旧数据（com.diskbutler.app appDataDir）
    if let Some(db_dir) = diskbutler_appdata_dir() {
        if db_dir.exists() {
            // disk-butler 的 scan-cache / cleanup-stats 存在 → 标记 legacy
            imported = true;
            log::info!("检测到 disk-butler 旧数据目录: {}", db_dir.display());
        }
    }

    // ③ WindowsClear 历史（%LOCALAPPDATA%\tanaer\WindowsClear\data\history.json）
    // → 这是 protected_paths 的数据源，由 HistoryManager 在加载时自动探测
    // （history.rs 独立管理，不并入 config）

    // target_root：运行时解析新默认（不继承旧值）
    if cfg.target_root.is_none() {
        if let Some(p) = validate::resolve_default_target_root() {
            cfg.target_root = Some(p.to_string_lossy().to_string());
        }
    }

    cfg.legacy_imported = imported;
    (cfg, imported)
}

/// WindowsClear 配置格式
#[derive(Deserialize)]
struct WcConfig {
    version: u32,
    target_root: String,
    scan_sources: Vec<WcScanSource>,
}

#[derive(Deserialize)]
struct WcScanSource {
    enabled: bool,
    label: String,
    path: String,
    target_subdir: String,
}

/// 尝试读取 WindowsClear 的 config.json
fn try_windowsclear_config() -> Option<WcConfig> {
    // WindowsClear 的 config 在 exe 同级目录（我们无法知道旧 exe 在哪）
    // P2 简化：检查常见位置
    let candidates: [Option<PathBuf>; 2] = [
        std::env::var("LOCALAPPDATA").ok().map(|d| PathBuf::from(d).join("WindowsClear").join("config.json")),
        Some(PathBuf::from(r"C:\Users\Administrator\AppData\Local\WindowsClear\config.json")),
    ];

    for path in candidates.iter().flatten() {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(wc) = serde_json::from_str::<WcConfig>(&content) {
                    return Some(wc);
                }
            }
        }
    }
    None
}

/// disk-butler 旧标识符的 appDataDir
fn diskbutler_appdata_dir() -> Option<PathBuf> {
    // 通过 Tauri 的路径解析（但需要在 AppHandle 上下文中调用）
    // P2 简化：直接构造路径
    let local = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(local).join("com.diskbutler.app"))
}
