mod appdata;
mod bigfiles;
mod cleaner;
mod context_menu;
mod growth;
mod health;
mod commands;
mod registry_clean;
mod relocate_commands;
mod relocate;
mod scan;
mod shell_icons;
mod slim;
mod slim_commands;
mod state;
mod tune;

// P1 骨架 + P2 M4 空间腾挪命令
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            // M1 总览
            commands::get_drives,
            commands::memory_report,
            commands::query_disk_health,
            commands::get_health_score,
            commands::pagefile_check,
            // M2 磁盘透视
            commands::scan_drive,
            commands::scan_dir,
            commands::scan_hotspot,
            commands::create_disk_snapshot,
            commands::compare_disk_growth,
            commands::scan_large_files,
            commands::cancel_large_file_scan,
            commands::has_disk_snapshot,
            commands::load_scan_cache,
            // M2 分类扫描/分页 + 增长明细导出（P1）
            commands::scan_junk_files,
            commands::get_categories,
            commands::scan_category,
            commands::get_deep_junk_category_page,
            commands::scan_disk_growth,
            commands::cancel_disk_growth_scan,
            commands::get_disk_growth_file_details,
            commands::get_disk_growth_directory_details,
            commands::get_disk_growth_export_tree,
            commands::save_disk_growth_html,
            // M3 一键清理
            commands::list_cleanup_items,
            commands::run_cleanup,
            commands::get_cleanup_stats,
            commands::scan_orphan_dirs,
            commands::clean_orphan_dirs,
            commands::scan_deep_junk_files,
            commands::cancel_deep_junk_scan,
            commands::delete_deep_junk_files,
            commands::scan_uninstall_leftovers,
            commands::check_leftover_safety,
            commands::delete_leftovers,
            commands::list_leftover_whitelist,
            commands::add_leftover_whitelist_entry,
            commands::remove_leftover_whitelist_entry,
            commands::scan_social_cache,
            commands::delete_social_cache,
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
            commands::open_recycle_bin,
            commands::open_file,
            // M8 设置与工具（P7 实现）
            commands::check_admin_privilege,
            commands::get_data_directory,
            commands::format_size,
            commands::verify_integrity,
            commands::migrate_legacy_portable_data,
            // M5 系统瘦身（P5 实现）
            slim_commands::get_system_slim_status,
            slim_commands::enable_hibernation,
            slim_commands::disable_hibernation,
            slim_commands::cleanup_winsxs,
            slim_commands::cleanup_winsxs_resetbase,
            slim_commands::rebuild_search_index,
            slim_commands::open_virtual_memory_settings,
            slim_commands::open_storage_settings,
            slim_commands::open_disk_cleanup,
            slim_commands::scan_old_drivers,
            slim_commands::delete_old_drivers,
            slim_commands::restore_all_driver_backups,
            slim_commands::open_driver_backup_dir,
            slim_commands::scan_registry_redundancy,
            slim_commands::delete_registry_entries,
            slim_commands::open_registry_backup_dir,
            slim_commands::scan_shell_icons,
            slim_commands::remove_shell_icon,
            slim_commands::restore_shell_icon,
            slim_commands::unlock_shell_icon,
            slim_commands::open_shell_icon_backup_dir,
            slim_commands::open_shell_icon_log,
            slim_commands::open_shell_icon_registry,
            slim_commands::restart_explorer,
            slim_commands::scan_context_menu,
            slim_commands::delete_context_menu_entries,
            slim_commands::scan_ai_model_assets,
            slim_commands::delete_ai_model
        ])
        .setup(|_app| {
            // R8: 单实例互斥（修复：句柄必须持有到进程退出，第二个实例直接 exit）
            match state::SingleInstance::new() {
                Ok(handle) => {
                    // 关键：forget 持有互斥体直到进程退出。
                    // 此前 Ok(_) 直接 drop → 互斥体立刻释放 → 保护完全失效。
                    std::mem::forget(handle);
                    Ok(())
                }
                Err(false) => {
                    // 已有实例在运行——立即退出本进程（否则双实例并行写状态）
                    eprintln!("CDisk Savior 已在运行中，第二实例退出");
                    std::process::exit(0);
                }
                Err(true) => Ok(()), // 创建失败，保守放行
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running cdisk-savior");
}
