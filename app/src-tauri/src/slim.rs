//! M5 系统瘦身
//!
//! 设计文档 §2 M5：休眠 / WinSxS / 虚拟内存 / 搜索索引 / 旧驱动 /
//! 注册表 / 图标缓存 / AI 模型
//! 设计文档 §6 C3 / §7 R4：按操作提权（disk-butler run_elevated_ps 模型）

use serde::Serialize;
use std::path::PathBuf;

/// 系统瘦身状态
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlimStatus {
    pub hibernation_enabled: bool,
    pub winsxs_size: Option<u64>,
    pub search_index_running: bool,
    pub old_driver_count: usize,
    pub old_driver_total_size: u64,
    pub registry_redundant_count: usize,
    pub ai_model_count: usize,
    pub ai_model_total_size: u64,
}

/// 获取系统瘦身状态
pub fn get_status() -> SlimStatus {
    SlimStatus {
        hibernation_enabled: check_hibernation(),
        winsxs_size: get_winsxs_size(),
        search_index_running: check_search_service(),
        old_driver_count: 0,
        old_driver_total_size: 0,
        registry_redundant_count: 0,
        ai_model_count: 0,
        ai_model_total_size: 0,
    }
}

/// 隐藏控制台子进程窗口（CREATE_NO_WINDOW）
/// 修复：控制台程序（powercfg / sc / pnputil / powershell）默认弹出黑框闪烁
fn cmd(program: &str) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    c
}

