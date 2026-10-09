// CDisk Savior · P1 骨架命令层
//
// 设计文档 §4：三方共 119 条命令 → 116 个统一命令。
// P1 阶段注册 disk-butler 公开集的 23 条（设计文档核验值），
// 其余（light-c 84 + WindowsClear 移植 12）在 P2+ 按模块接入。
// 全部为占位实现：返回结构化的"未实现"错误或最小可用数据。

use serde::Serialize;

/// 统一的"P1 骨架占位"错误类型
#[derive(Debug, Serialize)]
pub struct NotReady {
    pub cmd: &'static str,
    pub phase: &'static str,
}

pub fn not_ready(cmd: &'static str) -> NotReady {
    NotReady { cmd, phase: "P1 骨架：待接入真实实现" }
}

// ============ 通用类型（P1 最小集） ============

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub mount: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
    pub fs: String,
    pub kind: String,
}

// ============ M1 总览 ============

/// 驱动器列表（P1 真实实现：GetLogicalDrives + GetDiskFreeSpaceExW）
#[tauri::command]
pub fn get_drives() -> Result<Vec<DriveInfo>, String> {
    let mut drives = Vec::new();
    // P1：先返回 C:\ 的静态占位，P2 接 windows-sys 真实枚举
    drives.push(DriveInfo {
        mount: "C:\\".into(),
        label: "系统盘".into(),
        total: 0,
        free: 0,
        fs: "NTFS".into(),
        kind: "fixed".into(),
    });
    Ok(drives)
}

#[tauri::command]
pub fn memory_report() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "overview": { "total": 0, "used": 0, "swapTotal": 0 },
        "processes": [],
        "phase": "P1"
    }))
}

#[tauri::command]
pub fn pagefile_check() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "consistent": true, "details": [], "phase": "P1" }))
}

// ============ M2 磁盘透视 ============

#[tauri::command]
pub fn scan_drive(root: String) -> Result<serde_json::Value, String> {
    let _ = root;
    Err("P1 骨架：scan_drive 待 P3 接入 MFT/TreeMap 引擎".into())
}

#[tauri::command]
pub fn scan_dir(path: String) -> Result<serde_json::Value, String> {
    let _ = path;
    Err("P1 骨架：scan_dir 待 P3 接入".into())
}

#[tauri::command]
pub fn load_scan_cache(root: String) -> Option<serde_json::Value> {
    let _ = root;
    None
}

// ============ M3 一键清理 ============

#[tauri::command]
pub fn list_cleanup_items() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "items": [], "phase": "P1 骨架" }))
}

#[tauri::command]
pub fn run_cleanup(ids: Vec<String>) -> Result<serde_json::Value, String> {
    Err(format!("P1 骨架：run_cleanup({} 个 id) 待 P4 接入统一删除引擎", ids.len()))
}

#[tauri::command]
pub fn get_cleanup_stats() -> serde_json::Value {
    serde_json::json!({ "totalFreed": 0, "totalRuns": 0, "lastAt": 0 })
}

#[tauri::command]
pub fn scan_orphan_dirs() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "confirmed": [], "phase": "P1 骨架" }))
}

#[tauri::command]
pub fn clean_orphan_dirs(paths: Vec<String>) -> Result<serde_json::Value, String> {
    // 安全（R1）：id/路径白名单重扫在 P4 实现；P1 拒绝一切
    Err(format!("P1 骨架：clean_orphan_dirs 拒绝执行（{} 条路径）——待 P4 统一删除引擎就位", paths.len()))
}

// ============ M4 空间腾挪 ============

#[tauri::command]
pub fn get_relocate_config() -> Result<serde_json::Value, String> {
    // R3：P2 接入 legacy 探测链后返回真实配置
    Ok(serde_json::json!({
        "version": 1,
        "target_root": null,
        "scan_sources": [],
        "phase": "P1 骨架：默认迁移根待 §3.2 运行时解析"
    }))
}

#[tauri::command]
pub fn save_relocate_config(config: serde_json::Value) -> Result<(), String> {
    let _ = config;
    Err("P1 骨架：save_relocate_config 待 P2（含 R9 校验）".into())
}

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
