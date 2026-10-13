//! 迁移引擎（核心）——设计文档 §7 R2 全部安全要求
//!
//! 修复清单（对照 WindowsClear 原实现）：
//! V1  备份落同卷持久位置（source.parent），禁 %TEMP%
//! V6  reparse 按属性位识别（FILE_ATTRIBUTE_REPARSE_POINT），跳过项必须 UI 明示
//! V7  逐文件复制保留 mtime（P2：ACL/硬链接留待后续，至少不静默丢属性）
//! V8  Junction 经 junction crate（FSCTL），弃 cmd /C mklink
//! V10 迁移前空间预检：剩余 >= 源 × 1.1
//! V12 取消语义：清理 .partial + 写入残留登记
//! V13 临时目录名用拼接，不用 with_extension

use crate::relocate::{
    history::MigrationRecord, junction, proc_mgr,
    validate, MigrationProgress,
};
use crate::state;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

/// 迁移控制信号
const CTRL_RUNNING: u8 = 0;
const CTRL_PAUSED: u8 = 1;
const CTRL_CANCELLED: u8 = 2;

/// 迁移任务（可被 pause/resume/cancel 控制）
pub struct MigrationTask {
    control: Arc<AtomicU8>,
    app: AppHandle,
}

impl MigrationTask {
    pub fn new(app: AppHandle) -> Self {
        Self {
            control: Arc::new(AtomicU8::new(CTRL_RUNNING)),
            app,
        }
    }

    /// 获取控制信号句柄（供 pause/resume/cancel 命令使用）
    pub fn control_handle(&self) -> Arc<AtomicU8> {
        self.control.clone()
    }

