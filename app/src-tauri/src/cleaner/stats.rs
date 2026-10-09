//! 清理统计台账（V19 原子写 + R8 损坏自愈）

use crate::state;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupStats {
    pub total_freed: u64,
    pub total_runs: u32,
    pub last_at: u64,
}

impl Default for CleanupStats {
    fn default() -> Self {
        Self { total_freed: 0, total_runs: 0, last_at: 0 }
    }
}

pub fn stats_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok()?;
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("cleanup-stats.json"))
}

pub fn load(app: &tauri::AppHandle) -> CleanupStats {
    if let Some(file) = stats_file(app) {
        match state::read_json_self_heal::<CleanupStats>(&file) {
            Ok(Some((stats, recovered))) => {
                if recovered {
                    log::warn!("清理统计从 .bak 恢复");
                }
                stats
            }
            _ => CleanupStats::default(),
        }
    } else {
        CleanupStats::default()
    }
}

pub fn record(app: &tauri::AppHandle, freed_delta: u64) {
    let mut stats = load(app);
    stats.total_freed += freed_delta;
    stats.total_runs += 1;
    stats.last_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if let Some(file) = stats_file(app) {
        let _ = state::atomic_write_json(&file, &stats);
    }
}
