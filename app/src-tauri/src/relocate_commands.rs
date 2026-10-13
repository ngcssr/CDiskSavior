// CDisk Savior · M4 空间腾挪命令
//
// 设计文档 §4.3 的 12 条 M4 命令（WindowsClear 移植）。
// P2 阶段实现，含 R2/R3/R9 全部安全要求。

use crate::relocate::{
    config::{self, RelocateConfig},
    history::{HistoryManager, MigrationRecord},
    mover::{self, MigrationTask},
    proc_mgr,
    scanner::{self, RelocateTarget},
};
use crate::state;
use std::sync::atomic::AtomicU8;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// 全局迁移任务句柄（同一时刻只允许一个迁移在跑——单实例 + 全局写锁保证）
static CURRENT_TASK: Mutex<Option<TaskHandle>> = Mutex::new(None);

struct TaskHandle {
    control: Arc<AtomicU8>,
}

/// 扫描可迁移目标——MFT 快路径（树聚合，秒级）+ walkdir 回退
/// 扫描进度经 "relocate-scan-progress" 事件推送（真实百分比）
#[tauri::command]
pub async fn scan_relocate_targets(app: AppHandle) -> Result<Vec<RelocateTarget>, String> {
    use tauri::Emitter;
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load_config(&app);
        let emit = |percent: f32, phase: &str| {
            let _ = app.emit(
                "relocate-scan-progress",
                serde_json::json!({ "percent": percent, "phase": phase }),
            );
        };
        emit(0.0, "准备扫描");

        // 收集涉及的盘符树：DRIVE_TREE_CACHE 命中秒回；未命中且提权时
        // 进程内 MFT 单遍（11-25s，与磁盘透视共用缓存体系）
        let mut trees: std::collections::HashMap<char, crate::scan::TreeNode> =
            std::collections::HashMap::new();
        let letters: std::collections::HashSet<char> = cfg
            .scan_sources
            .iter()
            .filter(|s| s.enabled)
            .filter_map(|s| s.path.chars().next())
            .filter(|c| c.is_ascii_alphabetic())
            .collect();
        let total_letters = letters.len().max(1) as f32;
        for (i, letter) in letters.iter().enumerate() {
            let root = format!("{}:\\", letter);
            emit((i as f32 / total_letters) * 80.0, &format!("读取 {} 盘 MFT", letter));
            let tree = crate::commands::try_cached_tree(&root).or_else(|| {
                if crate::lightc_mft::is_elevated() && crate::lightc_mft::mft_available(*letter) {
                    crate::scan_log(&format!("relocate-scan MFT build root={}", root));
                    // MFT 真实进度映射到 0-80% 区间
                    crate::mft_scan::scan_mft(&root, |_, _, pct, phase| {
                        emit(pct * 0.8, &format!("扫描 {} 盘 MFT: {}", letter, phase));
                    })
                    .ok()
                    .and_then(|t| serde_json::to_value(&t).ok())
                    .and_then(|v| serde_json::from_value::<crate::scan::TreeNode>(v).ok())
                } else {
                    None
                }
            });
            if let Some(t) = tree {
                trees.insert(*letter, t);
            }
        }

        emit(85.0, "聚合大文件夹");
        // MFT 快路径
        let mut targets = scanner::scan_large_folders_from_tree(&cfg.scan_sources, &trees, None);

        // 回退：无树的源走 walkdir 慢路径
        let missing: Vec<&crate::relocate::scanner::ScanSource> = cfg
            .scan_sources
            .iter()
            .filter(|s| {
                s.enabled
                    && !trees.contains_key(&s.path.chars().next().unwrap_or('\0'))
            })
            .collect();
        if !missing.is_empty() {
            emit(88.0, "慢速遍历（MFT 不可用）");
            let refs: Vec<crate::relocate::scanner::ScanSource> =
                missing.iter().map(|s| (*s).clone()).collect();
            let extra = scanner::scan_large_folders(&refs, None);
            targets.extend(extra);
        }

        emit(100.0, "扫描完成");
        targets.sort_by(|a, b| b.size.cmp(&a.size));
        targets
    })
    .await
    .map_err(|e| format!("扫描失败: {}", e))
}

/// 检查占用进程（返回给前端展示 + 用户确认）
#[tauri::command]
pub fn list_locking_processes(path: String) -> Result<Vec<proc_mgr::LockingProcess>, String> {
    Ok(proc_mgr::check_dir_locking(std::path::Path::new(&path)))
}

