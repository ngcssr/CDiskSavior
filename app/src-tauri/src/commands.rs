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

/// 磁盘健康（物理盘 SMART 健康状态；映射表统一命令名 get_disk_health）
#[tauri::command]
pub async fn get_disk_health() -> Result<Vec<crate::health::DiskHealth>, String> {
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

/// 盘符全树缓存（WizTree 式体验：一次扫描，会话内下钻即时返回）。
/// 服务树含深度/宽度剪枝（深度 4、每层 30），缓存未命中的深层路径回源扫描。
static DRIVE_TREE_CACHE: std::sync::Mutex<Option<(String, crate::scan::TreeNode)>> =
    std::sync::Mutex::new(None);

fn normalize_drive_key(p: &str) -> String {
    p.trim_end_matches(['\\', '/']).to_ascii_uppercase()
}

/// 在缓存树中按路径查找子树（大小写不敏感，容忍尾部反斜杠差异）
fn find_subtree(node: &crate::scan::TreeNode, target: &str) -> Option<crate::scan::TreeNode> {
    if normalize_drive_key(&node.path) == normalize_drive_key(target) {
        return Some(node.clone());
    }
    for c in &node.children {
        if let Some(found) = find_subtree(c, target) {
            return Some(found);
        }
    }
    None
}

#[tauri::command]
pub async fn scan_drive(
    app: tauri::AppHandle,
    root: String,
    force_rescan: Option<bool>,
) -> Result<crate::scan::TreeNode, String> {
    use tauri::Emitter;

    // 缓存命中（非强制刷新）：下钻零耗时（WizTree 式体验）
    if !force_rescan.unwrap_or(false) {
        if let Ok(guard) = DRIVE_TREE_CACHE.lock() {
            if let Some((_, tree)) = guard.as_ref() {
                if let Some(sub) = find_subtree(tree, &root) {
                    return Ok(sub);
                }
            }
        }
    }

    tauri::async_runtime::spawn_blocking(move || {
        // 快路径优先级：① MFT 服务（SYSTEM 权限，管道）→ ② 进程内 MFT 引擎
        // （提权 + NTFS，与服务同代码 mft_scan::scan_mft，rayon 并行）
        // → ③ jwalk 慢速遍历（非提权/非 NTFS 兜底）
        // 便携版无服务侧车时 ② 生效——与 LightC 便携版同架构（进程内 MFT，无需服务）
        let scanned_root = root.clone();
        let emit_progress = |files: u64, bytes: u64, percent: f32, phase: &str| {
            let _ = app.emit("disk-growth:progress", serde_json::json!({
                "filesScanned": files, "bytesScanned": bytes,
                "percent": percent, "phase": phase, "engine": "mft",
            }));
        };
        let tree = if crate::svc_client::service_installed() {
            crate::svc_client::scan_via_service(&root, emit_progress).ok()
        } else {
            None
        };
        let tree = match tree {
            Some(t) => t,
            None => {
                // ② 进程内 MFT 引擎（提权 + 盘符根 + NTFS 时可用）
                let is_drive_root = {
                    let t = root.trim_end_matches(['\\', '/']);
                    t.len() == 2 && t.as_bytes()[1] == b':'
                };
                let letter = root.chars().next().filter(|c| c.is_ascii_alphabetic());
                let mft_inproc = is_drive_root
                    && letter.is_some()
                    && crate::lightc_mft::mft_available(letter.unwrap());
                if mft_inproc {
                    match crate::mft_scan::scan_mft(&root, emit_progress) {
                        Ok(t) => {
                            // mft_scan::TreeNode（enum 字段）→ scan::TreeNode（string 字段）：
                            // serde JSON 中转（与服务管道路径的序列化协议一致）
                            match serde_json::to_value(&t)
                                .and_then(|v| serde_json::from_value::<crate::scan::TreeNode>(v))
                            {
                                Ok(tree) => tree,
                                Err(_) => crate::scan::scan_directory(&root)?,
                            }
                        }
                        Err(_) => crate::scan::scan_directory(&root)?,
                    }
                } else {
                    crate::scan::scan_directory(&root)?
                }
            }
        };
        // 扫的是盘符根时刷新缓存（后续下钻走缓存）
        let is_drive_root = {
            let t = root.trim_end_matches(['\\', '/']);
            t.len() == 2 && t.as_bytes()[1] == b':'
        };
        if is_drive_root {
            if let Ok(mut guard) = DRIVE_TREE_CACHE.lock() {
                *guard = Some((normalize_drive_key(&scanned_root), tree.clone()));
            }
        }
        Ok(tree)
    })
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

/// 常规垃圾扫描（可选分类名过滤；与深度扫描共用 18 类资产，分类级进度事件）
#[tauri::command]
pub async fn scan_junk_files(app: tauri::AppHandle, categories: Option<Vec<String>>) -> Result<crate::cleaner::deep_junk::DeepScanResult, String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        crate::cleaner::deep_junk::scan_filtered_with_progress(categories, &|cat, (done, total)| {
            let _ = app.emit("deep-junk:progress", serde_json::json!({
                "category": cat, "done": done, "total": total,
            }));
        })
    })
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

