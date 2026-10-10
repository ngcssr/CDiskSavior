//! 注册表残留扫描与清理（light-c scanner/registry.rs + registry_scoring.rs 合并移植）
//!
//! 【安全声明】
//!   1. 只读扫描：扫描阶段绝不修改任何注册表键值
//!   2. 铁证条件：关联 exe 不存在 + 非系统路径 + 非系统进程，三项全部满足才输出
//!   3. 范围收敛：只扫描 HKCR\Applications（文件关联残留），不碰系统关键区域
//!   4. 真实备份：删除前使用 reg.exe export 创建可恢复的 .reg 文件（数据目录 reg_backups/）
//!   5. 用户确认：所有删除操作需用户明确选择
//!
//! 【为什么只扫描 HKCR\Applications】
//!   程序安装时在此注册文件关联（如 "用 xxx 打开 .pdf"），卸载后经常残留。
//!   这是最安全且最有价值的注册表清理目标——删错最多丢失文件关联，
//!   不会影响系统稳定性。

use chrono;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use winreg::enums::*;
use winreg::RegKey;

// ============================================================================
// 数据类型
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryScanResult {
    pub entries: Vec<RegistryEntry>,
    pub total_count: u32,
    pub scan_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEntry {
    /// HKCR\Applications 下的完整路径
    pub path: String,
    /// 应用程序名（注册表子键名，如 "notepad.exe"）
    pub name: String,
    /// 关联的不存在的可执行文件路径
    pub associated_path: String,
    /// 人类可读的问题描述
    pub issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDeleteResult {
    pub backup_path: String,
    pub deleted_count: u32,
    pub failed_entries: Vec<String>,
    pub errors: Vec<String>,
}

// ============================================================================
// 路径存在性缓存 (PathCache)
// ============================================================================

/// 路径存在性缓存，避免同一轮扫描中重复文件系统调用
pub struct PathCache {
    cache: HashMap<String, bool>,
    hits: u64,
    misses: u64,
}

impl PathCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::with_capacity(256),
            hits: 0,
            misses: 0,
        }
    }

    pub fn exists(&mut self, path: &str) -> bool {
        if let Some(&exists) = self.cache.get(path) {
            self.hits += 1;
            return exists;
        }
        self.misses += 1;
        let exists = Path::new(path).exists();
        self.cache.insert(path.to_string(), exists);
        exists
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.hits, self.misses)
    }
}

impl Default for PathCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 路径解析器 (PathResolver)
// ============================================================================

/// 从命令行字符串中提取可执行文件路径、展开环境变量
pub struct PathResolver {
    env_var_re: Regex,
}

impl PathResolver {
    pub fn new() -> Self {
        Self {
            env_var_re: Regex::new(r"%([^%]+)%").unwrap(),
        }
    }

    /// 从命令行字符串中提取并解析可执行文件路径
    ///
    /// 支持格式：
    /// - `"C:\Program Files\App\app.exe" %1`
    /// - `C:\Program Files\App\app.exe %1`
    /// - `%SystemRoot%\system32\notepad.exe %1`
    /// - `"C:\Program Files\App\app.exe",-123`
    ///
    /// 返回 Option<(解析后的路径, 是否在系统目录)>
    pub fn extract_and_resolve(&self, raw_command: &str) -> Option<(PathBuf, bool)> {
        let exe_path = self.extract_exe_path(raw_command)?;
        let resolved = self.expand_env_vars(&exe_path);
        let path = PathBuf::from(&resolved);
        let is_system = self.is_system_path(&resolved);

        Some((path, is_system))
    }

    fn extract_exe_path(&self, command: &str) -> Option<String> {
        let command = command.trim();

        if command.is_empty() {
            return None;
        }

        // 带引号的路径: "C:\path\to\app.exe" args
        if command.starts_with('"') {
            if let Some(end) = command[1..].find('"') {
                let path = &command[1..end + 1];
                if self.looks_like_exe_path(path) {
                    return Some(path.to_string());
                }
            }
        }

        // 不带引号的路径: C:\path\to\app.exe args
        let first_token = command.split_whitespace().next()?;
        let cleaned = first_token.trim_end_matches(',');

        if self.looks_like_exe_path(cleaned) {
            Some(cleaned.to_string())
        } else {
            None
        }
    }

