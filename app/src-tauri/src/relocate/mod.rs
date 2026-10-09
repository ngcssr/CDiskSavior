//! M4 空间腾挪：AppData 大文件夹迁移 + Junction + 占用解除 + 回滚
//!
//! P2 阶段实现，按设计文档 §4.3 + §7 R2/R3/R9 全部安全要求。

pub mod config;
pub mod history;
pub mod junction;
pub mod mover;
pub mod proc_mgr;
pub mod scanner;
pub mod validate;

use serde::Serialize;

/// 迁移请求（前端 → 后端）
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRequest {
    /// 要迁移的源目录列表（绝对路径）
    pub sources: Vec<String>,
    /// 迁移根目录（如 G:\CDisk Savior）
    pub target_root: String,
    /// 是否自动结束占用进程（用户已逐个确认过的 PID 列表）
    pub kill_pids: Option<Vec<u32>>,
}

/// 迁移进度事件（后端 → 前端，app.emit）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum MigrationProgress {
    /// 复制进度
    #[serde(rename = "progress")]
    Progress {
        source: String,
        bytes_done: u64,
        bytes_total: u64,
        speed_bps: u64,
    },
    /// 单个源目录完成
    #[serde(rename = "item-done")]
    ItemDone { source: String, freed: u64 },
    /// 单个源目录跳过（reparse point 等，必须明示）
    #[serde(rename = "item-skipped")]
    ItemSkipped { source: String, reason: String },
    /// 整批完成
    #[serde(rename = "done")]
    Done { total_freed: u64, records: Vec<history::MigrationRecord> },
    /// 出错
    #[serde(rename = "error")]
    Error { message: String },
    /// 被取消
    #[serde(rename = "cancelled")]
    Cancelled { partial_cleanup: String },
}

/// 迁移控制信号（AtomicU8 暂停点检查）
pub enum Control {
    Running,
    Paused,
    Cancelled,
}