/// 全盘空间变化分析（无基线自动创建；有基线返回对比明细；文件数进度事件）
#[tauri::command]
pub async fn scan_disk_growth(app: tauri::AppHandle, drive: String, max_change_entries: Option<usize>) -> Result<crate::growth::DiskScanAndAnalyzeResponse, String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        crate::growth::scan_disk_growth_with_progress(&drive, max_change_entries, &|files| {
            let _ = app.emit("disk-growth:progress", serde_json::json!({ "filesScanned": files }));
        })
    })
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

/// 大文件扫描（阈值过滤 + Top 200 + 可取消 + 进度事件）
#[tauri::command]
pub async fn scan_large_files(app: tauri::AppHandle, root: String, min_size: Option<u64>) -> Result<crate::bigfiles::LargeFileReport, String> {
    use tauri::Emitter;
    let min = min_size.unwrap_or(100 * 1024 * 1024); // 默认 100MB
    tauri::async_runtime::spawn_blocking(move || {
        crate::bigfiles::scan_large_files_with_progress(&root, min, &|scanned| {
            let _ = app.emit("bigfiles:progress", serde_json::json!({ "scannedFiles": scanned }));
        })
    })
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
pub fn load_scan_cache(root: String) -> Option<crate::scan::ScanCache> {
    crate::scan::load_scan_cache(&root)
}

// ============ M1 系统信息（P3 补齐） ============

/// 磁盘信息（卷容量/文件系统；与 get_drives 同数据源，映射表独立命令名）
#[tauri::command]
pub fn get_disk_info() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "drives": crate::scan::get_drives() }))
}

/// 系统信息（OS/CPU/内存概览；注册表 + GlobalMemoryStatusEx，无 sysinfo 依赖）
#[tauri::command]
pub fn get_system_info() -> Result<serde_json::Value, String> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    // OS 版本（Win11 的 ProductName 仍为 "Windows 10"，按 build ≥22000 修正）
    let mut os = "Windows".to_string();
    if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
    {
        let product: String = k.get_value("ProductName").unwrap_or_else(|_| "Windows".into());
        let disp: String = k.get_value("DisplayVersion").unwrap_or_default();
        let build: String = k.get_value("CurrentBuildNumber").unwrap_or_default();
        let product = if build.parse::<u32>().unwrap_or(0) >= 22000 {
            product.replace("Windows 10", "Windows 11")
        } else {
            product
        };
        os = format!("{} {} (build {})", product, disp, build);
    }

    // CPU 名称
    let cpu = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")
        .ok()
        .and_then(|k| k.get_value::<String, _>("ProcessorNameString").ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    // 内存（GlobalMemoryStatusEx）
    let mut mem_total = 0u64;
    let mut mem_avail = 0u64;
    unsafe {
        let mut stat: windows::Win32::System::SystemInformation::MEMORYSTATUSEX =
            std::mem::zeroed();
        stat.dwLength = std::mem::size_of_val(&stat) as u32;
        if windows::Win32::System::SystemInformation::GlobalMemoryStatusEx(&mut stat).is_ok() {
            mem_total = stat.ullTotalPhys;
            mem_avail = stat.ullAvailPhys;
        }
    }

    Ok(serde_json::json!({
        "os": os,
        "cpu": cpu,
        "arch": std::env::consts::ARCH,
        "memoryTotal": mem_total,
        "memoryAvailable": mem_avail,
    }))
}

