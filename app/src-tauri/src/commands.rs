// CDisk Savior · 命令层
//
// 设计文档 §4：116 个统一命令。P1 骨架 + P2 M4 + P3 M1/M2 真实实现。
// 其余模块在 P4+ 按模块接入。

use serde::Serialize;

/// 统一的占位错误类型
#[derive(Debug, Serialize)]
pub struct NotReady {
    pub cmd: &'static str,
    pub phase: &'static str,
}

pub fn not_ready(cmd: &'static str) -> NotReady {
    NotReady { cmd, phase: "待接入真实实现" }
}

// ============ M1 总览（P3 真实实现） ============

/// 驱动器列表（真实 Windows API：GetLogicalDriveStringsW + GetDiskFreeSpaceExW）
#[tauri::command]
pub fn get_drives() -> Result<Vec<crate::scan::DriveInfo>, String> {
    Ok(crate::scan::get_drives())
}

/// 内存报告（GlobalMemoryStatusEx）
#[tauri::command]
pub fn memory_report() -> Result<serde_json::Value, String> {
    Ok(crate::scan::memory_report())
}

/// 页面文件检查
#[tauri::command]
pub fn pagefile_check() -> Result<serde_json::Value, String> {
    Ok(crate::scan::pagefile_check())
}

// ============ M2 磁盘透视（P3 真实实现） ============

/// 扫描驱动器（jwalk 并行目录树 → TreeNode）
#[tauri::command]
pub fn scan_drive(root: String) -> Result<crate::scan::TreeNode, String> {
    crate::scan::scan_directory(&root)
}

/// 扫描指定目录
#[tauri::command]
pub fn scan_dir(path: String) -> Result<crate::scan::TreeNode, String> {
    crate::scan::scan_directory(&path)
}

#[tauri::command]
pub fn load_scan_cache(root: String) -> Option<serde_json::Value> {
    let _ = root;
    None
}

// ============ M3 一键清理（P4 真实实现） ============

/// 清理项扫描（返回当前可清理的项 + 大小）
#[tauri::command]
pub fn list_cleanup_items() -> Result<crate::cleaner::CleanupScan, String> {
    Ok(crate::cleaner::items::list_items())
}

/// 执行清理（id 驱动 + 当场重扫 + R1 四层护栏）
#[tauri::command]
pub fn run_cleanup(app: tauri::AppHandle, ids: Vec<String>) -> Result<crate::cleaner::CleanupReport, String> {
    let report = crate::cleaner::items::run(&ids);
    // 记账：用实际磁盘变化
    crate::cleaner::stats::record(&app, report.free_after.saturating_sub(report.free_before));
    Ok(report)
}

/// 累计清理统计
#[tauri::command]
pub fn get_cleanup_stats(app: tauri::AppHandle) -> crate::cleaner::stats::CleanupStats {
    crate::cleaner::stats::load(&app)
}

/// 孤儿目录扫描（含 C1 互斥：reparse 前置过滤）
#[tauri::command]
pub fn scan_orphan_dirs() -> Result<crate::cleaner::OrphanScan, String> {
    Ok(crate::cleaner::orphans::scan_orphans())
}

/// 孤儿目录清理（当场重扫白名单 + canonicalize 校验）
#[tauri::command]
pub fn clean_orphan_dirs(paths: Vec<String>) -> Result<crate::cleaner::ResidueReport, String> {
    Ok(crate::cleaner::orphans::clean_orphans(&paths))
}

// ============ M4 空间腾挪 ============
// M4 命令已移至 relocate_commands.rs（P2 实现）

// ============ M6 启动与内存 ============

#[tauri::command]
pub fn list_startup_items() -> Result<Vec<serde_json::Value>, String> {
    Ok(vec![])
}

#[tauri::command]
pub fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    // 安全（V27）：写入前须当场重扫校验 id 存在；P1 拒绝
    Err(format!("P1 骨架：set_startup_enabled({id}, {enabled}) 待 P6 接入（含 V27 复验）"))
}

// ============ M7 高级维护 ============

#[tauri::command]
pub fn analyze_system_clean() -> Result<serde_json::Value, String> {
    Err("P1 骨架：DISM 分析待 P7 接入（R4 按操作提权）".into())
}

#[tauri::command]
pub fn run_deep_analyze() -> Result<serde_json::Value, String> {
    Err("P1 骨架：待 P7".into())
}

#[tauri::command]
pub fn run_deep_clean() -> Result<serde_json::Value, String> {
    Err("P1 骨架：待 P7".into())
}

#[tauri::command]
pub fn run_deep_clean_system() -> Result<serde_json::Value, String> {
    Err("P1 骨架：待 P7（需 UAC，退出码 1223 判取消）".into())
}

#[tauri::command]
pub fn collect_rules(include_drives: bool) -> Result<serde_json::Value, String> {
    Err(format!("P1 骨架：collect_rules({include_drives}) 待 P7"))
}

// ============ M8 设置与工具 ============

#[tauri::command]
pub fn scan_service_available() -> bool {
    false // P1：CDiskSaviorSvc 尚未实现，始终回退慢速路径
}

#[tauri::command]
pub fn repair_scan_service() -> Result<(), String> {
    Err("P1 骨架：CDiskSaviorSvc 待 P8（R4：提权目标校验后接入）".into())
}

#[tauri::command]
pub fn open_apps_settings() -> Result<(), String> {
    Err("P1 骨架".into())
}

#[tauri::command]
pub fn set_always_on_top(value: bool) -> Result<(), String> {
    let _ = value;
    Err("P1 骨架".into())
}

#[tauri::command]
pub fn open_recycle_bin() -> Result<(), String> {
    Err("P1 骨架".into())
}