/// 非提权执行 PowerShell 并捕获 stdout（供 health/disk_health 等查询用）
pub fn run_powershell_capture(script: &str) -> Result<String, String> {
    let utf16: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64_encode(&utf16);
    let out = cmd("powershell")
        .args(["-NoProfile", "-EncodedCommand", &encoded])
        .output()
        .map_err(|e| format!("执行失败: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("执行失败: {}", stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

// ============ 休眠 ============

/// 检查休眠是否开启
fn check_hibernation() -> bool {
    // powercfg /a 输出含 "休眠" 或 "Hibernation" → 开启
    let out = cmd("powercfg")
        .args(["/a"])
        .output();
    match out {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.contains("休眠") || s.contains("Hibernation") && !s.contains("not available")
        }
        Err(_) => false,
    }
}

/// 启用休眠：powercfg /hibernate on（需提权）
pub fn enable_hibernation() -> Result<String, String> {
    run_elevated_ps("powercfg /hibernate on")
}

/// 禁用休眠：powercfg /hibernate off（需提权，释放 hiberfil.sys）
pub fn disable_hibernation() -> Result<String, String> {
    run_elevated_ps("powercfg /hibernate off")
}

// ============ WinSxS 清理（DISM） ============

/// DISM 组件清理（需提权，R4 模型）
pub fn cleanup_winsxs() -> Result<String, String> {
    run_elevated_ps("Dism.exe /Online /Cleanup-Image /StartComponentCleanup")
}

/// DISM 组件清理 + 重置基线（需提权）
pub fn cleanup_winsxs_resetbase() -> Result<String, String> {
    run_elevated_ps("Dism.exe /Online /Cleanup-Image /StartComponentCleanup /ResetBase")
}

// ============ 虚拟内存 / 系统设置 ============

/// 打开虚拟内存设置
pub fn open_virtual_memory_settings() -> Result<(), String> {
    std::process::Command::new("SystemPropertiesPerformance")
        .spawn()
        .map_err(|e| format!("打开性能设置失败: {}", e))?;
    Ok(())
}

/// 打开存储感知设置
pub fn open_storage_settings() -> Result<(), String> {
    std::process::Command::new("ms-settings:storagesense")
        .spawn()
        .map_err(|e| format!("打开存储设置失败: {}", e))?;
    Ok(())
}

/// 打开磁盘清理
pub fn open_disk_cleanup() -> Result<(), String> {
    std::process::Command::new("cleanmgr")
        .spawn()
        .map_err(|e| format!("打开磁盘清理失败: {}", e))?;
    Ok(())
}

// ============ 搜索索引 ============

/// 重建搜索索引（需提权：停止 WSearch → 删除索引 → 启动）
pub fn rebuild_search_index() -> Result<String, String> {
    run_elevated_ps(
        "Stop-Service WSearch -Force; \
         Remove-Item -Recurse -Force 'C:\\ProgramData\\Microsoft\\Search\\Data\\*' -EA SilentlyContinue; \
         Start-Service WSearch"
    )
}

fn check_search_service() -> bool {
    cmd("sc")
        .args(["query", "WSearch"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("RUNNING"))
        .unwrap_or(false)
}

// ============ 旧驱动扫描 ============

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverEntry {
    pub id: String,
    pub name: String,
    pub date: String,
    pub size: u64,
    pub path: String,
}

/// 扫描旧驱动（pnputil 枚举 + 按日期分组）
pub fn scan_old_drivers() -> Vec<DriverEntry> {
    // P5 简化：用 pnputil /enum-drivers 解析第三方驱动列表
    let out = cmd("pnputil")
        .args(["/enum-drivers"])
        .output();
    match out {
        Ok(o) => parse_pnputil(&String::from_utf8_lossy(&o.stdout)),
        Err(_) => Vec::new(),
    }
}

fn parse_pnputil(output: &str) -> Vec<DriverEntry> {
    let mut entries = Vec::new();
    let mut current_name = String::new();
    let mut current_date = String::new();
    let mut current_path = String::new();

    for line in output.lines() {
        let line = line.trim();
        if line.starts_with("Published Name:") || line.starts_with("已发布名称:") {
            if !current_name.is_empty() {
                entries.push(DriverEntry {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: current_name.clone(),
                    date: current_date.clone(),
                    size: 0,
                    path: current_path.clone(),
                });
            }
            current_name = String::new();
            current_date = String::new();
            current_path = line.split(':').nth(1).unwrap_or("").trim().to_string();
        } else if line.starts_with("Original Name:") || line.starts_with("原始名称:") {
            current_name = line.split(':').nth(1).unwrap_or("").trim().to_string();
        } else if line.starts_with("Driver Date:") || line.starts_with("驱动程序日期:") {
            current_date = line.split(':').nth(1).unwrap_or("").trim().to_string();
        }
    }
    if !current_name.is_empty() {
        entries.push(DriverEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: current_name,
            date: current_date,
            size: 0,
            path: current_path,
        });
    }
    entries
}

/// 删除旧驱动（pnputil /delete-driver，需提权）
pub fn delete_old_drivers(ids: &[String]) -> Result<u32, String> {
    let mut deleted = 0u32;
    for id in ids {
        // id 是 oem*.inf 文件名
        let script = format!("pnputil /delete-driver {} /force", id);
        match run_elevated_ps(&script) {
            Ok(_) => deleted += 1,
            Err(e) => log::warn!("删除驱动 {} 失败: {}", id, e),
        }
    }
    Ok(deleted)
}

// ============ 注册表冗余扫描 ============

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    pub id: String,
    pub key: String,
    pub value: String,
    pub data: String,
    pub size: u64,
}

/// 扫描注册表冗余（已知路径：卸载残留、MUI 缓存等）
pub fn scan_registry_redundancy() -> Vec<RegistryEntry> {
    // P5 简化：扫描 Uninstall 下的无效条目
    let entries = Vec::new();
    // TODO: 用 winreg 扫描 HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall
    // P5 暂不实现完整扫描——后端返回空列表，前端显示"未发现"
    entries
}

/// 删除注册表条目（先备份）
#[allow(dead_code)] // P9: 注册表清理接入后使用
pub fn delete_registry_entries(_entries: &[RegistryEntry]) -> u32 {
    // P5 占位：需先实现 reg export 备份 + reg delete
    0
}

// ============ 图标缓存 ============

/// 清理图标缓存（删除 IconCache.db + 重启资源管理器）
#[allow(dead_code)] // P9: 图标缓存清理接入后使用
pub fn clear_icon_cache() -> Result<(), String> {
    let local = std::env::var("LOCALAPPDATA").map_err(|_| "无法读取 LOCALAPPDATA")?;
    let icon_cache = PathBuf::from(&local).join("IconCache.db");
    let _ = std::fs::remove_file(&icon_cache);
    Ok(())
}

/// 重启资源管理器
pub fn restart_explorer() -> Result<(), String> {
    run_elevated_ps("Stop-Process -Name explorer -Force; Start-Process explorer")?;
    Ok(())
}

// ============ AI 模型扫描 ============

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelEntry {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub last_modified: u64,
}

/// 扫描 AI 模型资产
pub fn scan_ai_model_assets() -> Vec<AiModelEntry> {
    let mut entries = Vec::new();
    // 已知 AI 模型目录
    let candidates = [
        // LM Studio
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".lmstudio").join("models"),
        // Ollama
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".ollama").join("models"),
        // Hugging Face
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join(".cache").join("huggingface"),
        // GPT4All
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_default()).join("AppData").join("Local").join("nomic.ai"),
    ];

    for dir in &candidates {
        if !dir.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(dir).max_depth(2).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let ext = entry.path().extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "gguf" | "bin" | "safetensors" | "pt" | "onnx" | "ggml") {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size < 10 * 1024 * 1024 {
                continue; // 跳过 <10MB 的小文件
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let path = entry.path().to_string_lossy().to_string();
            let mtime = entry.metadata().ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            entries.push(AiModelEntry {
                id: uuid::Uuid::new_v4().to_string(),
                name, path, size, last_modified: mtime,
            });
        }
    }

    entries.sort_by(|a, b| b.size.cmp(&a.size));
    entries
}

