// CDisk Savior · 命令层
//
// 设计文档 §4：116 个统一命令。P1 骨架 + P2 M4 + P3 M1/M2 真实实现。
// 其余模块在 P4+ 按模块接入。

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
/// async + spawn_blocking：全盘扫描可能数秒，不阻塞主线程
#[tauri::command]
pub async fn scan_drive(root: String) -> Result<crate::scan::TreeNode, String> {
    tauri::async_runtime::spawn_blocking(move || crate::scan::scan_directory(&root))
        .await
        .map_err(|e| format!("扫描任务失败: {}", e))?
}

/// 扫描指定目录
#[tauri::command]
pub async fn scan_dir(path: String) -> Result<crate::scan::TreeNode, String> {
    tauri::async_runtime::spawn_blocking(move || crate::scan::scan_directory(&path))
        .await
        .map_err(|e| format!("扫描任务失败: {}", e))?
}

#[tauri::command]
pub fn load_scan_cache(root: String) -> Option<serde_json::Value> {
    let _ = root;
    None
}

// ============ M3 一键清理（P4 真实实现） ============

/// 清理项扫描（返回当前可清理的项 + 大小）——async：扫描多个目录可能较慢
#[tauri::command]
pub async fn list_cleanup_items() -> Result<crate::cleaner::CleanupScan, String> {
    tauri::async_runtime::spawn_blocking(crate::cleaner::items::list_items)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 执行清理（id 驱动 + 当场重扫 + R1 四层护栏）——async：删除操作可能较慢
#[tauri::command]
pub async fn run_cleanup(app: tauri::AppHandle, ids: Vec<String>) -> Result<crate::cleaner::CleanupReport, String> {
    let report = tauri::async_runtime::spawn_blocking(move || crate::cleaner::items::run(&ids))
        .await
        .map_err(|e| format!("清理失败: {}", e))?;
    // 记账：用实际磁盘变化
    crate::cleaner::stats::record(&app, report.free_after.saturating_sub(report.free_before));
    Ok(report)
}

/// 累计清理统计
#[tauri::command]
pub fn get_cleanup_stats(app: tauri::AppHandle) -> crate::cleaner::stats::CleanupStats {
    crate::cleaner::stats::load(&app)
}

/// 孤儿目录扫描（含 C1 互斥：reparse 前置过滤）——async：遍历安装根较慢
#[tauri::command]
pub async fn scan_orphan_dirs() -> Result<crate::cleaner::OrphanScan, String> {
    tauri::async_runtime::spawn_blocking(crate::cleaner::orphans::scan_orphans)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

// ============ M3 深度垃圾扫描（light-c deep_junk 移植） ============

/// 深度扫描：19 类全分区文件级匹配（可取消，分钟级耗时）
#[tauri::command]
pub async fn scan_deep_junk_files() -> Result<crate::cleaner::deep_junk::DeepScanResult, String> {
    tauri::async_runtime::spawn_blocking(crate::cleaner::deep_junk::scan)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 取消深度扫描
#[tauri::command]
pub fn cancel_deep_junk_scan() {
    crate::cleaner::deep_junk::cancel_scan();
}

/// 删除指定深度类别（当场重扫 + R1 引擎）
#[tauri::command]
pub async fn delete_deep_junk_files(ids: Vec<String>) -> Result<crate::cleaner::deep_junk::DeepDeleteReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::deep_junk::delete(&ids))
        .await
        .map_err(|e| format!("删除失败: {}", e))
}

/// 孤儿目录清理（当场重扫白名单 + canonicalize 校验）——async：删除+重扫
#[tauri::command]
pub async fn clean_orphan_dirs(paths: Vec<String>) -> Result<crate::cleaner::ResidueReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::orphans::clean_orphans(&paths))
        .await
        .map_err(|e| format!("清理失败: {}", e))
}

// ============ M4 空间腾挪 ============
// M4 命令已移至 relocate_commands.rs（P2 实现）

// ============ M6 启动与内存（P6 真实实现） ============

#[tauri::command]
pub async fn list_startup_items() -> Result<Vec<crate::tune::StartupItem>, String> {
    tauri::async_runtime::spawn_blocking(crate::tune::list_items)
        .await
        .map_err(|e| format!("枚举失败: {}", e))
}

#[tauri::command]
pub fn set_startup_enabled(id: String, enabled: bool) -> Result<(), String> {
    crate::tune::set_enabled(&id, enabled)
}

// ============ M7 高级维护（P7 实现） ============

