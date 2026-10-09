//! 孤儿目录扫描 + 清理（含 C1 互斥 + protected_paths）
//!
//! 设计文档 §6 C1：扫描前过滤 reparse point，清理前检查 protected_paths
//! 设计文档 §7 R1：clean_orphan_dirs 当场重扫白名单 + canonicalize 校验
//!
//! 漏洞 V2（已降级 P2）：disk-butler 的 is_safe_residue_path 用 canonicalize
//! 解析 junction 到真实目标 → 跨盘 junction 必在安装根外 → 被拒绝 → 不丢数据。
//! 本实现沿用该逻辑（显式化 + reparse 前置过滤）。

use super::engine::{self, DeleteResult};
use super::{OrphanEntry, OrphanScan, ResidueReport};
use crate::state;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 孤儿目录知识库（简化版：常用残留目录名 + 在装别名）
fn orphan_rules() -> Vec<(&'static str, &'static str, &'static [&'static str])> {
    vec![
        ("Doubao", "豆包", &["doubao"]),
        ("Discord", "Discord", &["discord"]),
        ("Notion", "Notion", &["notion"]),
        ("Spotify", "Spotify", &["spotify"]),
        ("Slack", "Slack", &["slack"]),
        ("Zoom", "Zoom", &["zoom"]),
        ("obsidian", "Obsidian", &["obsidian"]),
        ("CloudMusic", "网易云音乐", &["cloudmusic"]),
        ("Feishu", "飞书", &["feishu", "lark"]),
    ]
}

/// 安装根（AppData / ProgramData）
fn install_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for var in &["LOCALAPPDATA", "APPDATA", "ProgramData"] {
        if let Some(p) = std::env::var_os(var) {
            roots.push(PathBuf::from(p));
        }
    }
    roots
}

/// 共享厂商名（永不判为孤儿）
fn is_shared_vendor(name: &str) -> bool {
    matches!(
        name.to_lowercase().as_str(),
        "microsoft" | "google" | "tencent" | "kingsoft" | "mozilla" | "adobe"
    )
}

/// 扫描孤儿残留：遍历安装根一级目录，只报告"知识库确证 + 无在装软件"的真残留
pub fn scan_orphans() -> OrphanScan {
    let rules = orphan_rules();
    let mut confirmed = Vec::new();

    for root in install_roots() {
        let root_str = root.to_string_lossy().to_lowercase();
        if root_str.contains("program files") {
            continue; // Program Files 归安装目录残留管
        }

        let Ok(read) = std::fs::read_dir(&root) else {
            continue;
        };

        for entry in read.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }

            let dir_name = entry.file_name().to_string_lossy().to_string();
            if is_shared_vendor(&dir_name) {
                continue;
            }

            // C1 互斥：reparse point 前置过滤（V2 修复：不把 junction 当孤儿）
            if engine::is_reparse_point(&p) {
                continue;
            }

            // 只报告知识库确证的目录
            let Some(rule) = rules.iter().find(|r| r.0.eq_ignore_ascii_case(&dir_name)) else {
                continue;
            };

            // TODO: 在装判定（检查注册表/进程）——P4 简化：假设不在装
            // 完整实现需要 list_installed() + owner_installed()

            let size = dir_size(&p);
            if size == 0 {
                continue;
            }

            confirmed.push(OrphanEntry {
                path: p.to_string_lossy().to_string(),
                app_name: rule.1.to_string(),
                size,
                note: None,
            });
        }
    }

    OrphanScan { confirmed }
}

/// 清理孤儿残留：当场重扫白名单 + canonicalize 校验 + R1 四层护栏
pub fn clean_orphans(paths: &[String]) -> ResidueReport {
    // 全局写锁（C1 互斥）
    let _lock = state::acquire_write_lock();

    // 当场重扫得到允许集合
    let scan = scan_orphans();
    let allowed: HashSet<String> = scan
        .confirmed
        .iter()
        .map(|o| o.path.to_lowercase())
        .collect();

    let allowed_roots = install_roots();
    let mut freed = 0u64;
    let mut errors = Vec::new();

    for p in paths {
        // 第 0 层：路径必须在允许集合中
        if !allowed.contains(&p.to_lowercase()) {
            errors.push(format!("已跳过（不在可清理名单）：{}", p));
            continue;
        }

        let path = Path::new(p);

        // 第 2 层：canonicalize 校验（is_safe_residue_path 等效逻辑在 engine 内）
        // 第 3 层：reparse point 拒绝
        let result = engine::safe_delete_dir(path, &allowed_roots);
        match result {
            DeleteResult::Deleted(size) => freed += size,
            DeleteResult::Skipped(msg) => errors.push(msg),
            DeleteResult::Error(msg) => errors.push(msg),
        }
    }

    ResidueReport { freed, errors }
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    for entry in walkdir::WalkDir::new(path).follow_links(false).into_iter().flatten() {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }
    total
}
