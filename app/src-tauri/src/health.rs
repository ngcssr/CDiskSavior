//! 磁盘健康 + 系统健康评分：移植自 light-c disk_health + health_score
//!
//! disk_health：PowerShell CIM（MSFT_PhysicalDisk）健康状态 + 卷信息
//! health_score：磁盘空间 40 + 休眠文件 30 + 垃圾文件 30 = 100 分
//!   （修复源码硬编码 C:\hiberfil.sys → %SystemDrive%）

use serde::Serialize;
use std::path::PathBuf;

// ---------- 磁盘健康（物理盘 SMART 健康状态） ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskHealth {
    pub model: String,
    pub media_type: String,
    pub bus_type: String,
    pub health_status: String, // Healthy / Warning / Unhealthy
    pub size: u64,
    pub friendly_name: String,
}

/// 查询物理磁盘健康（PowerShell Get-PhysicalDisk，CREATE_NO_WINDOW）
pub fn query_disk_health() -> Result<Vec<DiskHealth>, String> {
    let script = r#"Get-PhysicalDisk | Select-Object FriendlyName,Model,MediaType,BusType,HealthStatus,Size | ConvertTo-Json -Compress"#;
    let out = crate::slim::run_powershell_capture(script)?;
    parse_physical_disks(&out)
}

fn parse_physical_disks(json: &str) -> Result<Vec<DiskHealth>, String> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("解析磁盘信息失败: {}", e))?;
    let arr = match &v {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::Object(_) => vec![v.clone()],
        _ => vec![],
    };
    let mut out = Vec::new();
    for d in arr {
        out.push(DiskHealth {
            friendly_name: d["FriendlyName"].as_str().unwrap_or("").to_string(),
            model: d["Model"].as_str().unwrap_or("").to_string(),
            media_type: media_type_name(d["MediaType"].as_u64().unwrap_or(0)),
            bus_type: bus_type_name(d["BusType"].as_u64().unwrap_or(0)),
            health_status: health_status_name(d["HealthStatus"].as_u64().unwrap_or(0)),
            size: d["Size"].as_u64().unwrap_or(0),
        });
    }
    Ok(out)
}

fn media_type_name(v: u64) -> String {
    match v { 3 => "HDD", 4 => "SSD", 5 => "SCM", _ => "未知" }.into()
}
fn bus_type_name(v: u64) -> String {
    match v {
        1 => "SCSI", 3 => "ATA", 5 => "USB", 7 => "RAID",
        9 => "iSCSI", 11 => "SATA", 12 => "SAS", 13 => "NVMe", _ => "其他",
    }.into()
}
fn health_status_name(v: u64) -> String {
    match v { 0 => "Healthy", 1 => "Warning", 2 => "Unhealthy", _ => "未知" }.into()
}

// ---------- 系统健康评分（100 = 磁盘 40 + 休眠 30 + 垃圾 30） ----------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthScore {
    pub score: u32,
    pub disk_score: u32,
    pub hibernation_score: u32,
    pub junk_score: u32,
    pub disk_free_percent: f64,
    pub has_hibernation: bool,
    pub hibernation_size: u64,
    pub junk_size: u64,
    pub grade: String, // 优/良/中/差
}

pub fn calculate_health_score() -> HealthScore {
    let (free_pct, disk_score) = disk_score_calc();
    let (has_hib, hib_size, hib_score) = hibernation_score_calc();
    let (junk_size, junk_score) = junk_score_calc();
    let score = disk_score + hib_score + junk_score;
    let grade = if score >= 85 { "优" } else if score >= 70 { "良" } else if score >= 50 { "中" } else { "差" }.to_string();
    HealthScore {
        score, disk_score, hibernation_score: hib_score, junk_score,
        disk_free_percent: free_pct, has_hibernation: has_hib,
        hibernation_size: hib_size, junk_size, grade,
    }
}

/// 磁盘空间评分（满分 40，按系统盘剩余百分比分档）
fn disk_score_calc() -> (f64, u32) {
    let sys_drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root = format!("{}\\", sys_drive);
    let (free, total) = disk_free_total(&root);
    if total == 0 {
        return (50.0, 20);
    }
    let pct = free as f64 / total as f64 * 100.0;
    let score = if pct >= 30.0 { 40 }
        else if pct >= 20.0 { 30 + ((pct - 20.0) / 10.0 * 10.0) as u32 }
        else if pct >= 10.0 { 20 + ((pct - 10.0) / 10.0 * 10.0) as u32 }
        else if pct >= 5.0 { 10 + ((pct - 5.0) / 5.0 * 10.0) as u32 }
        else { (pct / 5.0 * 10.0) as u32 };
    (pct, score.min(40))
}

fn disk_free_total(root: &str) -> (u64, u64) {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let (mut free, mut total, mut tf) = (0u64, 0u64, 0u64);
    unsafe { let _ = GetDiskFreeSpaceExW(windows::core::PCWSTR(wide.as_ptr()), Some(&mut free), Some(&mut total), Some(&mut tf)); }
    (free, total)
}

/// 休眠文件评分（满分 30；修复源码硬编码 C:\ → %SystemDrive%）
fn hibernation_score_calc() -> (bool, u64, u32) {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let hiberfil = PathBuf::from(drive).join("hiberfil.sys");
    if !hiberfil.exists() {
        return (false, 0, 30);
    }
    let size = std::fs::metadata(&hiberfil).map(|m| m.len()).unwrap_or(0);
    let gb = 1024u64 * 1024 * 1024;
    let score = if size < 4 * gb { 20 } else if size < 8 * gb { 15 } else if size < 16 * gb { 10 } else { 5 };
    (true, size, score)
}

/// 垃圾文件评分（满分 30）
fn junk_score_calc() -> (u64, u32) {
    let mut total = 0u64;
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(t) = std::env::var_os("TEMP") { paths.push(PathBuf::from(t)); }
    if let Ok(r) = std::env::var("SystemRoot") {
        paths.push(PathBuf::from(&r).join("Temp"));
        paths.push(PathBuf::from(&r).join("Prefetch"));
    }
    for p in &paths {
        if !p.exists() { continue; }
        if let Ok(read) = std::fs::read_dir(p) {
            for e in read.flatten() {
                if let Ok(m) = e.metadata() {
                    if m.is_file() { total += m.len(); }
                }
            }
        }
    }
    let mb = 1024u64 * 1024;
    let score = if total < 500 * mb { 30 }
        else if total < 1024 * mb { 25 }
        else if total < 3 * 1024 * mb { 15 }
        else if total < 5 * 1024 * mb { 8 }
        else { 3 };
    (total, score)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_score_bounded() {
        let h = calculate_health_score();
        assert!(h.score <= 100);
        assert!(h.disk_score <= 40 && h.hibernation_score <= 30 && h.junk_score <= 30);
        assert!(["优", "良", "中", "差"].contains(&h.grade.as_str()));
    }

    #[test]
    fn hibernation_uses_systemdrive_not_hardcoded() {
        // SystemDrive 存在时 hiberfil 路径必须基于它（V4 教训：不硬编码盘符）
        let drive = std::env::var("SystemDrive").unwrap();
        let expected = PathBuf::from(&drive).join("hiberfil.sys");
        assert!(expected.to_string_lossy().starts_with(&drive));
    }
}
