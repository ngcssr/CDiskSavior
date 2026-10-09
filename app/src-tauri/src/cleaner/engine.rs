//! R1 统一删除引擎：四层护栏 + 全局禁用原语
//!
//! 设计文档 §7 R1 / 漏洞 V4（硬编码 c:\）/ V6（reparse 识别）/ V28（闸门强制拒绝）
//!
//! 四层护栏：
//! 0. 结构性：id 模型（前端只传 id，后端当场重扫允许集合）
//! 1. canonicalize 路径校验（解析 junction → 必须在允许根下）
//! 2. FILE_ATTRIBUTE_REPARSE_POINT 属性校验（reparse → 拒绝删除）
//! 3. 唯一原语集合（remove_file / remove_dir / remove_dir_all）
//!
//! 全局禁用原语（实测穿透 junction 删目标盘数据）：
//! ⛔ del /s  ⛔ robocopy /MIR  ⛔ xcopy /E

use std::path::{Path, PathBuf};

/// 删除结果
pub enum DeleteResult {
    Deleted(u64),
    Skipped(String),
    Error(String),
}

/// 安全受保护的路径前缀（运行时从环境变量展开，V4 修复：不硬编码 c:\）
fn protected_prefixes() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in &[
        "SystemRoot", "ProgramFiles", "ProgramFiles(x86)", "ProgramData",
    ] {
        if let Some(p) = std::env::var_os(var) {
            v.push(PathBuf::from(p));
        }
    }
    // %SystemDrive%\Windows
    if let Ok(d) = std::env::var("SystemDrive") {
        v.push(PathBuf::from(&d).join("Windows"));
        v.push(PathBuf::from(&d).join("$Recycle.Bin"));
        v.push(PathBuf::from(&d).join("Windows.old"));
    }
    v
}

/// 检查路径是否为 reparse point（V6：用属性位，不用 is_symlink）
pub fn is_reparse_point(path: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };
    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let attrs = GetFileAttributesW(wide.as_ptr());
        if attrs == INVALID_FILE_ATTRIBUTES {
            return false;
        }
        (attrs & FILE_ATTRIBUTE_REPARSE_POINT) != 0
    }
}

/// 第 2 层：canonicalize 路径校验
///
/// 解析 junction/symlink 到真实目标 → 必须在允许根下且非根本身
fn is_safe_path(path: &Path, allowed_roots: &[PathBuf]) -> bool {
    // 第 2 层 a：受保护前缀
    let canon = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(_) => return false, // 不存在或无法解析 → 不安全
    };
    for prefix in protected_prefixes() {
        if let Ok(prefix_canon) = std::fs::canonicalize(&prefix) {
            if canon.starts_with(&prefix_canon) {
                // 检查是否在允许重建的系统缓存白名单内（简化：P4 不实现白名单穿透）
                return false;
            }
        }
    }
    // 第 2 层 b：必须在允许根下
    let in_allowed = allowed_roots.iter().any(|root| {
        if let Ok(root_canon) = std::fs::canonicalize(root) {
            canon.starts_with(&root_canon) && canon != root_canon
        } else {
            false
        }
    });
    if !in_allowed {
        return false;
    }
    true
}

/// 安全删除单个文件（四层护栏全过才删）
pub fn safe_delete_file(path: &Path, allowed_roots: &[PathBuf]) -> DeleteResult {
    // 第 3 层：reparse point → 拒绝
    if is_reparse_point(path) {
        return DeleteResult::Skipped(format!("跳过 reparse point: {}", path.display()));
    }
    // 第 2 层：canonicalize 校验
    if !is_safe_path(path, allowed_roots) {
        return DeleteResult::Skipped(format!("跳过受保护路径: {}", path.display()));
    }
    // 第 4 层：唯一原语
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    match std::fs::remove_file(path) {
        Ok(()) => DeleteResult::Deleted(size),
        Err(e) => DeleteResult::Error(format!("删除失败 {}: {}", path.display(), e)),
    }
}

/// 安全删除目录（四层护栏 + 不递归进 reparse point）
pub fn safe_delete_dir(path: &Path, allowed_roots: &[PathBuf]) -> DeleteResult {
    // 第 3 层：reparse point → 拒绝（不穿透 junction）
    if is_reparse_point(path) {
        return DeleteResult::Skipped(format!("跳过 reparse point（Junction/Symlink）: {}", path.display()));
    }
    // 第 2 层：canonicalize 校验
    if !is_safe_path(path, allowed_roots) {
        return DeleteResult::Skipped(format!("跳过受保护路径: {}", path.display()));
    }
    // 计算大小（删除前）
    let size = dir_size(path);
    // 第 4 层：唯一原语（remove_dir_all 实测不穿透 junction）
    // 先删内容（跳过 reparse 子项），再删目录本身
    match safe_remove_dir_all(path) {
        Ok(()) => DeleteResult::Deleted(size),
        Err(e) => DeleteResult::Error(format!("删除目录失败 {}: {}", path.display(), e)),
    }
}

/// 安全递归删除目录内容（跳过 reparse point 子项）
fn safe_remove_dir_all(path: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let p = entry.path();
        // V6：reparse point 子项跳过（不删 junction 指向的目标数据）
        if is_reparse_point(&p) {
            log::info!("跳过 reparse point 子项: {}", p.display());
            continue;
        }
        if p.is_dir() {
            safe_remove_dir_all(&p)?;
            let _ = std::fs::remove_dir(&p); // 删空目录
        } else {
            std::fs::remove_file(&p)?;
        }
    }
    std::fs::remove_dir(path) // 删自身
}

/// 计算目录大小
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 删除台账（记录每次操作，供统计和审计）
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLog {
    pub timestamp: u64,
    pub path: String,
    pub size: u64,
    pub result: String, // "deleted" | "skipped" | "error"
    pub reason: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn safe_delete_rejects_reparse_point() {
        // 创建 junction → safe_delete_dir 应跳过
        let dir = std::env::temp_dir().join(format!("cdisk-del-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let real = dir.join("real");
        let link = dir.join("link");
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("data.txt"), "hello").unwrap();

        // 创建 junction
        let _ = fs::remove_dir_all(&link);
        junction::create(&real, &link).unwrap();

        // 尝试删除 junction → 应跳过
        let result = safe_delete_dir(&link, &[dir.clone()]);
        match result {
            DeleteResult::Skipped(msg) => assert!(msg.contains("reparse")),
            _ => panic!("应跳过 reparse point"),
        }

        // 真实数据应完好
        assert!(real.join("data.txt").exists(), "真实数据不应被删");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_delete_rejects_protected_path() {
        // C:\Windows\System32 不应通过安全校验
        let sys32 = Path::new(r"C:\Windows\System32");
        if sys32.exists() {
            let result = safe_delete_dir(sys32, &[PathBuf::from(r"C:\Windows\System32")]);
            match result {
                DeleteResult::Skipped(_) => {}
                _ => panic!("系统目录应被拒绝"),
            }
        }
    }

    #[test]
    fn safe_delete_allows_temp() {
        let dir = std::env::temp_dir().join(format!("cdisk-del-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("junk.txt"), "garbage").unwrap();

        let result = safe_delete_dir(&dir, &[std::env::temp_dir()]);
        match result {
            DeleteResult::Deleted(size) => assert!(size > 0, "应返回删除大小"),
            _ => panic!("临时目录应可删除"),
        }
        assert!(!dir.exists(), "目录应已删除");
    }
}
