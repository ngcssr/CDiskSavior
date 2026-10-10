//! M6 启动项管理
//!
//! 移植自 disk-butler 的 startup.rs，简化（去掉图标提取和知识引擎）。
//! 通过 StartupApproved 二进制值切换启用/禁用（与任务管理器"禁用"完全等效、可逆）。

use serde::Serialize;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_SET_VALUE, REG_BINARY};
use winreg::RegKey;

const RUN_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_RUN: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_FOLDER: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupItem {
    pub id: String,
    pub name: String,
    pub command: String,
    pub location: String,
    pub enabled: bool,
    pub needs_admin: bool,
    pub mem_mb: u64,
    pub advice: String,
    pub reason: String,
}

/// 枚举启动项（注册表 Run + 启动文件夹）
pub fn list_items() -> Vec<StartupItem> {
    let mut out = Vec::new();

    // HKCU Run
    for (kind, hive, hive_name, needs_admin) in [
        ("hkcu-run", HKEY_CURRENT_USER, "用户注册表", false),
        ("hklm-run", HKEY_LOCAL_MACHINE, "系统注册表", true),
    ] {
        if let Ok(sub) = RegKey::predef(hive).open_subkey_with_flags(RUN_KEY, KEY_READ) {
            for (name, value) in sub.enum_values().flatten() {
                let enabled = read_enabled(hive, APPROVED_RUN, &name);
                let cmd_str = decode_reg_string(&value.bytes);
                let (advice, reason) = advise(&name, &cmd_str);
                out.push(StartupItem {
                    id: format!("{}::{}", kind, name),
                    name: name.clone(),
                    command: value.to_string(),
                    location: hive_name.into(),
                    enabled,
                    needs_admin,
                    mem_mb: 0,
                    advice: advice.into(),
                    reason: reason.into(),
                });
            }
        }
    }

    // 启动文件夹
    for (kind, env_var, hive, hive_name, needs_admin) in [
        ("user-folder", "APPDATA", HKEY_CURRENT_USER, "用户启动文件夹", false),
        ("common-folder", "ProgramData", HKEY_LOCAL_MACHINE, "公共启动文件夹", true),
    ] {
        if let Ok(base) = std::env::var(env_var) {
            let folder = format!("{}\\Microsoft\\Windows\\Start Menu\\Programs\\Startup", base);
            if let Ok(read) = std::fs::read_dir(&folder) {
                for entry in read.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path().to_string_lossy().to_string();
                    let enabled = read_enabled(hive, APPROVED_FOLDER, &fname);
                    let (advice, reason) = advise(&fname, &path);
                    out.push(StartupItem {
                        id: format!("{}::{}", kind, fname),
                        name: fname,
                        command: path,
                        location: hive_name.into(),
                        enabled,
                        needs_admin,
                        mem_mb: 0,
                        advice: advice.into(),
                        reason: reason.into(),
                    });
                }
            }
        }
    }

    out
}

/// 切换启用状态（V27：id 格式校验；HKLM 需提权）
pub fn set_enabled(id: &str, enabled: bool) -> Result<(), String> {
    let Some((kind, name)) = id.split_once("::") else {
        return Err("无效的启动项标识".into());
    };
    match kind {
        "hkcu-run" => write_enabled(HKEY_CURRENT_USER, APPROVED_RUN, name, enabled),
        "hklm-run" => write_enabled(HKEY_LOCAL_MACHINE, APPROVED_RUN, name, enabled),
        "user-folder" => write_enabled(HKEY_CURRENT_USER, APPROVED_FOLDER, name, enabled),
        "common-folder" => write_enabled(HKEY_LOCAL_MACHINE, APPROVED_FOLDER, name, enabled),
        _ => Err("无效的启动项类型".into()),
    }
}

/// 读取启用状态（StartupApproved 下的二进制值）
fn read_enabled(hive: isize, path: &str, name: &str) -> bool {
    let Ok(approved) = RegKey::predef(hive).open_subkey_with_flags(path, KEY_READ) else {
        return true;
    };
    match approved.get_raw_value(name) {
        Ok(val) => {
            if val.bytes.len() >= 4 {
                val.bytes[0] != 0x02
            } else {
                true
            }
        }
        Err(_) => true,
    }
}

/// 写入启用/禁用状态
fn write_enabled(hive: isize, path: &str, name: &str, enabled: bool) -> Result<(), String> {
    let approved = RegKey::predef(hive)
        .open_subkey_with_flags(path, KEY_SET_VALUE)
        .map_err(|e| format!("打开注册表失败（可能需要管理员）: {}", e))?;

    let flag: u8 = if enabled { 0x03 } else { 0x02 };
    let mut data = vec![flag, 0, 0, 0];
    data.resize(12, 0);
    approved
        .set_raw_value(name, &winreg::RegValue { vtype: REG_BINARY, bytes: data })
        .map_err(|e| format!("写入注册表失败: {}", e))?;
    Ok(())
}

/// 解码注册表字符串值（winreg 返回 UTF-16LE 字节，from_utf8_lossy 会乱码）
fn decode_reg_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// 简化的建议引擎
fn advise(name: &str, command: &str) -> (&'static str, &'static str) {
    let lower = name.to_lowercase();
    // 输入法
    if lower.contains("ctfmon") || lower.contains("input") {
        return ("keep", "输入法相关，禁用后可能无法打字");
    }
    // 安全软件
    if lower.contains("security") || lower.contains("defender") || lower.contains("360") {
        return ("keep", "安全软件，建议保留");
    }
    // 常见可禁用
    if lower.contains("teams") || lower.contains("onedrive") || lower.contains("spotify") {
        return ("disable", "非必需自启，按需手动启动即可");
    }
    if command.contains("OneDrive") {
        return ("disable", "OneDrive 自启，手动启动即可");
    }
    ("neutral", "根据使用习惯决定")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_reg_string_utf16() {
        let s = "C:\\Program Files\\Test 中文";
        let mut bytes: Vec<u8> = Vec::new();
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        bytes.push(0); bytes.push(0);
        assert_eq!(decode_reg_string(&bytes), s, "UTF-16 解码应无损");
    }

    #[test]
    fn decode_reg_string_ascii() {
        let s = "notepad.exe";
        let mut bytes: Vec<u8> = Vec::new();
        for u in s.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        assert_eq!(decode_reg_string(&bytes), s);
    }
}