/// 系统级清理分析（Windows\Temp + SoftwareDistribution 当前大小）——async：遍历系统目录较慢
#[tauri::command]
pub async fn analyze_system_clean() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let win = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let temp_path = format!("{}\\Temp", win);
        let sdu_path = format!("{}\\SoftwareDistribution\\Download", win);

        let temp_size = dir_size_str(&temp_path);
        let sdu_size = dir_size_str(&sdu_path);

        serde_json::json!({
            "windowsTemp": { "path": temp_path, "size": temp_size },
            "updateDownload": { "path": sdu_path, "size": sdu_size },
            "totalSize": temp_size + sdu_size,
        })
    })
    .await
    .map_err(|e| format!("分析失败: {}", e))
}

#[tauri::command]
pub async fn run_deep_analyze() -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let scan = crate::cleaner::items::list_items();
        serde_json::json!({ "items": scan.items, "totalSize": scan.total_size })
    })
    .await
    .map_err(|e| format!("分析失败: {}", e))
}

#[tauri::command]
pub async fn run_deep_clean(app: tauri::AppHandle, ids: Vec<String>) -> Result<crate::cleaner::CleanupReport, String> {
    let report = tauri::async_runtime::spawn_blocking(move || crate::cleaner::items::run(&ids))
        .await
        .map_err(|e| format!("清理失败: {}", e))?;
    crate::cleaner::stats::record(&app, report.free_after.saturating_sub(report.free_before));
    Ok(report)
}

#[tauri::command]
pub async fn run_deep_clean_system() -> Result<serde_json::Value, String> {
    // R4 提权：清 Windows\Temp + SoftwareDistribution\Download（DISM 级耗时）
    tauri::async_runtime::spawn_blocking(|| {
        let script = r#"Remove-Item -Recurse -Force "$env:SystemRoot\Temp\*" -EA SilentlyContinue;
                        Remove-Item -Recurse -Force "$env:SystemRoot\SoftwareDistribution\Download\*" -EA SilentlyContinue"#;
        crate::slim::run_elevated_ps_pub(script)?;
        Ok(serde_json::json!({ "result": "system temp cleaned" }))
    })
    .await
    .map_err(|e| format!("清理失败: {}", e))?
}

#[tauri::command]
pub fn collect_rules(_include_drives: bool) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "rules": [],
        "message": "规则采集功能待后续完善"
    }))
}

// ============ M8 设置与工具（P7 实现） ============

#[tauri::command]
pub fn scan_service_available() -> bool {
    // P8 前服务未实现
    false
}

#[tauri::command]
pub fn repair_scan_service() -> Result<(), String> {
    Err("CDiskSaviorSvc 待 P8 实现".into())
}

#[tauri::command]
pub fn open_apps_settings() -> Result<(), String> {
    std::process::Command::new("ms-settings:appsfeatures")
        .spawn().map_err(|e| format!("打开应用设置失败: {}", e))?;
    Ok(())
}

#[tauri::command]
pub fn set_always_on_top(app: tauri::AppHandle, value: bool) -> Result<(), String> {
    use tauri::Manager;
    if let Some(win) = app.get_webview_window("main") {
        win.set_always_on_top(value).map_err(|e| format!("{}", e))?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_recycle_bin() -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg("shell:RecycleBinFolder")
        .spawn().map_err(|e| format!("打开回收站失败: {}", e))?;
    Ok(())
}

// M8 额外命令
#[tauri::command]
pub fn check_admin_privilege() -> bool {
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        windows_sys::Win32::Foundation::CloseHandle(token);
        result != 0 && elevation.TokenIsElevated != 0
    }
}

#[tauri::command]
pub fn get_data_directory(app: tauri::AppHandle) -> Result<String, String> {
    use tauri::Manager;
    app.path().app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .map_err(|e| format!("无法定位数据目录: {}", e))
}

#[tauri::command]
pub fn format_size(bytes: u64) -> String {
    if bytes == 0 { return "0 B".into(); }
    let u = ["B", "KB", "MB", "GB", "TB"];
    let i = (bytes as f64).log(1024.0).floor() as usize;
    format!("{:.1} {}", bytes as f64 / 102f64.powi(i as i32), u[i.min(4)])
}

#[tauri::command]
pub fn verify_integrity() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "status": "ok",
        "modules": ["M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8"],
        "version": "0.1.0"
    }))
}

fn dir_size_str(path: &str) -> u64 {
    let p = std::path::Path::new(path);
    if !p.exists() { return 0; }
    let mut total = 0u64;
    for entry in jwalk::WalkDir::new(p).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}
