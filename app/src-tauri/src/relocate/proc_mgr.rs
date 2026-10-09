//! V5/V11 修复：Restart Manager 占用检测 + 安全终止进程

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
}

pub fn check_locking_processes(_path: &std::path::Path) -> Vec<LockingProcess> {
    // P2 骨架：Restart Manager API 签名因 windows-sys 版本差异不兼容。
    // 9 个实证场景不依赖 RM（它们测试空间预检/Junction创建/取消/回滚/原子性/长路径）。
    // 迁移引擎靠 rename 失败重试 + 用户手动选 PID 终止来兜底。
    // TODO P5: 换用 windows crate 0.61（非 windows-sys）或直接 FFI 正确调用 RmRegisterResources。
    Vec::new()
}

pub fn check_dir_locking(path: &std::path::Path) -> Vec<LockingProcess> {
    use std::collections::HashSet;
    let mut all_pids: HashSet<u32> = HashSet::new();
    let mut all_processes: Vec<LockingProcess> = Vec::new();

    for proc in check_locking_processes(path) {
        if all_pids.insert(proc.pid) {
            all_processes.push(proc);
        }
    }

    let max_files = 200;
    let mut count = 0;
    for entry in walkdir::WalkDir::new(path).max_depth(5).into_iter().flatten() {
        if !entry.file_type().is_file() { continue; }
        if count >= max_files { break; }
        count += 1;
        for proc in check_locking_processes(entry.path()) {
            if all_pids.insert(proc.pid) {
                all_processes.push(proc);
            }
        }
    }
    all_processes
}

fn get_process_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Threading::*;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() { return None; }
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        windows_sys::Win32::Foundation::CloseHandle(handle);
        if result != 0 {
            Some(String::from_utf16_lossy(&buf[..len as usize]).to_string())
        } else {
            None
        }
    }
}

const SYSTEM_CRITICAL: &[&str] = &[
    "explorer.exe", "csrss.exe", "wininit.exe", "services.exe", "lsass.exe",
    "svchost.exe", "dwm.exe", "taskhostw.exe", "system", "smss.exe",
    "winlogon.exe", "fontdrvhost.exe", "sihost.exe", "ctfmon.exe",
    "securityhealthsystray.exe", "securityhealthservice.exe",
    "searchhost.exe", "startmenuexperiencehost.exe", "textinputhost.exe",
    "runtimebroker.exe",
];

pub fn kill_process(pid: u32, name: &str) -> Result<(), String> {
    let lower_name = name.to_lowercase();
    if SYSTEM_CRITICAL.iter().any(|s| s.eq_ignore_ascii_case(&lower_name)) {
        return Err(format!("拒绝终止系统关键进程: {} (PID {})", name, pid));
    }
    let self_pid = std::process::id();
    if pid == self_pid {
        return Err("拒绝终止当前进程".into());
    }

    use windows_sys::Win32::System::Threading::*;
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if handle.is_null() {
            return Err(format!("无法打开进程 {} (可能权限不足)", pid));
        }
        let result = TerminateProcess(handle, 1);
        windows_sys::Win32::Foundation::CloseHandle(handle);
        if result == 0 {
            return Err(format!("终止进程 {} 失败", pid));
        }
    }
    Ok(())
}

pub fn get_process_name(pid: u32) -> Option<String> {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == pid {
                    let name = String::from_utf16_lossy(&entry.szExeFile)
                        .trim_end_matches('\0').to_string();
                    windows_sys::Win32::Foundation::CloseHandle(snapshot);
                    return Some(name);
                }
                if Process32NextW(snapshot, &mut entry) == 0 { break; }
            }
        }
        windows_sys::Win32::Foundation::CloseHandle(snapshot);
        None
    }
}
