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

/// 扫描可迁移目标——async：遍历 AppData 可能较慢
#[tauri::command]
pub async fn scan_relocate_targets(app: AppHandle) -> Result<Vec<RelocateTarget>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load_config(&app);
        scanner::scan_large_folders(&cfg.scan_sources, None)
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

/// 回滚迁移（恢复源目录）
#[tauri::command]
pub async fn restore_migration(app: AppHandle, record_id: String) -> Result<(), String> {
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
            return Err(format!(
                "还原位置 {} 已包含非链接目录，为保护数据取消还原。请先手动清理。",
                source.display()
            ));
        }

        // 跨卷 rename 可能失败 → 尝试同卷 rename
        if std::fs::rename(target, source).is_ok() {
            let history = HistoryManager::new(&app_clone).ok_or("无法初始化迁移历史")?;
            history.remove(&id_clone);
            return Ok(());
        }

        // 跨卷 → 复制（V13：拼接，不用 with_extension）
        let partial = source
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join(format!(
                "{}.partial_restore",
                source.file_name().unwrap_or_default().to_string_lossy()
            ));
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(&partial).map_err(|e| format!("创建临时还原目录失败: {}", e))?;

        copy_dir_recursive(target, &partial)?;
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

/// 递归复制目录（用于回滚的跨卷复制）
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败: {}", e))?;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取目录失败: {}", e))? {
        let entry = entry.map_err(|e| format!("读取条目失败: {}", e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path).map_err(|e| format!("复制文件失败: {}", e))?;
        }
    }
    Ok(())
}