// ============ M3 清理动作记录（P3 补齐） ============

/// 记录清理动作到 data_dir/logs/cleanup-actions.log（JSON 行）
#[tauri::command]
pub fn record_cleanup_action(entries: Vec<serde_json::Value>) -> Result<String, String> {
    use std::io::Write;
    let log_dir = crate::appdata::data_dir().join("logs");
    std::fs::create_dir_all(&log_dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    let file = log_dir.join("cleanup-actions.log");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
        .map_err(|e| format!("打开日志失败: {}", e))?;
    let ts = chrono::Local::now().to_rfc3339();
    for entry in &entries {
        let line = serde_json::json!({ "timestamp": ts, "entry": entry });
        writeln!(f, "{}", line).map_err(|e| format!("写入日志失败: {}", e))?;
    }
    Ok(file.to_string_lossy().to_string())
}

// ============ M2 热点取消（P3 补齐） ============

#[tauri::command]
pub fn cancel_hotspot_scan() {
    crate::growth::cancel_hotspot_scan();
}

/// 直接扫描指定路径（映射表独立命令名；与 scan_dir 同实现）
#[tauri::command]
pub async fn scan_path_direct(path: String) -> Result<crate::scan::TreeNode, String> {
    tauri::async_runtime::spawn_blocking(move || crate::scan::scan_directory(&path))
        .await
        .map_err(|e| format!("扫描失败: {}", e))?
}

// ============ M8 设置与工具（P3 补齐） ============

/// 文件夹选择对话框
#[tauri::command]
pub async fn pick_folder_dialog(app: tauri::AppHandle, title: Option<String>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let result = app
        .dialog()
        .file()
        .set_title(title.as_deref().unwrap_or("选择目录"))
        .blocking_pick_folder();
    Ok(result.map(|p| p.to_string()))
}

/// 文件磁盘占用大小（GetCompressedFileSizeW，压缩/稀疏感知；失败回退逻辑大小）
#[tauri::command]
pub fn get_physical_size(path: String) -> Result<u64, String> {
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut high = 0u32;
    let low = unsafe {
        windows::Win32::Storage::FileSystem::GetCompressedFileSizeW(
            windows::core::PCWSTR(wide.as_ptr()), Some(&mut high))
    };
    if low == u32::MAX {
        let err = unsafe { windows::Win32::Foundation::GetLastError() };
        if err.0 != 0 {
            // 回退：逻辑大小
            return std::fs::metadata(&path)
                .map(|m| m.len())
                .map_err(|e| format!("无法读取文件大小 {}: {}", path, e));
        }
    }
    Ok(((high as u64) << 32) | low as u64)
}

/// 打开日志目录
#[tauri::command]
pub fn open_logs_folder() -> Result<(), String> {
    let dir = crate::appdata::data_dir().join("logs");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建日志目录失败: {}", e))?;
    std::process::Command::new("explorer")
        .arg(&dir)
        .spawn()
        .map_err(|e| format!("打开日志目录失败: {}", e))?;
    Ok(())
}

/// 设置自定义数据目录（迁移现有数据，源保留）
#[tauri::command]
pub fn set_data_directory(path: String) -> Result<String, String> {
    crate::appdata::set_data_dir(std::path::Path::new(&path))
}

/// 发行渠道（installer / portable）
#[tauri::command]
pub fn get_distribution_channel() -> crate::appdata::DistributionChannel {
    crate::appdata::current_channel()
}

/// 存储位置信息
#[tauri::command]
pub fn get_storage_location_info() -> crate::appdata::StorageLocationInfo {
    crate::appdata::storage_location_info()
}

/// 存储写入诊断
#[tauri::command]
pub fn get_storage_write_diagnostic() -> crate::appdata::StorageWriteDiagnostic {
    crate::appdata::storage_write_diagnostic()
}

/// 可清理本地数据清单
#[tauri::command]
pub fn list_clearable_data_items() -> Vec<crate::appdata::ClearableDataItem> {
    crate::appdata::list_clearable_items()
}

/// 清空全部可清理本地数据
#[tauri::command]
pub fn clear_local_data() -> crate::appdata::ClearLocalDataResult {
    crate::appdata::clear_local_data_items(&[])
}

/// 按 id 清理指定本地数据
#[tauri::command]
pub fn clear_selected_local_data(keys: Vec<String>) -> crate::appdata::ClearLocalDataResult {
    crate::appdata::clear_local_data_items(&keys)
}

/// 路径前缀判断是否需要管理员权限
#[tauri::command]
pub fn check_admin_for_path(path: String) -> bool {
    let lower = path.to_lowercase().replace('/', "\\");
    let admin_prefixes = [
        "\\windows\\",
        "\\program files\\",
        "\\program files (x86)\\",
        "\\programdata\\",
        "$recycle.bin",
    ];
    admin_prefixes.iter().any(|p| lower.contains(p))
        || lower.starts_with("c:\\windows")
        || lower.starts_with("c:\\program files")
        || lower.starts_with("c:\\programdata")
}

/// 打开系统启动项管理（Win10+ 设置页）
#[tauri::command]
pub fn open_startup_manager() -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg("ms-settings:startupapps")
        .spawn()
        .map_err(|e| format!("打开启动项管理失败: {}", e))?;
    Ok(())
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
pub async fn scan_deep_junk_files(app: tauri::AppHandle) -> Result<crate::cleaner::deep_junk::DeepScanResult, String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        crate::cleaner::deep_junk::scan_filtered_with_progress(None, &|cat, (done, total)| {
            let _ = app.emit("deep-junk:progress", serde_json::json!({
                "category": cat, "done": done, "total": total,
            }));
        })
    })
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

