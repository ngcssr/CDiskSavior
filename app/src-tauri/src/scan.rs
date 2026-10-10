//! M1/M2 扫描引擎：磁盘枚举 + 目录树构建
//!
//! P3 实现：get_drives（真实 Windows API）+ scan_drive（jwalk 并行扫描 → TreeNode）

use serde::{Deserialize, Serialize};

/// 扫描树节点（设计文档 §7 R6：以 disk-butler 的 TreeNode 为准）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub has_children: bool,
    pub category: String,
    pub friendly_name: Option<String>,
    pub description: Option<String>,
    pub safety: String,
    #[serde(default)]
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    fn new_dir(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size: 0,
            is_dir: true,
            has_children: false,
            category: "unknown".into(),
            friendly_name: None,
            description: None,
            safety: "unknown".into(),
            children: Vec::new(),
        }
    }

    fn new_file(name: &str, path: &str, size: u64) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            size,
            is_dir: false,
            has_children: false,
            category: "file".into(),
            friendly_name: None,
            description: None,
            safety: "safe".into(),
            children: Vec::new(),
        }
    }
}

/// 驱动器信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveInfo {
    pub mount: String,
    pub label: String,
    pub total: u64,
    pub free: u64,
    pub fs: String,
    pub kind: String,
}

/// 枚举所有驱动器（真实 Windows API）
pub fn get_drives() -> Vec<DriveInfo> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    };

    let mut drives = Vec::new();

    unsafe {
        // 获取驱动器字符串列表（如 "C:\\\0D:\\\0\0"）
        let mut buf = [0u16; 256];
        let len = GetLogicalDriveStringsW(buf.len() as u32, buf.as_mut_ptr());
        if len == 0 {
            return drives;
        }

        let drive_strings = String::from_utf16_lossy(&buf[..len as usize]);
        for drive_str in drive_strings.split('\0') {
            if drive_str.is_empty() {
                continue;
            }

            let wide: Vec<u16> = drive_str.encode_utf16().chain(std::iter::once(0)).collect();
            let dt = GetDriveTypeW(wide.as_ptr());

            let kind = match dt {
                2 => "removable",
                3 => "fixed",
                4 => "network",
                5 => "cdrom",
                6 => "ramdisk",
                _ => "unknown",
            };

            // 跳过光驱和网络盘（不扫描）
            if dt == 5 {
                continue;
            }

            // 获取剩余/总空间
            let mut free: u64 = 0;
            let mut total: u64 = 0;
            let mut total_free: u64 = 0;
            let _ = GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, &mut total, &mut total_free);

            // 获取卷标和文件系统
            let mut label_buf = [0u16; 256];
            let mut fs_buf = [0u16; 256];
            let mut serial = 0u32;
            let mut max_len = 0u32;
            let mut flags = 0u32;
            let _ = GetVolumeInformationW(
                wide.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as u32,
                &mut serial,
                &mut max_len,
                &mut flags,
                fs_buf.as_mut_ptr(),
                fs_buf.len() as u32,
            );

            let label = String::from_utf16_lossy(&label_buf)
                .trim_end_matches('\0')
                .to_string();
            let fs = String::from_utf16_lossy(&fs_buf)
                .trim_end_matches('\0')
                .to_string();

            drives.push(DriveInfo {
                mount: drive_str.to_string(),
                label,
                total,
                free,
                fs,
                kind: kind.to_string(),
            });
        }
    }

    drives
}

/// 扫描目录树（jwalk 并行遍历）
///
/// 返回剪枝后的 TreeNode（只保留目录层 + 一级子项的大小汇总）。
/// 设计文档 §6 C6：P3 用 jwalk 慢速路径（MFT 引擎 P8 后接入）。
pub fn scan_directory(root: &str) -> Result<TreeNode, String> {
    let root_path = std::path::Path::new(root);
    if !root_path.exists() {
        return Err(format!("路径不存在: {}", root));
    }
    if !root_path.is_dir() {
        return Err(format!("不是目录: {}", root));
    }

    let name = root_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string());

    let mut tree = TreeNode::new_dir(&name, root);
    build_tree(&root_path, &mut tree, 0, 2); // 深度 2：顶层 + 一级子目录

    // 写扫描缓存（load_scan_cache 秒显上次结果；写失败不影响扫描）
    write_scan_cache(root, &tree);

    Ok(tree)
}

