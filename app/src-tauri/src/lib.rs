mod commands;

// P1 骨架命令表（disk-butler 公开集 23 条）
// 命名与处置依据：合并方案/命令映射表.csv
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
            // M4 空间腾挪
            commands::get_relocate_config,
            commands::save_relocate_config,
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
        .run(tauri::generate_context!())
        .expect("error while running cdisk-savior");
}