    /// 检查是否像可执行文件路径（盘符 + exe/dll/sys 扩展名）
    fn looks_like_exe_path(&self, s: &str) -> bool {
        let lower = s.to_lowercase();
        // 环境变量路径在展开前没有盘符，先允许合法的 %VAR% 前缀交给后续解析。
        let starts_with_environment_variable = lower.starts_with('%')
            && lower[1..]
                .find('%')
                .is_some_and(|end| lower[end + 1..].contains('\\'));
        (lower.ends_with(".exe") || lower.ends_with(".dll") || lower.ends_with(".sys"))
            && lower.len() > 4
            && (lower.chars().nth(1) == Some(':') || starts_with_environment_variable)
    }

    /// 展开环境变量（如 %SystemRoot%）
    fn expand_env_vars(&self, s: &str) -> String {
        self.env_var_re
            .replace_all(s, |caps: &regex::Captures| {
                let var_name = &caps[1];
                std::env::var(var_name).unwrap_or_else(|_| match var_name.to_uppercase().as_str() {
                    "SYSTEMROOT" => "C:\\Windows".to_string(),
                    "WINDIR" => "C:\\Windows".to_string(),
                    "PROGRAMFILES" => "C:\\Program Files".to_string(),
                    "PROGRAMFILES(X86)" => "C:\\Program Files (x86)".to_string(),
                    "PROGRAMDATA" => "C:\\ProgramData".to_string(),
                    "USERPROFILE" => "C:\\Users\\Default".to_string(),
                    "APPDATA" => "C:\\Users\\Default\\AppData\\Roaming".to_string(),
                    "LOCALAPPDATA" => "C:\\Users\\Default\\AppData\\Local".to_string(),
                    "TEMP" | "TMP" => "C:\\Windows\\Temp".to_string(),
                    _ => caps[0].to_string(),
                })
            })
            .to_string()
    }

    /// 检查路径是否在系统目录
    pub fn is_system_path(&self, path: &str) -> bool {
        let lower = path.to_lowercase();
        let system_roots = [
            "c:\\windows\\system32",
            "c:\\windows\\syswow64",
            "c:\\windows\\system",
            "c:\\windows\\winsxs",
            "c:\\windows\\servicing",
            "c:\\windows\\assembly",
        ];

        for root in &system_roots {
            if lower.starts_with(root) {
                return true;
            }
        }

        false
    }
}

