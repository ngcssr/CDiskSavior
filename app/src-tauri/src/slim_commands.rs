//! M5 系统瘦身的 Tauri 命令封装

use crate::slim;
use serde::Serialize;

#[tauri::command]
pub fn get_system_slim_status() -> Result<slim::SlimStatus, String> {
    Ok(slim::get_status())
}

#[tauri::command]
pub fn enable_hibernation() -> Result<String, String> {
    slim::enable_hibernation()
}

#[tauri::command]
pub fn disable_hibernation() -> Result<String, String> {
    slim::disable_hibernation()
}

#[tauri::command]
pub fn cleanup_winsxs() -> Result<String, String> {
    slim::cleanup_winsxs()
}

#[tauri::command]
pub fn cleanup_winsxs_resetbase() -> Result<String, String> {
    slim::cleanup_winsxs_resetbase()
}

#[tauri::command]
pub fn rebuild_search_index() -> Result<String, String> {
    slim::rebuild_search_index()
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
pub fn scan_old_drivers() -> Result<Vec<slim::DriverEntry>, String> {
    Ok(slim::scan_old_drivers())
}

#[tauri::command]
pub fn delete_old_drivers(ids: Vec<String>) -> Result<u32, String> {
    slim::delete_old_drivers(&ids)
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
pub fn restart_explorer() -> Result<(), String> {
    slim::restart_explorer()
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
pub fn scan_ai_model_assets() -> Result<Vec<slim::AiModelEntry>, String> {
    Ok(slim::scan_ai_model_assets())
}

#[tauri::command]
pub fn delete_ai_model(id: String) -> Result<u64, String> {
    // 先扫描获取完整列表，再按 id 删除
    let models = slim::scan_ai_model_assets();
    slim::delete_ai_model(&id, &models)
}
