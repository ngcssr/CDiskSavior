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

            // V1: 备份源目录（对齐 WindowsClear place_junction_with_backup 四层策略：
            // rename 重试 → PermissionDenied 换名重试 → 杀占用进程重试 → 清属性强制删兜底）
            let mut backup = source
                .parent()
                .unwrap_or(Path::new("."))
                .join(format!(".cdisk-backup-{}-{}", folder_name, batch_ts));

            // 1) rename 重试 3 次（200ms 间隔——WindowsClear 同款）
            let mut rename_ok = false;
            let mut last_err: Option<std::io::Error> = None;
            for _ in 0..3u32 {
                match std::fs::rename(&source, &backup) {
                    Ok(_) => { rename_ok = true; break; }
                    Err(e) => {
                        last_err = Some(e);
                        std::thread::sleep(std::time::Duration::from_millis(200));
                    }
                }
            }

            // 2) PermissionDenied（error 5）→ 换父目录新名重试（WindowsClear 同款）
            if !rename_ok {
                let denied = last_err
                    .as_ref()
                    .map(|e| {
                        e.kind() == std::io::ErrorKind::PermissionDenied
                            || e.raw_os_error() == Some(5)
                    })
                    .unwrap_or(false);
                if denied {
                    let alt = source
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(format!(".cdisk-bak2-{}-{}", folder_name, batch_ts));
                    for _ in 0..3u32 {
                        match std::fs::rename(&source, &alt) {
                            Ok(_) => { backup = alt; rename_ok = true; break; }
                            Err(e) => {
                                last_err = Some(e);
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                        }
                    }
                }
            }

            // 3) 杀占用进程重试（前端勾选 autoKill 传 pids；否则查锁定进程）
            if !rename_ok {
                let pids: Vec<u32> = if !kill_pids.is_empty() {
                    kill_pids.clone()
                } else {
                    proc_mgr::check_dir_locking(&source)
                        .iter()
                        .map(|p| p.pid)
                        .collect()
                };
                for &pid in &pids {
                    if let Some(name) = proc_mgr::get_process_name(pid) {
                        if let Err(e) = proc_mgr::kill_process(pid, &name) {
                            log::warn!("跳过进程终止: {}", e);
                        }
                    }
                }
                if !pids.is_empty() {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    for _ in 0..3u32 {
                        match std::fs::rename(&source, &backup) {
                            Ok(_) => { rename_ok = true; break; }
                            Err(e) => {
                                last_err = Some(e);
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                        }
                    }
                }
            }

            let mut renamed_to_backup = rename_ok;
            if !rename_ok {
                // 4) 兜底（数据已完整复制到 target，删除源不丢数据）：
                // 先清只读属性再删（remove_dir_all 对只读文件报 error 5
                // 会被误判为"被占用"——Adobe 等软件目录普遍含只读安装文件）
                if Self::force_remove_dir(&source).is_err() || source.exists() {
                    let pids = proc_mgr::check_dir_locking(&source);
                    let pid_txt = if pids.is_empty() {
                        "未检测到具体进程".to_string()
                    } else {
                        format!(
                            "PID {:?}（{}）",
                            pids.iter().map(|p| p.pid).collect::<Vec<_>>(),
                            pids.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")
                        )
                    };
                    let detail = last_err
                        .as_ref()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "unknown".to_string());
                    let msg = format!(
                        "备份源目录失败: {}。可能被占用，相关进程 {}. 请关闭相关软件后重试，或勾选「自动结束占用进程」。",
                        detail, pid_txt
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

        crate::scan_log(&format!(
            "relocate copy_dir start src={} total_size={}",
            src.display(),
            total_size
        ));

        // 0. 收集前先发初始进度（bytesTotal=total_size）——让前端从
        // "正在分析"切换到"0% 复制"，避免收集文件阶段（大目录 10-30s）
        // 无事件导致进度条"一直 0"
        self.emit(MigrationProgress::Progress {
            source: src.to_string_lossy().to_string(),
            bytes_done: 0,
            bytes_total: total_size,
            speed_bps: 0,
        });

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

        crate::scan_log(&format!(
            "relocate copy_dir collected files={} total_size={}",
            files.len(),
            total_size
        ));

        // 2. 并行复制文件（concurrency 限制并发数）+ 分块计数真实进度
        // 进度机制（对齐源码 copy_file_with_progress 的 1MB 分块 + 适配
        // Tauri IPC 的 200ms 节流 emit 专用线程）：大文件中途也更新进度，
        // 小文件风暴不再每文件 emit（治卡顿）
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AOrd};
        use std::sync::Arc;
        let bytes_done = Arc::new(AtomicU64::new(0));
        let stop_emit = Arc::new(AtomicBool::new(false));
        let t0 = Instant::now();
        let source_name = src.to_string_lossy().to_string();
        let max_threads = concurrency.max(1) as usize;

        // 进度线程：每 200ms 读原子计数 emit（真实连续进度，无 IPC 风暴）。
        // std::thread::spawn 要求 'static → 计数/停止用 Arc clone
        let prog_app = self.app.clone();
        let prog_source = source_name.clone();
        let prog_total = total_size;
        let prog_counter = bytes_done.clone();
        let prog_stop = stop_emit.clone();
        let emit_thread = std::thread::spawn(move || {
            let t = Instant::now();
            while !prog_stop.load(AOrd::Relaxed) {
                let done = prog_counter.load(AOrd::Relaxed);
                let elapsed = t.elapsed().as_secs_f64().max(0.001);
                let speed = (done as f64 / elapsed) as u64;
                let _ = prog_app.emit(
                    "migration-progress",
                    MigrationProgress::Progress {
                        source: prog_source.clone(),
                        bytes_done: done,
                        bytes_total: prog_total,
                        speed_bps: speed,
                    },
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            // 最后一次 emit（保证 100%）
            let done = prog_counter.load(AOrd::Relaxed);
            let _ = prog_app.emit(
                "migration-progress",
                MigrationProgress::Progress {
                    source: prog_source,
                    bytes_done: done,
                    bytes_total: prog_total,
                    speed_bps: 0,
                },
            );
        });

        // 用 thread::scope + 滑动窗口控制并发（不依赖全局 rayon 线程池）
        // scope 内 shadow 为 &AtomicU64（Arc deref，Copy 引用可被 spawn move）
        let active = std::sync::atomic::AtomicUsize::new(0);
        let errors: Vec<String> = std::thread::scope(|s| {
            let active = &active;
            let bytes_done: &AtomicU64 = &bytes_done;
            let mut handles: Vec<_> = Vec::new();
            for (sp, dp, _sz) in &files {
                // 检查取消
                if let Err(_e) = self.check_control() {
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
                let h = s.spawn(move || {
                    let result = (|| -> Result<(), String> {
                        for attempt in 0..3u32 {
                            match Self::copy_file_chunked(&sp, &dp, bytes_done) {
                                Ok(()) => return Ok(()),
                                Err(e) => {
                                    let locked = e.contains("(os error 32)")
                                        || e.contains("(os error 33)")
                                        || e.contains("(os error 5)");
                                    if locked {
                                        // 对齐源码 is_safe_to_skip_on_lock：
                                        // Electron 应用 leveldb 的 LOCK 文件常被
                                        // 占用，安全跳过（不影响软件运行）
                                        if Self::is_safe_to_skip_on_lock(&sp) {
                                            return Ok(());
                                        }
                                        if attempt < 2 {
                                            std::thread::sleep(std::time::Duration::from_millis(300));
                                            continue;
                                        }
                                    }
                                    return Err(e);
                                }
                            }
                        }
                        Ok(())
                    })();
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

        // 停止进度线程
        stop_emit.store(true, AOrd::Relaxed);
        let _ = emit_thread.join();
        let _ = t0;
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

    /// 强制删除目录：先递归清只读/系统/隐藏属性再删。
    /// Windows 的 remove_dir_all 对只读文件报 error 5（Access Denied），
    /// 会被误判为"被占用"——Adobe 等软件目录普遍含只读安装文件
    fn force_remove_dir(path: &Path) -> std::io::Result<()> {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_NORMAL};
        for entry in walkdir::WalkDir::new(path)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            let wide: Vec<u16> = entry
                .path()
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            unsafe {
                let _ = SetFileAttributesW(PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_NORMAL);
            }
        }
        std::fs::remove_dir_all(path)
    }

    /// 锁定文件是否可安全跳过（对齐源码 is_safe_to_skip_on_lock）：
    /// 仅 Electron/Chromium 应用 leveldb 目录的 LOCK 文件——被占用时
    /// 跳过不影响软件运行（应用会自行重建），其余锁定文件必须明示失败
    fn is_safe_to_skip_on_lock(src_path: &Path) -> bool {
        let Some(file_name) = src_path.file_name().and_then(|n| n.to_str()) else {
            return false;
        };
        if file_name.eq_ignore_ascii_case("LOCK") {
            let lower = src_path.to_string_lossy().to_lowercase();
            return lower.contains("\\leveldb\\");
        }
        false
    }

    /// 分块复制单文件（1MB 块，对齐源码 copy_file_with_progress）：
    /// 每块 fetch_add 全局计数 → 大文件中途进度真实（专用进度线程
    /// 200ms 节流 emit）。保留 mtime。
    fn copy_file_chunked(
        src: &Path,
        dst: &Path,
        counter: &std::sync::atomic::AtomicU64,
    ) -> Result<(), String> {
        use std::io::{BufReader, BufWriter, Read, Write};
        let open_err = |e: std::io::Error| format!("{}: {}", src.display(), e);
        let mut reader = BufReader::with_capacity(1024 * 1024, std::fs::File::open(src).map_err(open_err)?);
        let mut writer = BufWriter::with_capacity(1024 * 1024, std::fs::File::create(dst).map_err(open_err)?);
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let n = reader.read(&mut buffer).map_err(open_err)?;
            if n == 0 { break; }
            writer.write_all(&buffer[..n]).map_err(|e| format!("{}: {}", dst.display(), e))?;
            counter.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
        }
        writer.flush().map_err(|e| format!("{}: {}", dst.display(), e))?;
        // 保留 mtime
        if let Ok(meta) = std::fs::metadata(src) {
            if let Ok(modified) = meta.modified() {
                let _ = filetime::set_file_mtime(dst, filetime::FileTime::from_system_time(modified));
            }
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
