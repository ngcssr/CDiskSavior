mod commands;
mod relocate_commands;
mod relocate;
mod state;

// P1 骨架 + P2 M4 空间腾挪命令
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            // M1 总览
            commands::get_drives,
            commands::memory_report,
            commands::pagefile_check,
            // M2 磁盘透视
            commands::scan_drive,
            commands::scan_dir,
            commands::load_scan_cache,
            // M3 一键清理
            commands::list_cleanup_items,
            commands::run_cleanup,
            commands::get_cleanup_stats,
            commands::scan_orphan_dirs,
            commands::clean_orphan_dirs,
            // M4 空间腾挪（P2 实现）
            relocate_commands::scan_relocate_targets,
            relocate_commands::list_locking_processes,
            relocate_commands::kill_locking_process,
            relocate_commands::start_migration,
            relocate_commands::pause_migration,
            relocate_commands::resume_migration,
            relocate_commands::cancel_migration,
            relocate_commands::restore_migration,
            relocate_commands::list_migration_history,
            relocate_commands::remove_migration_record,
            relocate_commands::get_relocate_config,
            relocate_commands::save_relocate_config,
            // M6 启动与内存
            commands::list_startup_items,
            commands::set_startup_enabled,
            // M7 高级维护
            commands::analyze_system_clean,
            commands::run_deep_analyze,
            commands::run_deep_clean,
            commands::run_deep_clean_system,
            commands::collect_rules,
            // M8 设置与工具
            commands::scan_service_available,
            commands::repair_scan_service,
            commands::open_apps_settings,
            commands::set_always_on_top,
            commands::open_recycle_bin
        ])
        .setup(|_app| {
            // R8: 单实例互斥（第二个实例直接退出）
            match state::SingleInstance::new() {
                Ok(_) => Ok(()),
                Err(false) => {
                    // 已有实例在运行——非错误退出（正常路径）
                    // Tauri 还没启动窗口，直接退出即可
                    eprintln!("CDisk Savior 已在运行中");
                    Ok(())
                }
                Err(true) => Ok(()), // 创建失败，保守放行
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running cdisk-savior");
}