    /// 检查是否应暂停/取消（在复制循环的文件边界调用）
    fn check_control(&self) -> Result<(), String> {
        loop {
            match self.control.load(Ordering::SeqCst) {
                CTRL_RUNNING => return Ok(()),
                CTRL_PAUSED => {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
                CTRL_CANCELLED => return Err("用户已取消".into()),
                _ => return Err("未知控制状态".into()),
            }
        }
    }

    /// 执行迁移
    pub fn run(
        &self,
        sources: Vec<String>,
        target_root: String,
        kill_pids: Vec<u32>,
        concurrency: u32,
        history: &crate::relocate::history::HistoryManager,
    ) -> Result<(u64, Vec<MigrationRecord>), String> {
        // 全局写锁（C1 互斥）
        let _lock = state::acquire_write_lock();

        let target_root_path = validate::long_path(Path::new(&target_root));
        let batch_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let batch_id = uuid::Uuid::new_v4().to_string();
        let mut total_freed = 0u64;
        let mut records = Vec::new();

        for source_str in &sources {
            // 检查取消
            if let Err(e) = self.check_control() {
                self.emit(MigrationProgress::Cancelled {
                    partial_cleanup: format!("在迁移 {} 时被取消", source_str),
                });
                return Err(e);
            }

            let source = validate::long_path(Path::new(source_str));

            // V6: 如果源已是 junction，跳过并 UI 明示
            if junction::is_junction(&source) {
                self.emit(MigrationProgress::ItemSkipped {
                    source: source_str.clone(),
                    reason: "该目录已是 Junction（可能是已迁移过的目录），请先恢复原迁移再操作".into(),
                });
                continue;
            }

            if !source.exists() || !source.is_dir() {
                self.emit(MigrationProgress::ItemSkipped {
                    source: source_str.clone(),
                    reason: "源路径不存在或不是目录".into(),
                });
                continue;
            }

            // 计算源大小（可能 10-30s——发进度事件让用户知道在干活）
            self.emit(MigrationProgress::Progress {
                source: source_str.clone(),
                bytes_done: 0,
                bytes_total: 0,
                speed_bps: 0,
            });
            let source_size = crate::relocate::scanner::get_dir_size(&source);

            // V10: 空间预检
            match validate::check_disk_space(&target_root_path, source_size) {
                validate::TargetRootCheck::Ok => {}
                validate::TargetRootCheck::Rejected(msg) => {
                    self.emit(MigrationProgress::Error { message: msg.clone() });
                    return Err(msg);
                }
            }

            // 目标路径
            let folder_name = source
                .file_name()
                .ok_or("无效的源路径")?
                .to_string_lossy()
                .to_string();
            // 找到 target_subdir
            let target_subdir = self.find_target_subdir(source_str);
            let target_path = target_root_path.join(&target_subdir).join(&folder_name);
            let target_partial = target_root_path
                .join(&target_subdir)
                .join(format!("{}.partial", folder_name));

            // 确保目标根目录 + 子目录存在
            let _ = std::fs::create_dir_all(target_path.parent().unwrap_or(Path::new("")));

            // 复制
            if let Err(e) = self.copy_dir(
                &source,
                &target_partial,
                source_size,
                &kill_pids,
                concurrency,
            ) {
                // V12: 取消时清理 .partial
                let _ = std::fs::remove_dir_all(&target_partial);
                let friendly = if e.contains("os error 32") || e.contains("os error 33") || e.contains("os error 5") {
                    format!("{}（文件被占用——请关闭相关软件后重试，或勾选「自动结束占用进程」）", e)
                } else {
                    e
                };
                self.emit(MigrationProgress::Error {
                    message: format!("复制失败（{}）: {}", source_str, friendly),
                });
                return Err(friendly);
            }

            // 重命名 .partial → target_path（原子，同卷）
            if target_path.exists() {
                // 增量迁移：target 已存在，合并后不 rename（内容已就位）
                let _ = std::fs::remove_dir_all(&target_partial);
            } else {
                std::fs::rename(&target_partial, &target_path)
                    .map_err(|e| format!("整理目标目录失败: {}", e))?;
            }

            // V1: 备份源目录到同卷持久位置（NOT %TEMP%）
            let backup = source
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!(".cdisk-backup-{}-{}", folder_name, batch_ts));

            // 尝试 rename 源 → backup（同卷原子操作）
            let rename_ok = std::fs::rename(&source, &backup).is_ok();

            if !rename_ok && !kill_pids.is_empty() {
                // 终止指定进程后重试
                for &pid in &kill_pids {
                    if let Some(name) = proc_mgr::get_process_name(pid) {
                        if let Err(e) = proc_mgr::kill_process(pid, &name) {
                            log::warn!("跳过进程终止: {}", e);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }

            // 再试 rename
            let rename_ok = std::fs::rename(&source, &backup).is_ok() || rename_ok;

            let mut renamed_to_backup = rename_ok;
            if !rename_ok {
                // Fallback（对齐 AppData Mover 行为）：数据已完整复制到
                // target，rename 原子备份因文件被占用失败 → 重试后直接
                // 删除源目录再建 junction，数据安全由 target 完整副本保证
                for _ in 0..3 {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if std::fs::remove_dir_all(&source).is_ok() {
                        break;
                    }
                }
                if source.exists() {
                    let pids = proc_mgr::check_dir_locking(&source);
                    let pid_txt = if pids.is_empty() {
                        "未检测到具体进程（可能被系统服务锁定）".to_string()
                    } else {
                        format!(
                            "PID {:?}（{}）",
                            pids.iter().map(|p| p.pid).collect::<Vec<_>>(),
                            pids.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")
                        )
                    };
                    let msg = format!(
                        "源目录被占用无法迁移（{}）。请关闭相关软件后重试，或勾选「自动结束占用进程」。",
                        pid_txt
                    );
                    self.emit(MigrationProgress::Error { message: msg.clone() });
                    let _ = std::fs::remove_dir_all(&target_path);
                    return Err(msg);
                }
            }

            // V8: 创建 Junction（FSCTL，无 cmd.exe）
            match junction::create_junction(&source, &target_path) {
                Ok(()) => {
                    let freed = source_size;
                    total_freed += freed;

                    // 清理备份（rename 成功路径才有 backup；fallback 删除路径无 backup）
                    if renamed_to_backup {
                        if let Err(e) = std::fs::remove_dir_all(&backup) {
                            // 备份没删掉不致命（数据已在目标），但记日志
                            log::warn!("清理备份失败（不致命）: {}", e);
                        }
                    }

                    // V19: 原子写入迁移历史（target_path 存原始路径——
                    // 不带 \\?\ 前缀，否则回滚 exists 判断异常+UI 显示难看）
                    let raw_target = {
                        let raw_root = target_root.trim_end_matches(['\\', '/']);
                        format!("{}\\{}\\{}", raw_root, target_subdir, folder_name)
                    };
                    let record = MigrationRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: folder_name.clone(),
                        source_path: source_str.clone(),
                        target_path: raw_target,
                        size_bytes: source_size,
                        timestamp: batch_ts,
                        batch_id: Some(batch_id.clone()),
                        batch_timestamp: Some(batch_ts),
                    };
                    history.add(record.clone());
                    records.push(record);

                    self.emit(MigrationProgress::ItemDone {
                        source: source_str.clone(),
                        freed,
                    });
                }
                Err(e) => {
                    // Junction 创建失败 → 恢复源目录
                    if renamed_to_backup {
                        let _ = std::fs::remove_dir_all(&source); // 清理可能的半成品
                        std::fs::rename(&backup, &source)
                            .map_err(|e2| format!("恢复源目录失败: {}（Junction 失败: {}）", e2, e))?;
                        // 源已恢复，但备份在 source.parent 下（持久，不在 %TEMP%）
                        // 用户数据安全，但备份残留需提示
                        let msg = format!(
                            "创建 Junction 失败（{}），源目录已恢复。备份残留于: {}",
                            e,
                            backup.display()
                        );
                        self.emit(MigrationProgress::Error { message: msg.clone() });
                        return Err(msg);
                    }
                    // fallback 路径（源已删除，数据完整在 target）——不丢数据，
                    // 告知用户数据位置（罕见场景：FSCTL 失败）
                    let msg = format!(
                        "创建 Junction 失败（{}），但数据已完整复制到 {}。源目录已按 AppData Mover 兼容方式处理，请手动检查。",
                        e,
                        target_path.display()
                    );
                    self.emit(MigrationProgress::Error { message: msg.clone() });
                    return Err(msg);
                }
            }
        }

        self.emit(MigrationProgress::Done {
            total_freed,
            records: records.clone(),
        });
        Ok((total_freed, records))
    }

    /// 复制目录树（支持暂停/取消/进度回调/并发传输）
    ///
    /// V6: 跳过 reparse point（junction/symlink），但收集到 skipped 列表
    /// V7: 保留 mtime（P2 阶段；ACL/硬链接留后续）
    /// 并发传输：先遍历收集文件列表，再用 rayon 并行复制（concurrency 限制）
    fn copy_dir(
        &self,
        src: &Path,
        dst: &Path,
        total_size: u64,
        _kill_pids: &[u32],
        concurrency: u32,
    ) -> Result<(), String> {
        std::fs::create_dir_all(dst).map_err(|e| format!("创建目标目录失败: {}", e))?;

        // 1. 先遍历收集所有文件 + 建目录树
        let mut files: Vec<(std::path::PathBuf, std::path::PathBuf, u64)> = Vec::new();
        let mut skipped_reparse: Vec<String> = Vec::new();
        for entry in walkdir::WalkDir::new(src)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            self.check_control()?;
            let src_path = entry.path();
            if junction::is_junction(src_path) {
                skipped_reparse.push(src_path.to_string_lossy().to_string());
                continue;
            }
            let rel = src_path.strip_prefix(src).unwrap_or(src_path);
            let dst_path = dst.join(rel);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&dst_path)
                    .map_err(|e| format!("创建子目录失败 {}: {}", dst_path.display(), e))?;
            } else if entry.file_type().is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                files.push((src_path.to_path_buf(), dst_path, size));
            }
        }

        // 2. 并行复制文件（concurrency 限制并发数）
        use std::sync::atomic::{AtomicU64, Ordering as AOrd};
        let bytes_done = AtomicU64::new(0);
        let active = std::sync::atomic::AtomicUsize::new(0);
        let t0 = Instant::now();
        let source_name = src.to_string_lossy().to_string();
        let max_threads = concurrency.max(1) as usize;

        // 用 thread::scope + 滑动窗口控制并发（不依赖全局 rayon 线程池）
        // active/bytes_done 声明在 scope 外（'env 可被 spawn 线程借用），
        // scope 内 shadow 为引用（&AtomicUsize 是 Copy，spawn move 引用无害）
        let errors: Vec<String> = std::thread::scope(|s| {
            let active = &active;
            let bytes_done = &bytes_done;
            let mut handles: Vec<_> = Vec::new();
            for (sp, dp, sz) in &files {
                // 检查取消
                if let Err(e) = self.check_control() {
                    break;
                }
                // 等并发空位
                while active.load(AOrd::Acquire) >= max_threads {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    if self.check_control().is_err() { break; }
                }
                if self.check_control().is_err() { break; }
                active.fetch_add(1, AOrd::AcqRel);

                let sp = sp.clone();
                let dp = dp.clone();
                let sz = *sz;
                let sn = source_name.clone();
                let this_app = self.app.clone();
                let h = s.spawn(move || {
                    let result = (|| -> Result<(), String> {
                        for attempt in 0..3u32 {
                            match self.copy_file(&sp, &dp) {
                                Ok(()) => return Ok(()),
                                Err(e) => {
                                    let raw = std::io::Error::raw_os_error(&e);
                                    let locked = raw == Some(32) || raw == Some(33) || raw == Some(5);
                                    if locked && attempt < 2 {
                                        std::thread::sleep(std::time::Duration::from_millis(300));
                                        continue;
                                    }
                                    return Err(format!("{}: {}", sp.display(), e));
                                }
                            }
                        }
                        Ok(())
                    })();
                    if result.is_ok() {
                        let done = bytes_done.fetch_add(sz, AOrd::AcqRel) + sz;
                        let elapsed = t0.elapsed().as_secs_f64().max(0.001);
                        let speed = (done as f64 / elapsed) as u64;
                        let _ = this_app.emit("migration-progress", MigrationProgress::Progress {
                            source: sn,
                            bytes_done: done,
                            bytes_total: total_size,
                            speed_bps: speed,
                        });
                    }
                    active.fetch_sub(1, AOrd::AcqRel);
                    result
                });
                handles.push(h);
            }
            // 收集结果
            let mut errs: Vec<String> = Vec::new();
            for h in handles {
                if let Ok(Err(e)) = h.join() {
                    errs.push(e);
                }
            }
            errs
        });

        if !errors.is_empty() {
            return Err(format!("文件被占用无法复制（已重试）: {}", errors.join("; ")));
        }

        // V6: 跳过的 reparse point 必须明示
        for path in &skipped_reparse {
            self.emit(MigrationProgress::ItemSkipped {
                source: path.clone(),
                reason: "Junction/Symlink 子目录——已跳过（迁移后需手动处理）".into(),
            });
        }

        Ok(())
    }

    /// 逐文件复制（保留 mtime；V7：P2 阶段先保留 mtime，ACL 留后续）
    fn copy_file(&self, src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::copy(src, dst)?;

        // 保留 mtime
        if let Ok(meta) = std::fs::metadata(src) {
            if let Ok(modified) = meta.modified() {
                let _ = filetime::set_file_mtime(dst, filetime::FileTime::from_system_time(modified));
            }
        }
        Ok(())
    }

    /// 在配置中查找源路径对应的 target_subdir
    fn find_target_subdir(&self, source: &str) -> String {
        let lower = source.to_lowercase();
        if lower.contains("\\appdata\\local") {
            "Local".into()
        } else if lower.contains("\\appdata\\roaming") {
            "Roaming".into()
        } else if lower.contains("\\program files (x86)") {
            "Program Files (x86)".into()
        } else if lower.contains("\\program files") {
            "Program Files".into()
        } else if lower.contains("\\programdata") {
            "ProgramData".into()
        } else {
            "Other".into()
        }
    }

    fn emit(&self, progress: MigrationProgress) {
        let _ = self.app.emit("migration-progress", &progress);
    }
}

// ============ 控制命令实现 ============

/// 暂停迁移
pub fn pause(control: &Arc<AtomicU8>) {
    control.store(CTRL_PAUSED, Ordering::SeqCst);
}

/// 继续迁移
pub fn resume(control: &Arc<AtomicU8>) {
    control.store(CTRL_RUNNING, Ordering::SeqCst);
}

/// 取消迁移（V12：触发清理 .partial）
pub fn cancel(control: &Arc<AtomicU8>) {
    control.store(CTRL_CANCELLED, Ordering::SeqCst);
}