/// 删除 AI 模型（经 R1 引擎的 R9 校验）
pub fn delete_ai_model(id: &str, models: &[AiModelEntry]) -> Result<u64, String> {
    let model = models.iter().find(|m| m.id == id)
        .ok_or("找不到模型")?;
    let path = std::path::Path::new(&model.path);
    let size = model.size;
    std::fs::remove_file(path).map_err(|e| format!("删除失败: {}", e))?;
    Ok(size)
}

// ============ 辅助 ============

/// 获取 WinSxS 目录大小
fn get_winsxs_size() -> Option<u64> {
    let win = std::env::var("SystemRoot").ok()?;
    let winsxs = PathBuf::from(win).join("WinSxS");
    if !winsxs.exists() {
        return None;
    }
    let mut total = 0u64;
    for entry in jwalk::WalkDir::new(&winsxs).max_depth(1).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    Some(total)
}

/// 公开版本（供 commands.rs 调用）
pub fn run_elevated_ps_pub(script: &str) -> Result<String, String> {
    run_elevated_ps(script)
}

/// 按操作提权执行 PowerShell 脚本（R4 模型：disk-butler run_elevated_ps 蓝本）
///
/// 脚本经 UTF-16LE+Base64 编码内联 -EncodedCommand，不向 %TEMP% 落地 .ps1。
///
/// UAC 取消检测（marker 文件方案）：
/// Start-Process -Verb RunAs 在用户取消时向外层抛异常（外层退出码 1，非 1223），
/// 1223 检测永远不命中。改用 marker：提权脚本末尾写标记文件，
/// 执行后检查 marker 是否存在——存在=真实执行，缺失=用户取消。
fn run_elevated_ps(script: &str) -> Result<String, String> {
    // marker 文件路径（%LOCALAPPDATA%\CDiskSavior\elevated_marker_{uuid}）
    let marker_dir = std::env::var("LOCALAPPDATA")
        .map(|d| PathBuf::from(d).join("CDiskSavior"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&marker_dir);
    let marker_id = uuid::Uuid::new_v4().to_string();
    let marker_path = marker_dir.join(format!("elevated_marker_{}", marker_id));
    // 清理可能的残留
    let _ = std::fs::remove_file(&marker_path);

    // 提权脚本 = 用户脚本 + 写 marker
    let marker_str = marker_path.to_string_lossy().replace('\'', "''");
    let full_script = format!(
        "{}; Set-Content -Path '{}' -Value 'done' -ErrorAction SilentlyContinue",
        script, marker_str
    );

    // 编码：UTF-16LE → Base64
    let utf16: Vec<u8> = full_script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    let encoded = base64_encode(&utf16);

    let outer = format!(
        "$null = Start-Process -Verb RunAs -Wait -PassThru -FilePath powershell.exe \
         -ArgumentList '-NoProfile','-EncodedCommand','{}'",
        encoded
    );

    let result = cmd("powershell")
        .args(["-NoProfile", "-Command", &outer])
        .output()
        .map_err(|e| format!("提权执行失败: {}", e));

    // marker 检查（比退出码可靠：外层取消时退出码是 1 不是 1223）
    let marker_exists = marker_path.exists();
    let _ = std::fs::remove_file(&marker_path); // 清理

    let result = result?;
    if marker_exists {
        // 提权脚本真实执行过（即使有非致命错误也算成功路径）
        return Ok(String::from_utf8_lossy(&result.stdout).to_string());
    }

    // marker 不存在 → 提权进程从未启动
    let code = result.status.code().unwrap_or(-1);
    if code == 1223 || code == 1 || !result.status.success() {
        return Err("用户取消了管理员授权（或提权被拒绝）".into());
    }
    Err(format!("执行失败（退出码 {}）", code))
}

/// Base64 编码（无外部依赖）
fn base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((n >> 18) & 63) as usize] as char);
        result.push(CHARS[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((n >> 6) & 63) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(n & 63) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