// ============ M3 批量删除系（P2 补齐，R1 四层护栏） ============

/// 删除文件（移入回收站；允许集合 = 深度垃圾 18 类展开根，当场重扫）
#[tauri::command]
pub async fn delete_files(paths: Vec<String>) -> Result<crate::cleaner::bulk_delete::DeletePathsReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::bulk_delete::delete_files(&paths))
        .await
        .map_err(|e| format!("删除任务失败: {}", e))
}

/// 增强删除（永久删除 + 占用重试 3 次）
#[tauri::command]
pub async fn enhanced_delete_files(paths: Vec<String>) -> Result<crate::cleaner::bulk_delete::DeletePathsReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::bulk_delete::enhanced_delete_files(&paths))
        .await
        .map_err(|e| format!("删除任务失败: {}", e))
}

/// 卸载残留文件夹移入回收站（允许集合 = 残留扫描当场清单）
#[tauri::command]
pub async fn delete_leftover_folders(paths: Vec<String>) -> Result<crate::cleaner::bulk_delete::DeletePathsReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::bulk_delete::delete_leftover_folders(&paths))
        .await
        .map_err(|e| format!("删除任务失败: {}", e))
}

/// 卸载残留文件夹永久删除（R1 唯一原语）
#[tauri::command]
pub async fn delete_leftovers_permanent(paths: Vec<String>) -> Result<crate::cleaner::bulk_delete::DeletePathsReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::bulk_delete::delete_leftovers_permanent(&paths))
        .await
        .map_err(|e| format!("删除任务失败: {}", e))
}

/// 清空深度垃圾类别目录内容（保留目录；精确命中校验）
#[tauri::command]
pub async fn cleanup_directory_contents(path: String) -> Result<crate::cleaner::bulk_delete::DeletePathsReport, String> {
    tauri::async_runtime::spawn_blocking(move || crate::cleaner::bulk_delete::cleanup_directory_contents(&path))
        .await
        .map_err(|e| format!("清空任务失败: {}", e))
}

// ============ Q4 软件体检（feature_bloatware 启用） ============

