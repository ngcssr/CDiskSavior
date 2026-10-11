//! C 级防逆向：运行时反调试守卫（仅 release 构建生效）
//!
//! 两道检测：IsDebuggerPresent（当前进程被附加）+
//! CheckRemoteDebuggerPresent（远程调试器）。
//! 启动时检查一次，之后后台线程每 15 秒复检；命中即静默退出，
//! 不向调试器输出任何错误信息（避免暴露检测点）。
//!
//! 定位：提高逆向门槛，不是对抗确定性破解——配合构建加固
//! （strip + fat LTO + 单代码生成单元）消除符号与内联边界。

#[cfg(windows)]
fn debugger_present() -> bool {
    use windows::Win32::System::Diagnostics::Debug::{
        CheckRemoteDebuggerPresent, IsDebuggerPresent,
    };
    use windows::Win32::System::Threading::GetCurrentProcess;
    unsafe {
        if IsDebuggerPresent().as_bool() {
            return true;
        }
        let mut remote = windows::core::BOOL(0);
        if CheckRemoteDebuggerPresent(GetCurrentProcess(), &mut remote).is_ok()
            && remote.as_bool()
        {
            return true;
        }
        false
    }
}

/// 应用启动时调用一次（lib.rs run 入口最前）。
pub fn startup_guard() {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        if debugger_present() {
            std::process::exit(1);
        }
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(15));
            if debugger_present() {
                std::process::exit(1);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn guard_is_callable_in_debug() {
        // debug 构建下守卫必须是空操作（不干扰开发调试）
        super::startup_guard();
    }
}