/// 安全终止进程（V5：黑名单校验 + 用户已确认）
#[tauri::command]
pub fn kill_locking_process(pid: u32) -> Result<(), String> {
    if let Some(name) = proc_mgr::get_process_name(pid) {
        proc_mgr::kill_process(pid, &name)
    } else {
        Err(format!("无法获取进程 {} 的名称", pid))
    }
}

/// 启动迁移（后台任务 + 事件推送进度）
#[tauri::command]
pub async fn start_migration(
    app: AppHandle,
    sources: Vec<String>,
    target_root: String,
    kill_pids: Vec<u32>,
    concurrency: u32,
) -> Result<serde_json::Value, String> {
    // R9: target_root 校验
    match crate::relocate::validate::validate_target_root(std::path::Path::new(&target_root)) {
        crate::relocate::validate::TargetRootCheck::Ok => {}
        crate::relocate::validate::TargetRootCheck::Rejected(msg) => return Err(msg),
    }

    let task = MigrationTask::new(app.clone());
    let control = task.control_handle();

    // 存入全局句柄
    {
        let mut current = CURRENT_TASK.lock().unwrap_or_else(|p| p.into_inner());
        if current.is_some() {
            return Err("已有迁移任务在运行".into());
        }
        *current = Some(TaskHandle {
            control: control.clone(),
        });
    }

    let app2 = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let history = HistoryManager::new(&app2);
        let history = history.ok_or("无法初始化迁移历史")?;
        task.run(sources, target_root, kill_pids, concurrency, &history)
    })
    .await
    .map_err(|e| format!("迁移任务失败: {}", e))?;

    // 清除全局句柄
    {
        let mut current = CURRENT_TASK.lock().unwrap_or_else(|p| p.into_inner());
        *current = None;
    }

    match result {
        Ok((freed, records)) => Ok(serde_json::json!({
            "freed": freed,
            "records": records,
        })),
        Err(e) => Err(e),
    }
}

/// 暂停迁移
#[tauri::command]
pub fn pause_migration() -> Result<(), String> {
    let current = CURRENT_TASK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(handle) = current.as_ref() {
        mover::pause(&handle.control);
        Ok(())
    } else {
        Err("没有正在运行的迁移任务".into())
    }
}

/// 继续迁移
#[tauri::command]
pub fn resume_migration() -> Result<(), String> {
    let current = CURRENT_TASK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(handle) = current.as_ref() {
        mover::resume(&handle.control);
        Ok(())
    } else {
        Err("没有正在运行的迁移任务".into())
    }
}

/// 取消迁移（V12：触发 .partial 清理）
#[tauri::command]
pub fn cancel_migration() -> Result<(), String> {
    let current = CURRENT_TASK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(handle) = current.as_ref() {
        mover::cancel(&handle.control);
        Ok(())
    } else {
        Err("没有正在运行的迁移任务".into())
    }
}