/// 软件体检扫描（客观行为：自启/常驻/占用，不定性）
#[tauri::command]
pub async fn scan_bloatware(app: tauri::AppHandle, include_all: Option<bool>) -> Result<crate::bloatware::BloatwareScan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let ignored = crate::bloatware::load_ignored(&app);
        crate::bloatware::scan(&ignored, include_all.unwrap_or(false))
    })
    .await
    .map_err(|e| format!("软件体检失败: {}", e))
}

/// 方法A一键卸载（提权：停服务/杀进程/官方卸载器静默）
#[tauri::command]
pub async fn uninstall_software(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::bloatware::uninstall(&id))
        .await
        .map_err(|e| format!("卸载任务失败: {}", e))?
}

/// 卸载后残留全景扫描（安装目录 + 同名数据目录 + HKCU 注册表键，严格白名单）
#[tauri::command]
pub async fn scan_residue(
    name: String,
    publisher: String,
    install_location: String,
) -> Result<crate::bloatware::ResidueScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::bloatware::scan_residue(&name, &publisher, &install_location)
    })
    .await
    .map_err(|e| format!("残留扫描失败: {}", e))
}

/// 清理卸载残留（允许集合同场重扫，集合外强制跳过）
#[tauri::command]
pub async fn clean_residue(
    name: String,
    publisher: String,
    install_location: String,
    dirs: Vec<String>,
    reg_keys: Vec<String>,
) -> Result<crate::bloatware::ResidueReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::bloatware::clean_residue(&name, &publisher, &install_location, dirs, reg_keys)
    })
    .await
    .map_err(|e| format!("残留清理失败: {}", e))
}

/// "不再提醒"持久化
#[tauri::command]
pub async fn bloatware_set_ignored(app: tauri::AppHandle, key: String, ignored: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::bloatware::set_ignored(&app, key, ignored))
        .await
        .map_err(|e| format!("更新失败: {}", e))
}

/// 停止软件后台（提权；后端按 id 重读安装目录，不信任前端路径）
#[tauri::command]
pub async fn stop_software(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::bloatware::stop_software(&id))
        .await
        .map_err(|e| format!("停止失败: {}", e))?
}

/// 提权操作是否已开始（UAC 已授权，前端轮询用）
#[tauri::command]
pub fn op_started() -> bool {
    crate::bloatware::op_started()
}

/// 强力卸载预览（红色二次确认清单）
#[tauri::command]
pub async fn bloatware_force_preview(id: String) -> Result<crate::bloatware::ForcePlan, String> {
    tauri::async_runtime::spawn_blocking(move || Ok(crate::bloatware::force_preview(&id)))
        .await
        .map_err(|e| format!("预览失败: {}", e))?
}

/// 强力卸载（提权：删服务/任务/进程/目录/注册表项）
#[tauri::command]
pub async fn force_uninstall_software(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || crate::bloatware::force_uninstall(&id))
        .await
        .map_err(|e| format!("强力卸载失败: {}", e))?
}

/// 前台打开官方卸载程序（安全软件等有自我保护的场景）
#[tauri::command]
pub fn open_official_uninstaller(id: String) -> Result<(), String> {
    crate::bloatware::open_official_uninstaller(&id)
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

/// 规则采集报告（disk-butler collector 移植：目录名+大小+缓存模式+软件清单，
/// 脱敏无文件名/内容/用户名，报告写桌面）
#[tauri::command]
pub async fn collect_rules(include_drives: bool) -> Result<crate::collector::CollectResult, String> {
    tauri::async_runtime::spawn_blocking(move || crate::collector::collect(include_drives))
        .await
        .map_err(|e| format!("采集失败: {}", e))?
}

// ============ M8 设置与工具（P7 实现） ============

#[tauri::command]
pub fn scan_service_available() -> bool {
    crate::svc_client::service_installed()
}

/// 一键修复扫描服务：提权运行 cdisk-savior-svc.exe install（弹一次 UAC）
#[tauri::command]
pub fn repair_scan_service() -> Result<(), String> {
    crate::svc_client::repair_service()
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
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token = windows::Win32::Foundation::HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut ret_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let _ = windows::Win32::Foundation::CloseHandle(token);
        result.is_ok() && elevation.TokenIsElevated != 0
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
