//! 便携版 UAC 自动提权：计划任务绕过。
//!
//! 流程：
//! 1. 已提权 → 确保计划任务存在（供下次静默），正常继续
//! 2. 未提权 + 计划任务已存在 → schtasks /run 静默拉起提权实例，退出当前
//! 3. 未提权 + 无任务 → ShellExecuteW("runas") 弹一次 UAC 创建+启动，退出当前
//!
//! 效果：任意机器首次一次 UAC，之后永久静默。便携单文件，无需安装。

use std::os::windows::process::CommandExt;
use std::process::Command;

const TASK_NAME: &str = "CDiskSaviorElevated";

pub fn ensure_elevation() {
    if crate::lightc_mft::is_elevated() {
        ensure_task_exists();
        return;
    }
    if task_exists() {
        let _ = Command::new("schtasks")
            .args(["/run", "/tn", TASK_NAME])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn();
        std::process::exit(0);
    }
    // 首次：弹一次 UAC
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    unsafe { shell_execute_runas(&exe_str); }
    std::process::exit(0);
}

fn ensure_task_exists() {
    if task_exists() {
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    let _ = Command::new("schtasks")
        .args([
            "/create", "/tn", TASK_NAME,
            "/tr", &format!("\"{}\"", exe_str),
            "/sc", "onlogon", "/rl", "highest", "/f",
        ])
        .creation_flags(0x08000000)
        .spawn()
        .and_then(|mut c| c.wait());
}

fn task_exists() -> bool {
    Command::new("schtasks")
        .args(["/query", "/tn", TASK_NAME])
        .creation_flags(0x08000000)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

unsafe fn shell_execute_runas(exe_path: &str) {
    #[repr(C)]
    struct SHELLEXECUTEINFOW {
        cb_size: u32,
        f_mask: u32,
        hwnd: usize,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *const u16,
        lp_directory: *const u16,
        n_show: i32,
        h_inst_app: usize,
        lp_id_list: usize,
        lp_class: *const u16,
        hkey_class: usize,
        dw_hot_key: u32,
        h_icon: usize,
        h_process: usize,
    }
    extern "system" {
        fn ShellExecuteExW(info: *mut SHELLEXECUTEINFOW) -> i32;
    }
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let file: Vec<u16> = exe_path.encode_utf16().chain(std::iter::once(0)).collect();
    let mut info = SHELLEXECUTEINFOW {
        cb_size: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        f_mask: 0x00000040,
        hwnd: 0,
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: std::ptr::null(),
        lp_directory: std::ptr::null(),
        n_show: 1,
        h_inst_app: 0,
        lp_id_list: 0,
        lp_class: std::ptr::null(),
        hkey_class: 0,
        dw_hot_key: 0,
        h_icon: 0,
        h_process: 0,
    };
    ShellExecuteExW(&mut info);
}