/// 回滚迁移（恢复源目录）。force=true 时源位置的非链接目录会被
/// 迁移数据覆盖（软件迁移后重建了源目录的场景，用户显式确认）
#[tauri::command]
pub async fn restore_migration(app: AppHandle, record_id: String, force: Option<bool>) -> Result<(), String> {
    // 读记录（只读，不需要锁）
    let history = HistoryManager::new(&app).ok_or("无法初始化迁移历史")?;
    let records = history.load();
    let record = records
        .iter()
        .find(|r| r.id == record_id)
        .ok_or(format!("找不到迁移记录: {}", record_id))?
        .clone();

    let source = record.source_path.clone();
    let target = record.target_path.clone();
    let app_clone = app.clone();
    let id_clone = record_id.clone();
    let force_clone = force.unwrap_or(false);

    // 整个写操作放进 spawn_blocking（锁在其中获取，不跨 await 边界 → Send 安全）
    tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let _lock = state::acquire_write_lock();

        let source = std::path::Path::new(&source);
        let target = std::path::Path::new(&target);

        if !target.exists() {
            return Err(format!(
                "迁移目标 {} 已不存在，无法恢复。建议直接移除失效快捷方式。",
                target.display()
            ));
        }

        // 源位置是 junction → 先删除 junction
        if crate::relocate::junction::is_junction(source) {
            crate::relocate::junction::remove_junction(source)
                .map_err(|e| format!("解除 Junction 失败: {}", e))?;
        } else if source.exists() && source.is_dir() {
            if !force_clone {
                return Err(format!(
                    "RESTORE_CONFLICT:还原位置 {} 已包含非链接目录（可能是软件迁移后重建）。强制回滚将删除该目录并用迁移数据还原。",
                    source.display()
                ));
            }
            // 强制回滚：删除重建的源目录，用迁移数据还原
            std::fs::remove_dir_all(source)
                .map_err(|e| format!("清理已重建的源目录失败: {}", e))?;
        }

        // 跨卷 rename 可能失败 → 尝试同卷 rename
        if std::fs::rename(target, source).is_ok() {
            let history = HistoryManager::new(&app_clone).ok_or("无法初始化迁移历史")?;
            history.remove(&id_clone);
            return Ok(());
        }

        // 跨卷 → 复制（V13：拼接，不用 with_extension）+ 真实进度事件
        let partial = source
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(format!(
                "{}.partial_restore",
                source.file_name().unwrap_or_default().to_string_lossy()
            ));
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(&partial).map_err(|e| format!("创建临时还原目录失败: {}", e))?;

        // 回滚进度（真实）：先算总量，复制中分块计数，200ms 节流 emit
        {
            use tauri::Emitter;
            use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AOrd};
            use std::sync::Arc;
            let total = crate::relocate::scanner::get_dir_size(target);
            let _ = app_clone.emit(
                "relocate-restore-progress",
                serde_json::json!({ "id": id_clone, "bytesDone": 0, "bytesTotal": total, "speedBps": 0 }),
            );
            let counter = Arc::new(AtomicU64::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let prog_app = app_clone.clone();
            let prog_id = id_clone.clone();
            let prog_counter = counter.clone();
            let prog_stop = stop.clone();
            let emit_thread = std::thread::spawn(move || {
                let t = std::time::Instant::now();
                while !prog_stop.load(AOrd::Relaxed) {
                    let done = prog_counter.load(AOrd::Relaxed);
                    let elapsed = t.elapsed().as_secs_f64().max(0.001);
                    let speed = (done as f64 / elapsed) as u64;
                    let _ = prog_app.emit(
                        "relocate-restore-progress",
                        serde_json::json!({ "id": prog_id, "bytesDone": done, "bytesTotal": total, "speedBps": speed }),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let copy_result = copy_dir_recursive(target, &partial, &counter);
            stop.store(true, AOrd::Relaxed);
            let _ = emit_thread.join();
            copy_result?;
        }
        std::fs::rename(&partial, source)
            .map_err(|e| format!("整理被还原目录失败: {}", e))?;
        let _ = std::fs::remove_dir_all(target);

        let history = HistoryManager::new(&app_clone).ok_or("无法初始化迁移历史")?;
        history.remove(&id_clone);
        Ok(())
    })
    .await
    .map_err(|e| format!("还原任务失败: {}", e))?
}

/// 列出迁移历史
#[tauri::command]
pub fn list_migration_history(app: AppHandle) -> Result<Vec<MigrationRecord>, String> {
    let history = HistoryManager::new(&app).ok_or("无法初始化迁移历史")?;
    Ok(history.load())
}

/// 删除迁移记录（不恢复数据，仅删除记录）
#[tauri::command]
pub fn remove_migration_record(app: AppHandle, record_id: String) -> Result<(), String> {
    let history = HistoryManager::new(&app).ok_or("无法初始化迁移历史")?;
    history.remove(&record_id);
    Ok(())
}

/// 读取迁移配置
#[tauri::command]
pub fn get_relocate_config(app: AppHandle) -> Result<RelocateConfig, String> {
    // 预置源强制启用已统一在 config::load_config（扫描/读取/迁移全路径生效）
    Ok(config::load_config(&app))
}

/// 查询目标盘剩余空间（用户可用字节数）
#[tauri::command]
pub fn get_target_drive_free(path: String) -> Result<u64, String> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    crate::scan_log(&format!("get_target_drive_free called path={}", path));
    // GetDiskFreeSpaceExW 只接受盘符根路径（如 "G:\"），子目录路径
    // 会失败——从 path 提取盘符根
    let root = if path.len() >= 2 && path.as_bytes()[1] == b':' {
        let drive = path.as_bytes()[0] as char;
        if drive.is_ascii_alphabetic() {
            format!("{}:\\", drive)
        } else {
            path.clone()
        }
    } else {
        path.clone()
    };
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut avail: u64 = 0;
    let mut total: u64 = 0;
    let mut free: u64 = 0;
    let r = unsafe {
        GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut avail), Some(&mut total), Some(&mut free))
    };
    match r {
        Ok(_) => {
            crate::scan_log(&format!("get_target_drive_free ok root={} avail={}", root, avail));
            Ok(avail)
        }
        Err(e) => {
            crate::scan_log(&format!("get_target_drive_free ERR root={} {:?}", root, e));
            Err(format!("查询磁盘空间失败（root={}）: {:?}", root, e))
        }
    }
}