// ---------- 扫描缓存（scan-cache-<root>.json，数据目录下） ----------

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanCache {
    pub scanned_at: u64,
    pub root: String,
    pub tree: TreeNode,
}

fn scan_cache_file(root: &str) -> Option<std::path::PathBuf> {
    let safe = root.replace(['\\', ':', '/'], "_");
    Some(crate::appdata::data_dir().join(format!("scan-cache-{}.json", safe)))
}

fn write_scan_cache(root: &str, tree: &TreeNode) {
    let Some(file) = scan_cache_file(root) else { return };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cache = ScanCache { scanned_at: now, root: root.to_string(), tree: tree.clone() };
    let _ = crate::state::atomic_write_json(&file, &cache);
}

/// 读取上次扫描缓存（根路径匹配才返回；损坏自愈）
pub fn load_scan_cache(root: &str) -> Option<ScanCache> {
    let file = scan_cache_file(root)?;
    let (cache, _) = crate::state::read_json_self_heal::<ScanCache>(&file).ok()??;
    if cache.root != root { return None; }
    Some(cache)
}

/// 递归构建目录树——返回子树总大小（单次遍历，消除冗余 dir_size 调用）
///
/// 性能优化：此前每个子目录先 dir_size()（全树遍历）再 build_tree()（又遍历子项），
/// 子树被走两遍。现在 build_tree 自底向上返回大小，每目录只走一次。
fn build_tree(path: &std::path::Path, node: &mut TreeNode, depth: usize, max_depth: usize) -> u64 {
    // 超过显示深度：只算大小（快速路径，不建节点）
    if depth >= max_depth {
        return dir_size(path);
    }

    let mut total_size: u64 = 0;
    let mut children: Vec<TreeNode> = Vec::new();

    // 使用 jwalk 并行遍历
    for entry in jwalk::WalkDir::new(path)
        .max_depth(1)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.path() == path {
            continue; // 跳过自身
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let path_str = entry.path().to_string_lossy().to_string();
        let ft = entry.file_type();

        if ft.is_dir() {
            let mut child = TreeNode::new_dir(&name, &path_str);
            // 递归构建子树 + 获取大小（单次遍历）
            let size = build_tree(&entry.path(), &mut child, depth + 1, max_depth);
            child.size = size;
            total_size += size;
            children.push(child);
        } else if ft.is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_size += size;
            // 只保留大文件（>1MB）以减少树的大小
            if size > 1024 * 1024 {
                children.push(TreeNode::new_file(&name, &path_str, size));
            }
        }
    }

    // 按大小降序排列
    children.sort_by(|a, b| b.size.cmp(&a.size));
    // 限制子节点数量（TreeMap 性能）
    children.truncate(200);

    node.size = total_size;
    node.has_children = !children.is_empty();
    node.children = children;
    total_size
}

/// 计算目录大小（不跟随链接）
fn dir_size(path: &std::path::Path) -> u64 {
    let mut total = 0u64;
    for entry in jwalk::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}

/// 内存报告（简化版：用 sysinfo 风格的 Windows API）
pub fn memory_report() -> serde_json::Value {
    use windows_sys::Win32::System::SystemInformation::*;

    unsafe {
        let mut meminfo: MEMORYSTATUSEX = std::mem::zeroed();
        meminfo.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        GlobalMemoryStatusEx(&mut meminfo);

        serde_json::json!({
            "overview": {
                "total": meminfo.ullTotalPhys,
                "available": meminfo.ullAvailPhys,
                "usedPercent": meminfo.dwMemoryLoad,
                "swapTotal": meminfo.ullTotalPageFile,
                "swapAvailable": meminfo.ullAvailPageFile,
            },
            "processes": [],
            "phase": "P3"
        })
    }
}

/// 页面文件检查
pub fn pagefile_check() -> serde_json::Value {
    // P3 简化：返回内存报告中的 swap 信息
    let mem = memory_report();
    let overview = &mem["overview"];
    serde_json::json!({
        "consistent": true,
        "details": [{
            "total": overview["swapTotal"],
            "available": overview["swapAvailable"],
        }],
        "phase": "P3"
    })
}
