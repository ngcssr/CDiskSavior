//! M5 系统瘦身的 Tauri 命令封装

use crate::slim;

/// 获取状态——async：check_hibernation 调 powercfg + get_winsxs_size 遍历
#[tauri::command]
pub async fn get_system_slim_status() -> Result<slim::SlimStatus, String> {
    tauri::async_runtime::spawn_blocking(slim::get_status)
        .await
        .map_err(|e| format!("获取状态失败: {}", e))
}

#[tauri::command]
pub async fn enable_hibernation() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(slim::enable_hibernation)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub async fn disable_hibernation() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(slim::disable_hibernation)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub async fn cleanup_winsxs() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(slim::cleanup_winsxs)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub async fn cleanup_winsxs_resetbase() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(slim::cleanup_winsxs_resetbase)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub async fn rebuild_search_index() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(slim::rebuild_search_index)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub fn open_virtual_memory_settings() -> Result<(), String> {
    slim::open_virtual_memory_settings()
}

#[tauri::command]
pub fn open_storage_settings() -> Result<(), String> {
    slim::open_storage_settings()
}

#[tauri::command]
pub fn open_disk_cleanup() -> Result<(), String> {
    slim::open_disk_cleanup()
}

#[tauri::command]
pub async fn scan_old_drivers() -> Result<Vec<slim::DriverEntry>, String> {
    tauri::async_runtime::spawn_blocking(slim::scan_old_drivers)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

#[tauri::command]
pub async fn delete_old_drivers(ids: Vec<String>) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || slim::delete_old_drivers(&ids))
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub fn open_driver_backup_dir() -> Result<(), String> {
    Err("待实现".into())
}

#[tauri::command]
pub fn restore_all_driver_backups() -> Result<(), String> {
    Err("待实现".into())
}

#[tauri::command]
pub fn scan_registry_redundancy() -> Result<Vec<slim::RegistryEntry>, String> {
    Ok(slim::scan_registry_redundancy())
}

#[tauri::command]
pub fn delete_registry_entries(entries: Vec<serde_json::Value>) -> Result<u32, String> {
    let _ = entries;
    Ok(0)
}

#[tauri::command]
pub fn open_registry_backup_dir() -> Result<(), String> {
    Err("待实现".into())
}

#[tauri::command]
pub fn scan_shell_icons() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "icons": [], "phase": "P5" }))
}

#[tauri::command]
pub fn remove_shell_icon(id: String) -> Result<(), String> {
    Err(format!("待实现: {}", id))
}

#[tauri::command]
pub fn restore_shell_icon() -> Result<(), String> { Err("待实现".into()) }

#[tauri::command]
pub fn unlock_shell_icon() -> Result<(), String> { Err("待实现".into()) }

#[tauri::command]
pub fn open_shell_icon_backup_dir() -> Result<(), String> { Err("待实现".into()) }

#[tauri::command]
pub fn open_shell_icon_log() -> Result<(), String> { Err("待实现".into()) }

#[tauri::command]
pub fn open_shell_icon_registry() -> Result<(), String> { Err("待实现".into()) }

#[tauri::command]
pub async fn restart_explorer() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(slim::restart_explorer)
        .await
        .map_err(|e| format!("执行失败: {}", e))?
}

#[tauri::command]
pub fn scan_context_menu() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "entries": [], "phase": "P5" }))
}

#[tauri::command]
pub fn delete_context_menu_entries(ids: Vec<String>) -> Result<u32, String> {
    let _ = ids;
    Ok(0)
}

#[tauri::command]
pub async fn scan_ai_model_assets() -> Result<Vec<slim::AiModelEntry>, String> {
    tauri::async_runtime::spawn_blocking(slim::scan_ai_model_assets)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

#[tauri::command]
pub async fn delete_ai_model(id: String) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let models = slim::scan_ai_model_assets();
        slim::delete_ai_model(&id, &models)
    })
    .await
    .map_err(|e| format!("删除失败: {}", e))?
}
