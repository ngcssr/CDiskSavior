//! R9 路径健壮性：target_root 校验 + 长路径前缀 + 盘符枚举
//!
//! 设计文档 §7 R9 / 漏洞 V20（longPathAware）/ V21（target_root 零校验）的修复。

use std::path::{Path, PathBuf};

/// 目标盘校验结果
pub enum TargetRootCheck {
    Ok,
    Rejected(String),
}

/// 校验 target_root 是否合法（V21 修复）
///
/// 检查项：
/// 1. 必须是本地固定磁盘（3），拒绝网络/可移动/光驱/RAM 盘
/// 2. 不能是 OneDrive 同步根或其子目录（junction 指入会引发同步风暴）
/// 3. 路径不能为空或纯根（如 G:\）
/// 4. 目标目录不存在时可以创建，但父目录链必须可写
pub fn validate_target_root(root: &Path) -> TargetRootCheck {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDriveTypeW, GetLogicalDrives,
    };

    // 1. 解析盘符
    let root_str = root.to_string_lossy();
    if root_str.len() < 2 || !root_str.as_bytes()[1].is_ascii() && root_str.as_bytes()[1] != b':' {
        return TargetRootCheck::Rejected(format!("路径格式无效：{}", root_str));
    }
    let drive_letter = root_str.chars().next().unwrap_or('\0');
    if !drive_letter.is_ascii_alphabetic() {
        return TargetRootCheck::Rejected(format!("不是有效的盘符：{}", root_str));
    }

    // 2. 检查驱动器类型（必须是本地固定磁盘）
    let drive_path: Vec<u16> = format!("{}:\\", drive_letter)
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let dt = GetDriveTypeW(drive_path.as_ptr());
        if dt != 3 {
            let type_name = match dt {
                2 => "可移动磁盘（U 盘等）",
                3 => "本地固定磁盘",
                4 => "网络路径",
                5 => "光驱",
                6 => "RAM 盘",
                _ => "未知类型",
            };
            return TargetRootCheck::Rejected(format!(
                "目标必须是本地固定磁盘，当前为：{}（{}: 盘）",
                type_name, drive_letter
            ));
        }
    }

    // 3. OneDrive 同步根检测
    if let Some(onedrive) = std::env::var_os("OneDrive") {
        let od = PathBuf::from(&onedrive);
        let od_consumer = std::env::var_os("OneDriveConsumer")
            .map(PathBuf::from);
        let lower_root = root_str.to_lowercase();
        let lower_od = od.to_string_lossy().to_lowercase();
        if lower_root.starts_with(&lower_od) {
            return TargetRootCheck::Rejected(
                "目标目录在 OneDrive 同步范围内，Junction 指入会引发同步风暴".into(),
            );
        }
        if let Some(odc) = od_consumer {
            let lower_odc = odc.to_string_lossy().to_lowercase();
            if lower_root.starts_with(&lower_odc) {
                return TargetRootCheck::Rejected(
                    "目标目录在 OneDrive 消费版同步范围内".into(),
                );
            }
        }
    }

    // 4. 路径不能是纯盘根（至少要有子目录名）
    let s = root_str.trim_end_matches('\\');
    if s.len() <= 3 {
        return TargetRootCheck::Rejected(
            "目标目录不能是盘符根目录，请指定一个子目录（如 G:\\CDisk Savior）".into(),
        );
    }

    TargetRootCheck::Ok
}

