//! M3 一键清理：统一删除引擎 + 清理项 + 孤儿目录 + 统计
//!
//! 设计文档 §7 R1：四层护栏 + 全局禁用原语清单
//! 设计文档 §6 C1：M3 清理与 M4 迁移互斥（protected_paths）

pub mod engine;
pub mod items;
pub mod orphans;
pub mod stats;
pub mod deep_junk;
pub mod leftovers;
pub mod social;

use serde::Serialize;

/// 清理项（前端展示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size: u64,
    pub path: String,
    pub category: String,
    pub safety: String, // "safe" | "caution" | "keep"
}

/// 清理扫描结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupScan {
    pub items: Vec<CleanupItem>,
    pub total_size: u64,
}

/// 清理执行结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub freed: u64,
    pub free_before: u64,
    pub free_after: u64,
    pub errors: Vec<String>,
    pub skipped: Vec<String>,
}

/// 孤儿扫描结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanScan {
    pub confirmed: Vec<OrphanEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrphanEntry {
    pub path: String,
    pub app_name: String,
    pub size: u64,
    pub note: Option<String>,
}

/// 孤儿清理结果
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidueReport {
    pub freed: u64,
    pub errors: Vec<String>,
}
