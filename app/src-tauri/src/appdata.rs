//! 应用数据目录解析（与 Tauri app_data_dir 对齐）
//!
//! Tauri 的 app_data_dir() 需要 AppHandle；无句柄的后端模块（注册表备份、日志等）
//! 通过本模块解析同一目录，避免层层传递句柄。
//! Windows 上 app_data_dir = %APPDATA%\<identifier>（identifier 见 tauri.conf.json）。

use std::path::PathBuf;

/// Bundle identifier（tauri.conf.json），勿硬编码盘符
const IDENTIFIER: &str = "com.cdisksavior.app";

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(IDENTIFIER)
}

#[cfg(test)]
mod tests {
    use super::data_dir;

    #[test]
    fn data_dir_uses_identifier_suffix() {
        assert!(data_dir().ends_with("com.cdisksavior.app"));
    }
}