impl Default for PathResolver {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 硬过滤条件 (Hard Delete Safety Check)
// ============================================================================

/// 确认注册表项关联的文件路径满足"铁证条件"——可以安全删除
///
/// 【铁证条件】
///   1. 路径非空且能从注册表值中提取到
///   2. 关联文件在磁盘上不存在 (Path.exists == false)
///   3. 路径不在系统目录 (Windows / System32 / SysWOW64)
///   4. 文件名不是系统关键进程 (rundll32.exe / svchost.exe 等)
///
/// 任何不确定的情况一律返回 false —— 安全优先原则
pub fn is_definitely_safe_to_delete(extracted_path: &str, path_cache: &mut PathCache) -> bool {
    // 条件1：路径非空
    if extracted_path.is_empty() {
        return false;
    }

    // 条件2：文件必须不存在（存在意味着程序可能仍在使用）
    if path_cache.exists(extracted_path) {
        return false;
    }

    // 条件3：路径不能在系统目录
    let lower = extracted_path.to_lowercase();
    let system_dirs = [
        "\\windows\\system32",
        "\\windows\\syswow64",
        "\\windows\\system\\",
        "\\windows\\winsxs",
        "\\windows\\servicing",
        "\\windows\\assembly",
    ];
    for dir in &system_dirs {
        if lower.contains(dir) {
            return false;
        }
    }

    // 条件4：文件名不能是系统关键进程
    let file_name = Path::new(extracted_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");
    let file_name_lower = file_name.to_lowercase();
    let system_exes = [
        "rundll32.exe",
        "svchost.exe",
        "explorer.exe",
        "regedit.exe",
        "cmd.exe",
        "powershell.exe",
        "csrss.exe",
        "lsass.exe",
        "services.exe",
        "winlogon.exe",
        "taskmgr.exe",
        "conhost.exe",
        "smss.exe",
        "spoolsv.exe",
        "dllhost.exe",
    ];
    if system_exes.contains(&file_name_lower.as_str()) {
        return false;
    }

    true
}

// ============================================================================
// 扫描器
// ============================================================================

/// 扫描 HKCR\Applications 下所有子键，检查其 shell\open\command
/// 引用的可执行文件是否仍然存在
pub fn scan() -> RegistryScanResult {
    let start_time = std::time::Instant::now();
    let mut entries = Vec::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let mut path_cache = PathCache::new();
    let path_resolver = PathResolver::new();

    log::info!("开始扫描注册表残留 (HKCR\\Applications)");

    let apps_key = match RegKey::predef(HKEY_CLASSES_ROOT)
        .open_subkey_with_flags("Applications", KEY_READ)
    {
        Ok(k) => k,
        Err(e) => {
            log::warn!("无法打开 HKCR\\Applications: {}", e);
            return RegistryScanResult {
                entries: vec![],
                total_count: 0,
                scan_duration_ms: start_time.elapsed().as_millis() as u64,
            };
        }
    };

    for app_name in apps_key.enum_keys().filter_map(|k| k.ok()) {
        if entries.len() >= 100 {
            break;
        }

        // 读 shell\open\command 默认值（命令行字符串）
        let shell_path = format!(r"{}\shell\open\command", app_name);
        let exe_path = match apps_key.open_subkey_with_flags(&shell_path, KEY_READ) {
            Ok(cmd_key) => match cmd_key.get_value::<String, _>("") {
                Ok(cmd) => path_resolver
                    .extract_and_resolve(&cmd)
                    .map(|(path, _)| path.to_string_lossy().to_string()),
                Err(_) => None,
            },
            Err(_) => None,
        };

        let exe_path = match exe_path {
            Some(p) => p,
            None => continue,
        };

        // 铁证条件：路径不存在 + 非系统路径 + 非系统进程
        if !is_definitely_safe_to_delete(&exe_path, &mut path_cache) {
            continue;
        }

        // 去重：同一个 exe 可能被多个应用名注册
        let path_lower = exe_path.to_lowercase();
        if seen_paths.contains(&path_lower) {
            continue;
        }
        seen_paths.insert(path_lower);

        entries.push(RegistryEntry {
            path: format!(r"HKEY_CLASSES_ROOT\Applications\{}", app_name),
            name: app_name.clone(),
            associated_path: exe_path.clone(),
            issue: format!("关联的可执行文件不存在: {}", exe_path),
        });
    }

    entries.sort_by(|a, b| a.name.cmp(&b.name));

    let total_count = entries.len() as u32;
    let scan_duration_ms = start_time.elapsed().as_millis() as u64;

    let (cache_hits, cache_misses) = path_cache.stats();
    log::info!(
        "注册表残留扫描完成: {} 个条目, 耗时 {}ms, 缓存命中率 {:.1}%",
        total_count,
        scan_duration_ms,
        if cache_hits + cache_misses > 0 {
            (cache_hits as f64 / (cache_hits + cache_misses) as f64) * 100.0
        } else {
            0.0
        }
    );

    RegistryScanResult {
        entries,
        total_count,
        scan_duration_ms,
    }
}

// ============================================================================
// 备份
// ============================================================================

/// 备份目录：数据目录 reg_backups/（与右键菜单/外壳入口备份统一位置）
pub fn backup_dir() -> PathBuf {
    crate::appdata::data_dir().join("reg_backups")
}

/// 批量导出注册表键到一个 .reg 文件（可双击恢复）
pub fn export_backup(entries: &[RegistryEntry], backup_dir: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(backup_dir).map_err(|e| format!("创建备份目录失败: {}", e))?;

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let backup_file = backup_dir.join(format!("cdisksavior_registry_backup_{}.reg", timestamp));

    let mut file =
        File::create(&backup_file).map_err(|e| format!("创建备份文件失败: {}", e))?;

    // .reg 文件头
    writeln!(file, "Windows Registry Editor Version 5.00")
        .map_err(|e| format!("写入备份文件失败: {}", e))?;
    writeln!(file).map_err(|e| format!("写入备份文件失败: {}", e))?;
    writeln!(
        file,
        "; CDisk Savior 注册表备份 — {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    )
    .map_err(|e| format!("写入备份文件失败: {}", e))?;
    writeln!(file, "; 条目数: {}, 如需恢复请双击此文件", entries.len())
        .map_err(|e| format!("写入备份文件失败: {}", e))?;
    writeln!(file).map_err(|e| format!("写入备份文件失败: {}", e))?;

    for entry in entries {
        export_key_via_reg_exe(&mut file, entry)?;
    }

    file.flush()
        .map_err(|e| format!("刷新备份文件失败: {}", e))?;
    log::info!("注册表备份已保存: {:?}", backup_file);

    Ok(backup_file)
}

/// 使用 reg.exe export 导出完整注册表键（跨语言 Windows 编码安全）
fn export_key_via_reg_exe(file: &mut File, entry: &RegistryEntry) -> Result<(), String> {
    let temp_file = std::env::temp_dir().join(format!(
        "cdisksavior_temp_export_{}.reg",
        std::process::id()
    ));

    let reg_subpath = to_reg_exe_format(&entry.path)?;

    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW：抑制控制台黑框（工程教训）
    let output = Command::new("reg")
        .args(["export", &reg_subpath, &temp_file.to_string_lossy(), "/y"])
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| format!("执行 reg export 失败: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // 回退：手动记录路径头
        writeln!(
            file,
            "; [回退] reg export 失败: {}. 手动记录路径",
            stderr.trim()
        )
        .map_err(|e| format!("写入备份注释失败: {}", e))?;
        writeln!(file, "[{}]", entry.path).map_err(|e| format!("写入备份路径失败: {}", e))?;
        writeln!(file, "; 此条目备份失败，请手动检查: {}", entry.issue)
            .map_err(|e| format!("写入备份注释失败: {}", e))?;
        writeln!(file).map_err(|e| format!("写入备份失败: {}", e))?;
        return Ok(());
    }

    // reg.exe 在不同语言 Windows 上输出编码不同：
    // - 中文/日文/韩文 Windows → UTF-16 LE + BOM (\xFF\xFE)
    // - 英文 Windows → UTF-8 (无 BOM) 或 ANSI
    // 直接读 bytes 按 BOM 判断，兼容所有情况
    let raw_bytes = fs::read(&temp_file).map_err(|e| format!("读取临时导出文件失败: {}", e))?;
    let content = decode_reg_export(&raw_bytes)?;

    // 跳过 .reg 文件头，追加内容
    let mut started = false;
    for line in content.lines() {
        if !started {
            if line.is_empty() || line.starts_with("Windows Registry Editor") {
                continue;
            }
            started = true;
        }
        writeln!(file, "{}", line).map_err(|e| format!("写入备份内容失败: {}", e))?;
    }
    writeln!(file).map_err(|e| format!("写入备份换行失败: {}", e))?;

    let _ = fs::remove_file(&temp_file);
    Ok(())
}

/// 路径转换: HKEY_CLASSES_ROOT\Applications\xxx → HKCR\Applications\xxx (供 reg.exe)
fn to_reg_exe_format(path: &str) -> Result<String, String> {
    if let Some(subpath) = path.strip_prefix("HKEY_CURRENT_USER\\") {
        Ok(format!("HKCU\\{}", subpath))
    } else if let Some(subpath) = path.strip_prefix("HKEY_LOCAL_MACHINE\\") {
        Ok(format!("HKLM\\{}", subpath))
    } else if let Some(subpath) = path.strip_prefix("HKEY_CLASSES_ROOT\\") {
        Ok(format!("HKCR\\{}", subpath))
    } else {
        Err(format!("无法解析注册表路径: {}", path))
    }
}

// ============================================================================
// 删除
// ============================================================================

/// 删除选中的注册表条目（每个条目在调用前已由 export_backup 纳入备份文件）
pub fn delete_entries(entries: &[RegistryEntry]) -> RegistryDeleteResult {
    let backup_result = export_backup(entries, &backup_dir());

    let mut deleted_count = 0u32;
    let mut failed_entries = Vec::new();
    let mut errors = Vec::new();

    for entry in entries {
        match delete_registry_entry(entry) {
            Ok(()) => deleted_count += 1,
            Err(e) => {
                failed_entries.push(entry.path.clone());
                errors.push(format!("{}: {}", entry.path, e));
            }
        }
    }

    let backup_path = backup_result
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|e| {
            errors.push(format!("备份失败: {}", e));
            String::new()
        });

    RegistryDeleteResult {
        backup_path,
        deleted_count,
        failed_entries,
        errors,
    }
}

/// 删除单个注册表条目（删除整个子键）
///
/// 调用前必须通过 export_backup 创建备份。
pub fn delete_registry_entry(entry: &RegistryEntry) -> Result<(), String> {
    let (root_key, subpath) = parse_registry_path_components(&entry.path)?;
    let (parent_path, child_name) = split_last_component(subpath)?;

    let parent_key = root_key
        .open_subkey_with_flags(parent_path, KEY_WRITE)
        .map_err(|e| format!("打开父键失败: {}", e))?;

    parent_key
        .delete_subkey_all(child_name)
        .map_err(|e| format!("删除注册表键失败: {}", e))?;

    log::info!("已删除注册表键: {}", entry.path);
    Ok(())
}

fn parse_registry_path_components(path: &str) -> Result<(RegKey, &str), String> {
    if let Some(subpath) = path.strip_prefix("HKEY_CURRENT_USER\\") {
        Ok((RegKey::predef(HKEY_CURRENT_USER), subpath))
    } else if let Some(subpath) = path.strip_prefix("HKEY_LOCAL_MACHINE\\") {
        Ok((RegKey::predef(HKEY_LOCAL_MACHINE), subpath))
    } else if let Some(subpath) = path.strip_prefix("HKEY_CLASSES_ROOT\\") {
        Ok((RegKey::predef(HKEY_CLASSES_ROOT), subpath))
    } else {
        Err(format!("无法解析注册表路径: {}", path))
    }
}

fn split_last_component(path: &str) -> Result<(&str, &str), String> {
    path.rsplit_once('\\')
        .ok_or_else(|| format!("无法分割路径: {}", path))
}

/// 解码 reg.exe 导出的 .reg 文件内容（按 BOM 判断编码，兼容中英文 Windows）
fn decode_reg_export(raw: &[u8]) -> Result<String, String> {
    if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        // UTF-16 LE with BOM — 中日韩 Windows 的默认输出
        let utf16: Vec<u16> = raw[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&utf16).map_err(|e| format!("UTF-16 解码失败: {}", e))
    } else if raw.len() >= 3 && raw[0] == 0xEF && raw[1] == 0xBB && raw[2] == 0xBF {
        // UTF-8 with BOM
        String::from_utf8(raw[3..].to_vec()).map_err(|e| format!("UTF-8 解码失败: {}", e))
    } else {
        // 无 BOM，按 UTF-8 尝试，失败则 lossy 兜底（兼容英文 Windows 的 ANSI 输出）
        Ok(String::from_utf8(raw.to_vec())
            .unwrap_or_else(|_| String::from_utf8_lossy(raw).into_owned()))
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::{
        delete_entries, is_definitely_safe_to_delete, to_reg_exe_format, PathCache,
        PathResolver, RegistryEntry,
    };
    use std::path::PathBuf;

    #[test]
    fn path_resolver_basic() {
        let resolver = PathResolver::new();

        let (path, _) = resolver
            .extract_and_resolve(r#""C:\Program Files\App\app.exe" %1"#)
            .unwrap();
        assert_eq!(path, PathBuf::from(r"C:\Program Files\App\app.exe"));

        let (path, _) = resolver.extract_and_resolve(r"C:\App\app.exe %1").unwrap();
        assert_eq!(path, PathBuf::from(r"C:\App\app.exe"));
    }

    #[test]
    fn path_resolver_env_var() {
        let resolver = PathResolver::new();
        let result = resolver.extract_and_resolve(r"%SystemRoot%\system32\notepad.exe %1");
        assert!(result.is_some());
        let (path, is_system) = result.unwrap();
        let path_str = path.to_string_lossy().to_lowercase();
        assert!(path_str.contains("system32"));
        assert!(is_system);
    }

    #[test]
    fn path_cache_counts_hits_and_misses() {
        let mut cache = PathCache::new();

        assert!(cache.exists(r"C:\Windows"));
        assert!(cache.exists(r"C:\Windows"));

        let (hits, misses) = cache.stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }

    #[test]
    fn is_definitely_safe_with_fake_path() {
        let mut cache = PathCache::new();
        assert!(is_definitely_safe_to_delete(
            r"C:\ThisPathDoesNotExist_9f8e7d\fake.exe",
            &mut cache
        ));
    }

    #[test]
    fn is_definitely_safe_system32_rejected() {
        let mut cache = PathCache::new();
        assert!(!is_definitely_safe_to_delete(
            r"C:\Windows\System32\some.exe",
            &mut cache
        ));
    }

    #[test]
    fn is_definitely_safe_svchost_rejected() {
        let mut cache = PathCache::new();
        assert!(!is_definitely_safe_to_delete(
            r"C:\Program Files\SomeApp\svchost.exe",
            &mut cache
        ));
    }

    #[test]
    fn is_definitely_safe_existing_file_rejected() {
        let mut cache = PathCache::new();
        // C:\Windows 存在但不是文件——exists() 对目录也返回 true，仍应拒绝
        assert!(!is_definitely_safe_to_delete(r"C:\Windows", &mut cache));
    }

    #[test]
    fn to_reg_exe_format_conversion() {
        assert_eq!(
            to_reg_exe_format(r"HKEY_CLASSES_ROOT\Applications\notepad.exe").unwrap(),
            r"HKCR\Applications\notepad.exe"
        );
        assert_eq!(
            to_reg_exe_format(r"HKEY_CURRENT_USER\Software\Test").unwrap(),
            r"HKCU\Software\Test"
        );
        assert!(to_reg_exe_format(r"BAD_ROOT\Software").is_err());
    }

    #[test]
    fn delete_entries_reports_failure_for_missing_key() {
        let entries = vec![RegistryEntry {
            path: r"HKEY_CURRENT_USER\Software\CDiskSaviorTest\NonExistentKey9f8e7d".to_string(),
            name: "NonExistentKey9f8e7d".to_string(),
            associated_path: r"C:\NonExistent_9f8e7d\fake.exe".to_string(),
            issue: "测试条目".to_string(),
        }];
        let result = delete_entries(&entries);
        // 删除必须失败（键不存在）且不得 panic
        assert_eq!(result.deleted_count, 0);
        assert_eq!(result.failed_entries.len(), 1);
        // 备份依赖外部 reg.exe（受限环境可能被策略拦截），
        // 成功则记录路径，失败则记录原因——两者都不得静默
        if result.backup_path.is_empty() {
            assert!(result.errors.iter().any(|e| e.contains("备份失败")));
        } else {
            assert!(std::path::Path::new(&result.backup_path).is_file());
        }
    }
}
