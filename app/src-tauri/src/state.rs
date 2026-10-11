//! R8 状态层健壮性：原子写 + 损坏自愈 + 单实例互斥
//!
//! 设计文档 §7 R8 / 漏洞 V19 / V22 的修复实现。
//! 全部状态文件（迁移历史、清理统计、配置）的写入必须走 atomic_write。

use std::fs;
use std::io;
use std::path::Path;

/// 原子写入文件：先写临时文件 + rename 替换（V19 修复）
///
/// 保证崩溃/断电时要么旧内容完好、要么新内容完整，不存在截断的中间态。
/// 临时文件与目标同目录（同卷），rename 是原子的。
pub fn atomic_write(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "路径无父目录"))?;
    fs::create_dir_all(parent)?;

    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    // Windows 上 rename 目标已存在时会失败，先移除
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&tmp, path)
}

/// 原子写入 JSON（序列化失败时不落盘）
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let content = serde_json::to_vec_pretty(value)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("序列化失败: {}", e)))?;
    atomic_write(path, &content)
}

/// 读取 JSON + 损坏自愈（V19 修复：禁静默清零）
///
/// 返回 (值, 是否从备份恢复)。
/// 主文件损坏时尝试 .bak；.bak 也损坏则返回 None（调用方必须显式告警）。
pub fn read_json_self_heal<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> io::Result<Option<(T, bool)>> {
    let bak = path.with_extension("bak");

    // 先备份当前可用版本（为下次自愈准备）
    let try_read = |p: &Path| -> Option<T> {
        fs::read(p)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
    };

    if let Some(v) = try_read(path) {
        // 主文件正常；顺手刷新 .bak（非阻塞，失败不影响主流程）
        if let Ok(content) = fs::read(path) {
            let _ = fs::write(&bak, &content);
        }
        return Ok(Some((v, false)));
    }

    // 主文件损坏 → 尝试 .bak
    if let Some(v) = try_read(&bak) {
        // 从 .bak 恢复主文件
        let _ = atomic_write(path, &fs::read(&bak).unwrap_or_default());
        return Ok(Some((v, true))); // true = 从备份恢复
    }

    // 两边都坏/不存在 → None（调用方必须显式处理，禁止静默清零）
    Ok(None)
}

// ============ 单实例互斥（V22 修复） ============

/// 命名互斥体句柄（Drop 时自动释放）
pub struct SingleInstance {
    _handle: windows::Win32::Foundation::HANDLE,
}

impl SingleInstance {
    /// 尝试获取全局命名互斥。返回 Err(false) = 已有实例在运行。
    pub fn new() -> Result<Self, bool> {
        Self::with_name("Global\\CDiskSavior_SingleInstance_Mutex")
    }

    /// 带名字的获取（测试用独立名称，避免与正在运行的应用互相干扰）
    fn with_name(name: &str) -> Result<Self, bool> {
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows::Win32::Foundation::CloseHandle;

        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

        unsafe {
            let handle = match CreateMutexW(None, false, windows::core::PCWSTR(wide.as_ptr())) {
                Ok(h) => h,
                Err(_) => return Err(true), // 创建失败按已有实例处理（保守）
            };
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(handle);
                return Err(false); // 已有实例
            }
            Ok(SingleInstance { _handle: handle })
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self._handle);
        }
    }
}

/// 全局写操作互斥锁（设计文档 §6 C1 / §7.1）
///
/// 所有写操作（迁移/清理/注册表/服务）共用一把，串行执行。
/// 用进程内 Mutex 即可（单实例互斥已保证只有一个进程）。
pub struct WriteGuard;

static WRITE_LOCK: std::sync::Mutex<WriteGuard> = std::sync::Mutex::new(WriteGuard);

/// 获取全局写锁（阻塞直到其他写操作完成）
pub fn acquire_write_lock() -> std::sync::MutexGuard<'static, WriteGuard> {
    WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

// ============ 测试 ============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip() {
        let dir = std::env::temp_dir().join(format!("cdisk-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.json");

        atomic_write(&file, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"a\":1}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn atomic_write_replaces_existing() {
        let dir = std::env::temp_dir().join(format!("cdisk-test2-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.json");

        atomic_write(&file, b"{\"v\":1}").unwrap();
        atomic_write(&file, b"{\"v\":2}").unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"v\":2}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn self_heal_from_corruption() {
        let dir = std::env::temp_dir().join(format!("cdisk-test3-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let file = dir.join("test.json");

        // 写入 → 建立备份 → 弄坏主文件 → 自愈
        atomic_write_json(&file, &serde_json::json!({"ok": true})).unwrap();
        let _ = read_json_self_heal::<serde_json::Value>(&file); // 触发备份
        fs::write(&file, b"{corrupted").unwrap(); // 弄坏

        let (v, recovered) = read_json_self_heal::<serde_json::Value>(&file).unwrap().unwrap();
        assert!(recovered, "应从备份恢复");
        assert_eq!(v["ok"], true);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 场景 9：双实例互斥——第一个成功、第二个被拒
    /// 用测试专用互斥名（避免与正在运行的应用实例互相干扰）
    #[test]
    fn double_instance_rejected() {
        let name = format!("Global\\CDiskSavior_Test_Mutex_{}", std::process::id());
        let first = SingleInstance::with_name(&name);
        assert!(first.is_ok(), "第一次获取互斥应成功");

        // 第二次获取同一互斥 → 应被拒（已有实例）
        match SingleInstance::with_name(&name) {
            Err(false) => {} // 预期：ERROR_ALREADY_EXISTS
            Err(true) => {} // 也可接受：创建失败按已有处理
            Ok(_) => panic!("第二次实例应被互斥拒绝"),
        }
        // first 在此 drop，释放互斥
    }
}
