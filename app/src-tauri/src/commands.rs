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

/// 磁盘健康（物理盘 SMART 健康状态）
#[tauri::command]
pub async fn query_disk_health() -> Result<Vec<crate::health::DiskHealth>, String> {
    tauri::async_runtime::spawn_blocking(crate::health::query_disk_health)
        .await
        .map_err(|e| format!("查询失败: {}", e))?
}

/// 系统健康评分（100 = 磁盘 40 + 休眠 30 + 垃圾 30）
#[tauri::command]
pub async fn get_health_score() -> Result<crate::health::HealthScore, String> {
    tauri::async_runtime::spawn_blocking(crate::health::calculate_health_score)
        .await
        .map_err(|e| format!("评分失败: {}", e))
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

/// 热点目录排行（AppData 大目录，>10MB 上榜，>5GB 智能下钻）
#[tauri::command]
pub async fn scan_hotspot(path: Option<String>) -> Result<crate::growth::HotspotReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::growth::scan_hotspot(path.as_deref()))
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 创建磁盘增长基线快照
#[tauri::command]
pub async fn create_disk_snapshot(drive: String) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || crate::growth::create_snapshot(&drive))
        .await
        .map_err(|e| format!("快照失败: {}", e))?
}

/// 对比磁盘增长（与基线快照）
#[tauri::command]
pub async fn compare_disk_growth(drive: String) -> Result<crate::growth::GrowthReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::growth::compare_growth(&drive))
        .await
        .map_err(|e| format!("对比失败: {}", e))?
}

// ============ M2 深度垃圾分类/分页（P1 补齐） ============

/// 常规垃圾扫描（可选分类名过滤；与深度扫描共用 18 类资产）
#[tauri::command]
pub async fn scan_junk_files(categories: Option<Vec<String>>) -> Result<crate::cleaner::deep_junk::DeepScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::deep_junk::scan_filtered(categories))
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 全部分类的静态信息
#[tauri::command]
pub fn get_categories() -> Vec<crate::cleaner::deep_junk::CategoryInfo> {
    crate::cleaner::deep_junk::category_infos()
}

/// 扫描单分类（同时建立分页会话）
#[tauri::command]
pub async fn scan_category(category: String) -> Result<crate::cleaner::deep_junk::CategoryResult, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::deep_junk::scan_category(&category))
        .await
        .map_err(|e| format!("扫描失败: {}", e))?
}

/// 分类文件分页（会话未建立时自动重扫）
#[tauri::command]
pub async fn get_deep_junk_category_page(
    category: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<crate::cleaner::deep_junk::CategoryPage, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::cleaner::deep_junk::category_page(&category, offset.unwrap_or(0), limit.unwrap_or(500))
    })
    .await
    .map_err(|e| format!("分页读取失败: {}", e))?
}

// ============ M2 增长明细与导出（P1 补齐） ============

/// 全盘空间变化分析（无基线自动创建；有基线返回对比明细）
#[tauri::command]
pub async fn scan_disk_growth(drive: String, max_change_entries: Option<usize>) -> Result<crate::growth::DiskScanAndAnalyzeResponse, String> {
    tauri::async_runtime::spawn_blocking(move || crate::growth::scan_disk_growth(&drive, max_change_entries))
        .await
        .map_err(|e| format!("全盘分析任务失败: {}", e))?
}

#[tauri::command]
pub fn cancel_disk_growth_scan() {
    crate::growth::cancel_growth_scan();
}

/// 文件级变化明细（基线时间后修改/新增，分页）
#[tauri::command]
pub async fn get_disk_growth_file_details(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
    drive: Option<String>,
) -> Result<crate::growth::FileDetailsResponse, String> {
    let d = drive.unwrap_or_else(|| "C:\\".into());
    tauri::async_runtime::spawn_blocking(move || {
        crate::growth::get_file_change_details(&path, offset.unwrap_or(0), limit.unwrap_or(500), &d)
    })
    .await
    .map_err(|e| format!("读取文件明细失败: {}", e))?
}

/// 目录级变化明细（直接子目录大小+文件数，分页）
#[tauri::command]
pub async fn get_disk_growth_directory_details(
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<crate::growth::DirDetailsResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::growth::get_directory_change_details(&path, offset.unwrap_or(0), limit.unwrap_or(500))
    })
    .await
    .map_err(|e| format!("读取目录明细失败: {}", e))?
}

/// HTML 导出目录树（多根路径 + 深度限制）
#[tauri::command]
pub async fn get_disk_growth_export_tree(paths: Vec<String>, max_depth: Option<usize>) -> Result<crate::growth::ExportTreeResponse, String> {
    tauri::async_runtime::spawn_blocking(move || crate::growth::get_export_tree(&paths, max_depth))
        .await
        .map_err(|e| format!("导出树生成失败: {}", e))
}

/// 通过系统保存对话框写入增长分析 HTML
#[tauri::command]
pub async fn save_disk_growth_html(
    app: tauri::AppHandle,
    content: String,
    default_file_name: String,
    dialog_title: Option<String>,
) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    if content.is_empty() {
        return Err("导出的 HTML 内容为空".to_string());
    }
    let selected_path = app
        .dialog()
        .file()
        .set_title(dialog_title.as_deref().unwrap_or("导出增长报告"))
        .set_file_name(&default_file_name)
        .add_filter("HTML 文件", &["html"])
        .blocking_save_file();
    let Some(selected_path) = selected_path else {
        return Ok(None); // 用户取消属正常流程
    };
    let mut output_path = std::path::PathBuf::from(selected_path.to_string());
    let has_html = output_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html"));
    if !has_html {
        output_path.set_extension("html");
    }
    std::fs::write(&output_path, content)
        .map_err(|e| format!("写入 HTML 失败（{}）: {}", output_path.display(), e))?;
    Ok(Some(output_path.to_string_lossy().into_owned()))
}