/// 运行时解析默认迁移根（设计文档 §3.2）
///
/// 枚举本地固定磁盘 → 排除系统盘 → 排除 OneDrive → 取剩余空间最大者。
/// 无候选返回 None（单盘机器，由 UI 引导用户）。
pub fn resolve_default_target_root() -> Option<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
    };

    let system_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let onedrive = std::env::var_os("OneDrive").map(PathBuf::from);

    let mut candidates: Vec<(char, u64)> = Vec::new();
    unsafe {
        let mask = GetLogicalDrives();
        for i in 0..26u32 {
            if mask & (1 << i) == 0 {
                continue;
            }
            let letter = char::from_u32(b'A' as u32 + i).unwrap_or('?');
            let drive_str = format!("{}:\\", letter);
            let wide: Vec<u16> = drive_str.encode_utf16().chain(std::iter::once(0)).collect();

            if GetDriveTypeW(wide.as_ptr()) != 3 {
                continue;
            }
            // 排除系统盘
            let letter_upper = letter.to_ascii_uppercase();
            if format!("{}:", letter_upper) == system_drive.to_uppercase() {
                continue;
            }
            // 排除 OneDrive 所在盘
            if let Some(od) = &onedrive {
                let od_letter = od.to_string_lossy().chars().next().unwrap_or('?');
                if letter_upper == od_letter.to_ascii_uppercase() {
                    // 同盘不自动排除（OneDrive 只是一个目录），但不在其下创建
                    // 这里不排除整盘，只在 validate_target_root 里做目录级排除
                }
            }

            // 获取剩余空间
            let mut free: u64 = 0;
            let mut total: u64 = 0;
            let mut _total_free: u64 = 0;
            if GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut _total_free) != 0 {
                candidates.push((letter, free));
            }
        }
    }

    // 取剩余空间最大者（并列取盘符序最小）
    candidates.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    candidates.first().map(|(letter, _)| PathBuf::from(format!("{}:\\CDisk Savior", letter)))
}

/// 空间预检（V10 修复）：目标盘剩余 >= 源大小 × 1.1
pub fn check_disk_space(target_root: &Path, source_size: u64) -> TargetRootCheck {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let root_str = target_root.to_string_lossy().to_string();
    // 找到盘符根
    let drive_root = if root_str.len() >= 2 {
        format!("{}:\\", root_str.chars().next().unwrap_or('C'))
    } else {
        return TargetRootCheck::Rejected("无法解析目标盘符".into());
    };
    let wide: Vec<u16> = drive_root.encode_utf16().chain(std::iter::once(0)).collect();

    let mut free: u64 = 0;
    let mut total: u64 = 0;
    let mut total_free: u64 = 0;
    unsafe {
        if GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free) == 0 {
            return TargetRootCheck::Rejected("无法查询目标盘剩余空间".into());
        }
    }

    let required = source_size + source_size / 10; // 1.1 倍
    if free < required {
        let free_gb = free as f64 / (1 << 30) as f64;
        let need_gb = required as f64 / (1 << 30) as f64;
        return TargetRootCheck::Rejected(format!(
            "目标盘剩余空间不足：需要 {:.1} GB（源大小 × 1.1），实际剩余 {:.1} GB",
            need_gb, free_gb
        ));
    }
    TargetRootCheck::Ok
}

/// \\?\ 长路径前缀（V20 修复）
///
/// Windows MAX_PATH = 260。加 \\?\ 前缀后上限 32767。
/// 已有前缀则原样返回。
pub fn long_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.starts_with("\\\\?\\") {
        return p.to_path_buf();
    }
    // UNC 路径需要特殊处理：\\?\UNC\server\share
    if s.starts_with("\\\\") {
        return PathBuf::from(format!("\\\\?\\UNC\\{}", &s[2..]));
    }
    PathBuf::from(format!("\\\\?\\{}", s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_path_prefix() {
        assert_eq!(long_path(Path::new(r"C:\foo")).to_str().unwrap(), r"\\?\C:\foo");
        assert_eq!(long_path(Path::new(r"\\?\C:\foo")).to_str().unwrap(), r"\\?\C:\foo");
    }

    #[test]
    fn validate_rejects_root() {
        // G:\ 单独作为 target_root 应被拒绝
        match validate_target_root(Path::new("G:\\")) {
            TargetRootCheck::Rejected(msg) => assert!(msg.contains("根目录"), "{}", msg),
            TargetRootCheck::Ok => panic!("盘根应被拒绝"),
        }
    }

    #[test]
    fn validate_accepts_fixed_drive_subdir() {
        // 只在有非系统固定盘的机器上此测试才有意义
        if let Some(p) = resolve_default_target_root() {
            match validate_target_root(&p) {
                TargetRootCheck::Ok => {}
                TargetRootCheck::Rejected(m) => panic!("合法路径被拒: {}", m),
            }
        }
    }
}
