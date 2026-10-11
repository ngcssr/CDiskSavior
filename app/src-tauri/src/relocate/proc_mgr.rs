//! V5/V11 修复：Restart Manager 占用检测 + 安全终止进程

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockingProcess {
    pub pid: u32,
    pub name: String,
    pub path: Option<String>,
}

/// C2 定案：Restart Manager 真实占用检测（windows crate 0.62 签名已兼容）。
///
/// 对单个路径启动 RM 会话 → 注册为文件资源 → 枚举占用进程。
/// 返回空 Vec 表示无占用或 RM 调用失败（失败时由 rename 重试 + 用户手选 PID 兜底，与 P2 策略一致）。
pub fn check_locking_processes(path: &std::path::Path) -> Vec<LockingProcess> {
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
    };

    let mut out = Vec::new();
    unsafe {
        // 会话 key：32 个 u16 的 GUID 形式（无需真实 GUID，唯一即可）
        let mut key = [0u16; 32];
        let key_str = format!("{:08x}-{:04x}-cds1", std::process::id(), path.to_string_lossy().len());
        for (i, u) in key_str.encode_utf16().take(31).enumerate() {
            key[i] = u;
        }
        let mut session = 0u32;
        if RmStartSession(&mut session, None, windows::core::PWSTR(key.as_mut_ptr())).0 != 0 {
            return out;
        }

        let wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
        let files = [windows::core::PCWSTR(wide.as_ptr())];
        if RmRegisterResources(session, Some(&files), None, None).0 != 0 {
            RmEndSession(session);
            return out;
        }

        // 第一次调用取所需缓冲数量
        let mut needed = 0u32;
        let mut count = 0u32;
        let mut reason = 0u32;
        let rc = RmGetList(session, &mut needed, &mut count, None, &mut reason);
        if rc.0 != 0 || needed == 0 {
            RmEndSession(session);
            return out;
        }

        let mut infos: Vec<RM_PROCESS_INFO> = vec![RM_PROCESS_INFO::default(); needed as usize];
        count = needed;
        let rc = RmGetList(session, &mut needed, &mut count, Some(infos.as_mut_ptr()), &mut reason);
        if rc.0 == 0 {
            for info in infos.iter().take(count as usize) {
                let name = String::from_utf16_lossy(&info.strAppName)
                    .trim_end_matches('\0')
                    .to_string();
                let pid = info.Process.dwProcessId;
                out.push(LockingProcess {
                    pid,
                    path: get_process_path(pid),
                    name: if name.is_empty() {
                        get_process_name(pid).unwrap_or_else(|| format!("PID {}", pid))
                    } else {
                        name
                    },
                });
            }
        }
        RmEndSession(session);
    }
    out
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
    use windows::Win32::System::Threading::*;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let result = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if result.is_ok() {
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

    use windows::Win32::System::Threading::*;
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, false, pid)
            .map_err(|_| format!("无法打开进程 {} (可能权限不足)", pid))?;
        let result = TerminateProcess(handle, 1);
        let _ = windows::Win32::Foundation::CloseHandle(handle);
        if result.is_err() {
            return Err(format!("终止进程 {} 失败", pid));
        }
    }
    Ok(())
}

pub fn get_process_name(pid: u32) -> Option<String> {
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID == pid {
                    let name = String::from_utf16_lossy(&entry.szExeFile)
                        .trim_end_matches('\0').to_string();
                    let _ = windows::Win32::Foundation::CloseHandle(snapshot);
                    return Some(name);
                }
                if Process32NextW(snapshot, &mut entry).is_err() { break; }
            }
        }
        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
        None
    }
}