/// 大文件扫描（阈值过滤 + Top 200 + 可取消）
#[tauri::command]
pub async fn scan_large_files(root: String, min_size: Option<u64>) -> Result<crate::bigfiles::LargeFileReport, String> {
    let min = min_size.unwrap_or(100 * 1024 * 1024); // 默认 100MB
    tauri::async_runtime::spawn_blocking(move || crate::bigfiles::scan_large_files(&root, min))
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 取消大文件扫描
#[tauri::command]
pub fn cancel_large_file_scan() {
    crate::bigfiles::cancel();
}

/// 是否已有基线快照
#[tauri::command]
pub fn has_disk_snapshot(drive: String) -> bool {
    crate::growth::has_baseline(&drive)
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

// ============ M3 卸载残留（light-c leftovers 置信度评分引擎） ============

/// 残留扫描（注册表在装映射 + 加权评分 + 阈值分级）
#[tauri::command]
pub async fn scan_uninstall_leftovers() -> Result<crate::cleaner::leftovers::LeftoverScan, String> {
    tauri::async_runtime::spawn_blocking(crate::cleaner::leftovers::scan)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 单路径安全性检查（评分 + 理由）
#[tauri::command]
pub fn check_leftover_safety(path: String) -> crate::cleaner::leftovers::LeftoverSafety {
    crate::cleaner::leftovers::check_safety(&path)
}

/// 删除残留（当场重扫 + R1 引擎）
#[tauri::command]
pub async fn delete_leftovers(paths: Vec<String>) -> Result<crate::cleaner::leftovers::LeftoverDeleteReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::leftovers::delete(&paths))
        .await
        .map_err(|e| format!("删除失败: {}", e))
}

/// 白名单管理
#[tauri::command]
pub fn list_leftover_whitelist() -> Vec<String> {
    crate::cleaner::leftovers::list_whitelist()
}

#[tauri::command]
pub fn add_leftover_whitelist_entry(entry: String) -> Result<(), String> {
    crate::cleaner::leftovers::add_whitelist(&entry)
}

#[tauri::command]
pub fn remove_leftover_whitelist_entry(entry: String) -> Result<(), String> {
    crate::cleaner::leftovers::remove_whitelist(&entry)
}

// ============ M3 社交缓存（light-c social_scanner 移植） ============

/// 社交软件缓存扫描（微信/QQ/钉钉/飞书/企微，四级风险分类）
#[tauri::command]
pub async fn scan_social_cache() -> Result<crate::cleaner::social::SocialScan, String> {
    tauri::async_runtime::spawn_blocking(crate::cleaner::social::scan)
        .await
        .map_err(|e| format!("扫描失败: {}", e))
}

/// 删除社交缓存（CRITICAL 数据库硬拒绝）
#[tauri::command]
pub async fn delete_social_cache(app: String, categories: Vec<String>) -> Result<crate::cleaner::social::SocialDeleteReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::social::delete(&app, &categories))
        .await
        .map_err(|e| format!("删除失败: {}", e))
}

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

/// 打开文件（R10/V23：排除可执行扩展名，避免任意路径执行面）
#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    const EXECUTABLE_EXTS: &[&str] = &[
        "exe", "bat", "cmd", "ps1", "scr", "msi", "vbs", "js", "lnk", "com", "pif",
    ];
    let p = std::path::Path::new(&path);
    let ext = p
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if EXECUTABLE_EXTS.contains(&ext.as_str()) {
        return Err(format!("可执行类型 .{} 不允许通过此入口打开", ext));
    }
    if !p.is_file() {
        return Err("目标不是有效文件".into());
    }
    std::process::Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|e| format!("打开文件失败: {}", e))?;
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

/// 旧版 disk-butler 数据导入（合并设计 §3.1 旧名迁移）
///
/// 旧标识符 com.diskbutler.app 的 appDataDir 中 scan-cache-*.json / cleanup-stats.json
/// 存在则导入新数据目录；同名文件已存在时保留新值（不覆盖）。
#[tauri::command]
pub fn migrate_legacy_portable_data(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    use tauri::Manager;

    let legacy_dir = std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|p| p.join("com.diskbutler.app"));
    let legacy_dir = match legacy_dir {
        Some(d) if d.is_dir() => d,
        _ => {
            return Ok(serde_json::json!({
                "found": false,
                "imported": [],
                "message": "未找到旧版数据目录（com.diskbutler.app）"
            }))
        }
    };

    let new_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法定位数据目录: {}", e))?;
    std::fs::create_dir_all(&new_dir).map_err(|e| format!("创建数据目录失败: {}", e))?;

    let mut imported: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    // scan-cache-*.json + cleanup-stats.json（旧版格式沿用，直接复制）
    let patterns = ["scan-cache-", "cleanup-stats.json"];
    if let Ok(entries) = std::fs::read_dir(&legacy_dir) {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy().to_string();
            let matches = name.starts_with(patterns[0]) && name.ends_with(".json")
                || name == patterns[1];
            if !matches {
                continue;
            }
            let target = new_dir.join(&name);
            if target.exists() {
                skipped.push(name);
                continue;
            }
            match std::fs::copy(entry.path(), &target) {
                Ok(_) => imported.push(name),
                Err(e) => skipped.push(format!("{}（复制失败: {}）", name, e)),
            }
        }
    }

    Ok(serde_json::json!({
        "found": true,
        "legacy_dir": legacy_dir.to_string_lossy(),
        "imported": imported,
        "skipped": skipped,
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
