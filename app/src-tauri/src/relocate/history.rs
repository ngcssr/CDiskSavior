//! V19 修复：迁移历史 + 原子写入 + 损坏自愈
//!
//! 设计文档 §7 R8：状态文件原子写（临时文件 + rename）+ 损坏自愈（禁静默清零）。
//! 迁移历史是 protected_paths 保护名单的唯一数据源——损坏即清零保护。

use crate::state;
use tauri::Manager;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 迁移记录（单条迁移：一个源目录 → 一个目标位置 + Junction）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRecord {
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub target_path: String,
    pub size_bytes: u64,
    pub timestamp: u64,
    pub batch_id: Option<String>,
    pub batch_timestamp: Option<u64>,
}

/// 迁移历史管理器
pub struct HistoryManager {
    file: PathBuf,
}

impl HistoryManager {
    /// 数据目录：跟随 appdata 解析（便携模式自包含于 exe 旁，安装版与 appDataDir 一致）
    pub fn new(_app: &tauri::AppHandle) -> Option<Self> {
        let dir = crate::appdata::data_dir();
        let _ = std::fs::create_dir_all(&dir);
        Some(HistoryManager {
            file: dir.join("migration-history.json"),
        })
    }

    /// 读取全部记录（含损坏自愈）
    pub fn load(&self) -> Vec<MigrationRecord> {
        match state::read_json_self_heal::<Vec<MigrationRecord>>(&self.file) {
            Ok(Some((records, recovered)) ) => {
                if recovered {
                    log::warn!("迁移历史从 .bak 恢复（主文件曾损坏）");
                    // 如果你的日志系统没接 log crate，这里至少不会静默清零
                }
                records
            }
            Ok(None) => {
                // 文件不存在或两边都坏 → 返回空（首次使用或严重损坏）
                // 调用方应提示用户"历史记录不可用"而非假装无事
                Vec::new()
            }
            Err(e) => {
                log::error!("读取迁移历史失败: {}", e);
                Vec::new()
            }
        }
    }

    /// 添加一条记录（原子写入）
    pub fn add(&self, record: MigrationRecord) {
        let mut records = self.load();
        records.push(record);
        self.save_all(&records);
    }

    /// 删除一条记录（原子写入）
    pub fn remove(&self, id: &str) {
        let mut records = self.load();
        records.retain(|r| r.id != id);
        self.save_all(&records);
    }

    /// 保存全部记录（原子写入）
    fn save_all(&self, records: &[MigrationRecord]) {
        if let Err(e) = state::atomic_write_json(&self.file, &records.to_vec()) {
            log::error!("写入迁移历史失败（原子写）: {}", e);
        }
    }

    /// 获取 protected_paths：所有现存 junction 的源路径（用于 M3 清理互斥）
    #[allow(dead_code)] // P9: M3 清理时使用
    pub fn protected_paths(&self) -> Vec<String> {
        self.load()
            .iter()
            .filter(|r| {
                // 仅返回源路径上仍有 junction 的记录
                crate::relocate::junction::is_junction(std::path::Path::new(&r.source_path))
            })
            .map(|r| r.source_path.clone())
            .collect()
    }
}
