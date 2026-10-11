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
        // 已提权：后台确保计划任务存在（schtasks /query 在 Task Scheduler
        // 服务未就绪时会阻塞十几秒，必须异步，不能卡住 UI 启动）
        std::thread::spawn(ensure_task_exists);
        return;
    }
    if task_matches_current() {
        let _ = Command::new("schtasks")
            .args(["/run", "/tn", TASK_NAME])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn();
        std::process::exit(0);
    }
    // 首次或 exe 已移动：弹一次 UAC 重建任务并启动
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    unsafe { shell_execute_runas(&exe_str); }
    std::process::exit(0);
}

fn ensure_task_exists() {
    if task_matches_current() {
        return;
    }
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    // 手动触发型任务：/sc once + 过去日期 → 永不自动运行，只能 schtasks /run 显式拉起。
    // （切勿用 onlogon——那会导致每次登录自动弹提权实例，非预期。）
    let _ = Command::new("schtasks")
        .args([
            "/create", "/tn", TASK_NAME,
            "/tr", &format!("\"{}\"", exe_str),
            "/sc", "once", "/st", "00:00", "/sd", "01/01/2000",
            "/rl", "highest", "/f",
        ])
        .creation_flags(0x08000000)
        .spawn()
        .and_then(|mut c| c.wait());
}

fn task_matches_current() -> bool {
    // 不仅要存在，还要指向当前 exe（便携版可能被移动到新路径）
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy().to_string();
    let out = Command::new("schtasks")
        .args(["/query", "/tn", TASK_NAME, "/fo", "list", "/v"])
        .creation_flags(0x08000000)
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            text.contains(&exe_str)
        }
        _ => false,
    }
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
