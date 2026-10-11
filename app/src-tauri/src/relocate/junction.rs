//! V8 修复：通过 junction crate 的 FSCTL 创建 Junction，弃用 cmd /C mklink
//!
//! junction crate 内部使用 DeviceIoControl(FSCTL_SET_REPARSE_POINT)，
//! 无 cmd.exe 子进程、无参数转义风险、含空格与 & 的路径天然安全。

use std::path::Path;

/// 创建目录联接（junction）：link_path → target_path
///
/// 成功返回 Ok(())，失败返回可读错误信息。
pub fn create_junction(link_path: &Path, target_path: &Path) -> Result<(), String> {
    // junction crate 1.4.2: create(target, junction) — target=真实数据，junction=要创建的链接
    junction::create(target_path, link_path).map_err(|e| {
        format!(
            "创建 Junction 失败：{} → {}: {}",
            link_path.display(),
            target_path.display(),
            e
        )
    })
}

/// 删除目录联接（仅删除链接本身，不影响目标数据）
///
/// 用 std::fs::remove_dir（RemoveDirectoryW），实测对 junction 只删链接本身。
/// junction crate 的 delete 只清 reparse 属性、留空目录壳——不彻底。
pub fn remove_junction(link_path: &Path) -> Result<(), String> {
    std::fs::remove_dir(link_path).map_err(|e| {
        format!("删除 Junction 失败 {}: {}", link_path.display(), e)
    })
}

/// 判断路径是否为 Junction（V6 修复：用属性位，不用 is_symlink）
///
/// 设计文档 §7 R1 / §10.3：统一用 FILE_ATTRIBUTE_REPARSE_POINT，
/// 禁止跨语言类推 is_symlink 语义（Rust 对 junction 返回 true，Python 返回 false）。
pub fn is_junction(path: &Path) -> bool {
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let attrs = GetFileAttributesW(windows::core::PCWSTR(wide.as_ptr()));
        if attrs == INVALID_FILE_ATTRIBUTES {
            return false;
        }
        (attrs & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0
    }
}

/// 获取 Junction 指向的真实目标（canonicalize 解析链接）
#[allow(dead_code)] // P9 使用
pub fn junction_target(path: &Path) -> Option<std::path::PathBuf> {
    std::fs::canonicalize(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn junction_create_remove_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cdisk-jct-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); // 清理前次残留
        let _ = fs::create_dir_all(&dir);
        let real = dir.join("real");
        let link = dir.join("link");
        let _ = fs::remove_dir_all(&link); // 确保 link 不存在
        fs::create_dir_all(&real).unwrap();
        fs::write(real.join("data.txt"), "hello").unwrap();

        // 创建 junction
        create_junction(&link, &real).expect("创建 Junction");
        assert!(is_junction(&link), "应识别为 junction");
        assert!(link.join("data.txt").exists(), "通过 junction 应能读到数据");

        // 删除 junction（仅删链接，数据不动）
        remove_junction(&link).expect("删除 Junction");
        assert!(!link.exists(), "链接应已删除");
        assert!(real.join("data.txt").exists(), "真实数据应完好");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_junction_normal_dir() {
        let dir = std::env::temp_dir();
        assert!(!is_junction(&dir), "普通目录不应被判定为 junction");
    }

    #[test]
    fn junction_with_spaces_and_ampersand() {
        // V8 场景：路径含空格与 &
        let dir = std::env::temp_dir().join(format!("cdisk-jct2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir); // 清理前次残留
        let _ = fs::create_dir_all(&dir);
        let real = dir.join("My App & Data");
        let link = dir.join("Link To & Data");
        let _ = fs::remove_dir_all(&link); // 确保 link 不存在
        fs::create_dir_all(&real).unwrap();

        create_junction(&link, &real).expect("含特殊字符路径的 Junction 应成功");
        assert!(is_junction(&link));
        remove_junction(&link).unwrap();

        let _ = fs::remove_dir_all(&dir);
    }
}