/// 保存迁移配置（含 R9 校验）
#[tauri::command]
pub fn save_relocate_config(app: AppHandle, config_value: serde_json::Value) -> Result<(), String> {
    let cfg: RelocateConfig = serde_json::from_value(config_value)
        .map_err(|e| format!("配置格式错误: {}", e))?;
    config::save_config(&app, &cfg)
}

/// 递归复制目录（用于回滚的跨卷复制）——并发版（8 线程滑动窗口）
/// + 分块计数真实进度（counter 由调用方传入，进度线程节流 emit）
fn copy_dir_recursive(
    src: &std::path::Path,
    dst: &std::path::Path,
    counter: &std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), String> {
    use std::io::{BufReader, BufWriter, Read, Write};
    use std::sync::atomic::{AtomicUsize, Ordering as AOrd};
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {}", e))?;

    // 1. 收集文件 + 建目录树
    let mut files: Vec<(std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    for entry in walkdir::WalkDir::new(src)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        let sp = entry.path();
        if crate::relocate::junction::is_junction(sp) {
            continue; // 回滚不含链接子项
        }
        let rel = sp.strip_prefix(src).unwrap_or(sp);
        let dp = dst.join(rel);
        if entry.file_type().is_dir() {
            let _ = std::fs::create_dir_all(&dp);
        } else if entry.file_type().is_file() {
            files.push((sp.to_path_buf(), dp));
        }
    }

    // 2. 并行复制（滑动窗口 8 并发，分块计数）
    let active = AtomicUsize::new(0);
    let errors: Vec<String> = std::thread::scope(|s| {
        let active = &active;
        let mut handles: Vec<_> = Vec::new();
        for (sp, dp) in &files {
            while active.load(AOrd::Acquire) >= 8 {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            active.fetch_add(1, AOrd::AcqRel);
            let sp = sp.clone();
            let dp = dp.clone();
            let h = s.spawn(move || {
                // 分块复制（1MB 块，每块计数）
                let r = (|| -> Result<(), String> {
                    let open_err = |e: std::io::Error| format!("{}: {}", sp.display(), e);
                    let mut reader = BufReader::with_capacity(1024 * 1024, std::fs::File::open(&sp).map_err(open_err)?);
                    let mut writer = BufWriter::with_capacity(1024 * 1024, std::fs::File::create(&dp).map_err(open_err)?);
                    let mut buffer = vec![0u8; 1024 * 1024];
                    loop {
                        let n = reader.read(&mut buffer).map_err(open_err)?;
                        if n == 0 { break; }
                        writer.write_all(&buffer[..n]).map_err(|e| format!("{}: {}", dp.display(), e))?;
                        counter.fetch_add(n as u64, AOrd::Relaxed);
                    }
                    writer.flush().map_err(|e| format!("{}: {}", dp.display(), e))?;
                    if let Ok(meta) = std::fs::metadata(&sp) {
                        if let Ok(modified) = meta.modified() {
                            let _ = filetime::set_file_mtime(&dp, filetime::FileTime::from_system_time(modified));
                        }
                    }
                    Ok(())
                })();
                active.fetch_sub(1, AOrd::AcqRel);
                r
            });
            handles.push(h);
        }
        let mut errs: Vec<String> = Vec::new();
        for h in handles {
            if let Ok(Err(e)) = h.join() {
                errs.push(e);
            }
        }
        errs
    });

    if !errors.is_empty() {
        return Err(format!("复制文件失败: {}", errors.join("; ")));
    }
    Ok(())
}
