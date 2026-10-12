mod appdata;
mod anti_tamper;
mod auto_elevate;
mod bigfiles;
mod bloatware;
mod cleaner;
mod collector;
mod context_menu;
mod growth;
mod health;
mod commands;
mod icon;
mod lightc_mft;
#[cfg(test)]
mod lightc_bench;
pub mod knowledge;
pub mod mft_scan;
mod registry_clean;
mod relocate_commands;
mod relocate;
mod scan;
mod shell_icons;
mod slim;
mod slim_commands;
mod state;
pub mod svc;
pub mod svc_client;
mod tune;

// P1 骨架 + P2 M4 空间腾挪命令

/// 启动计时诊断（定位白屏阶段；写入临时文件，稳定后可移除）
static STARTUP_T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
fn startup_log(msg: &str) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use std::io::Write;
        let t0 = STARTUP_T0.get_or_init(std::time::Instant::now);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("cdisk_startup.log"))
        {
            let _ = writeln!(f, "[{:>6}ms] {}", t0.elapsed().as_millis(), msg);
        }
    }
}

/// 扫描性能诊断日志（%TEMP%\cdisk_scan.log，unix 毫秒时间戳，相邻行差值即阶段耗时）
pub fn scan_log(msg: &str) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        use std::io::Write;
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("cdisk_scan.log"))
        {
            let _ = writeln!(f, "[{}] {}", ms, msg);
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    STARTUP_T0.get_or_init(std::time::Instant::now);
    startup_log("run enter");

    auto_elevate::ensure_elevation();
    startup_log("after auto_elevate");

    anti_tamper::startup_guard(); // C 级防逆向：反调试守卫（release 生效）
    startup_log("after anti_tamper");

    // 便携版：WebView2 profile 落 exe 旁 webview/main（LightC 同款效果）。
    // 用 WEBVIEW2_USER_DATA_FOLDER 环境变量实现（WebView2 Loader 官方机制，
    // 优先级低于显式参数、高于默认 %LOCALAPPDATA%）。
    // 不改窗口创建流程——手动建窗在本机有兼容性问题（23:16 版踩坑）。
    // profile 在 C 盘 %LOCALAPPDATA%（磁盘曾 99.5% 满读写慢）会拖慢
    // WebView2 渲染进程初始化 → 启动白屏；落到 exe 旁即规避。
    if appdata::portable_mode() {
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.to_path_buf()))
            .map(|p| p.join("webview").join("main"))
        {
            if dir.is_absolute() && std::fs::create_dir_all(&dir).is_ok() {
                std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &dir);
                startup_log("WEBVIEW2_USER_DATA_FOLDER set (portable)");
            }
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            // 零白屏：前端 DOM 就绪后触发 Rust show
            commands::window_ready,
            // M1 总览
            commands::get_drives,
            commands::memory_report,
            commands::get_disk_health,
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
            // Q4 软件体检（feature_bloatware）
            commands::scan_bloatware,
            commands::uninstall_software,
            commands::scan_residue,
            commands::clean_residue,
            commands::bloatware_set_ignored,
            commands::stop_software,
            commands::op_started,
            commands::bloatware_force_preview,
            commands::force_uninstall_software,
            commands::open_official_uninstaller,
            // M3 批量删除系（P2）
            commands::delete_files,
            commands::enhanced_delete_files,
            commands::delete_leftover_folders,
            commands::delete_leftovers_permanent,
            commands::cleanup_directory_contents,
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
            relocate_commands::get_target_drive_free,
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
            // M1 系统信息 + M2 热点取消 + M3 动作记录（P3）
            commands::get_disk_info,
            commands::get_system_info,
            commands::record_cleanup_action,
            commands::cancel_hotspot_scan,
            commands::scan_path_direct,
            // M8 设置与工具批（P3）
            commands::pick_folder_dialog,
            commands::get_physical_size,
            commands::open_logs_folder,
            commands::set_data_directory,
            commands::get_distribution_channel,
            commands::get_storage_location_info,
            commands::get_storage_write_diagnostic,
            commands::list_clearable_data_items,
            commands::clear_local_data,
            commands::clear_selected_local_data,
            commands::check_admin_for_path,
            commands::open_startup_manager,
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
        .on_page_load(|_webview, _payload| {
            startup_log("on_page_load");
        })
        .setup(|app| {
            startup_log("setup enter (WebView created)");
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
