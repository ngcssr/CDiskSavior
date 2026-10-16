//! 联网更新检测（GitHub Releases 更新源）
//!
//! 机制：启动/手动触发时 GET 更新清单 latest.json，比较版本号。
//! - 有更新：返回 has_update + 提示信息（用户可稍后）
//! - 强制更新：本地版本 < minimum_version 时 force=true（前端全屏拦截，不可跳过）
//!
//! latest.json 托管在 GitHub Releases（或 raw），格式：
//! ```json
//! { "version": 129, "minimum_version": 120, "url": "https://github.com/OWNER/REPO/releases/latest", "notes": "..." }
//! ```
//! version 用 git 提交序号（与发布产物 v{N}-{hash} 一致，见 build.rs GIT_VERSION）。

use serde::Deserialize;

/// 编译时注入的 git 提交序号（build.rs）
fn current_version() -> u32 {
    env!("GIT_VERSION").parse().unwrap_or(0)
}

/// 更新清单地址 —— 改成你的 GitHub 仓库（raw 或 releases 直链 latest.json）
const UPDATE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/OWNER/REPO/main/latest.json";

const MANIFEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, Deserialize)]
struct UpdateManifest {
    version: u32,
    #[serde(default)]
    minimum_version: u32,
    #[serde(default)]
    url: String,
    #[serde(default)]
    notes: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateInfo {
    pub has_update: bool,
    pub latest_version: u32,
    pub current_version: u32,
    /// 强制更新（低于 minimum_version 不可跳过）
    pub force: bool,
    pub url: String,
    pub notes: String,
}

/// 当前版本号（git 提交序号）
#[tauri::command]
pub fn get_current_version() -> u32 {
    current_version()
}

/// 检查更新：拉取清单并比较版本。失败返回 Ok(无更新) 而非 Err——离线/网络异常不打扰用户。
#[tauri::command]
pub fn check_update() -> Result<UpdateInfo, String> {
    match fetch_manifest() {
        Some(m) => {
            let has_update = m.version > current_version();
            let force = has_update && current_version() < m.minimum_version;
            Ok(UpdateInfo {
                has_update,
                latest_version: m.version,
                current_version: current_version(),
                force,
                url: m.url,
                notes: m.notes,
            })
        }
        None => Ok(UpdateInfo {
            has_update: false,
            latest_version: current_version(),
            current_version: current_version(),
            force: false,
            url: String::new(),
            notes: String::new(),
        }),
    }
}

fn fetch_manifest() -> Option<UpdateManifest> {
    let agent = ureq::AgentBuilder::new()
        .timeout(MANIFEST_TIMEOUT)
        .build();
    let body = agent
        .get(UPDATE_MANIFEST_URL)
        .set("User-Agent", "CDiskSavior-updater")
        .call()
        .ok()?
        .into_string()
        .ok()?;
    serde_json::from_str::<UpdateManifest>(&body).ok()
}
