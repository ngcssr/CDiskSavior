//! 联网更新（GitHub Releases 更新源）——检测 + 下载 + 自替换完整链路
//!
//! 参考 AntigravityTools 的更新机制，适配编译型 exe（无源码增量包，仅完整包）。
//!
//! latest.json 托管在 GitHub Releases（或 raw），格式：
//! ```json
//! {
//!   "version": 130,
//!   "minimum_version": 120,
//!   "url": "https://github.com/OWNER/REPO/releases/latest",
//!   "download_url": "https://github.com/OWNER/REPO/releases/download/v130/CDiskSavior-Portable.exe",
//!   "sha256": "hex...",
//!   "notes": "..."
//! }
//! ```
//! version 用 git 提交序号（与发布产物 v{N}-{hash} 一致，build.rs 注入 GIT_VERSION）。

use serde::Deserialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

/// 编译时注入的 git 提交序号（build.rs）
fn current_version() -> u32 {
    env!("GIT_VERSION").parse().unwrap_or(0)
}

/// 更新清单地址 —— 改成你的 GitHub 仓库（raw 或 releases 直链 latest.json）
const UPDATE_MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/OWNER/REPO/main/latest.json";

const MANIFEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);

#[derive(Debug, Clone, Deserialize)]
struct UpdateManifest {
    version: u32,
    #[serde(default)]
    minimum_version: u32,
    #[serde(default)]
    url: String,
    /// 自动下载的完整包 URL（可选；缺省则只能引导手动下载）
    #[serde(default)]
    download_url: String,
    /// 完整包 SHA256（可选；缺省跳过校验，宽松策略）
    #[serde(default)]
    sha256: String,
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
    pub download_url: String,
    pub sha256: String,
    pub notes: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadResult {
    pub ok: bool,
    pub path: String,
    pub sha256_ok: bool,
    pub error: Option<String>,
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
                download_url: m.download_url,
                sha256: m.sha256,
                notes: m.notes,
            })
        }
        None => Ok(UpdateInfo {
            has_update: false,
            latest_version: current_version(),
            current_version: current_version(),
            force: false,
            url: String::new(),
            download_url: String::new(),
            sha256: String::new(),
            notes: String::new(),
        }),
    }
}

/// 下载更新包到临时目录，带进度事件 + SHA256 校验 + URL 白名单
#[tauri::command]
pub fn download_update(app: AppHandle, url: String, sha256: String) -> Result<DownloadResult, String> {
    // URL 白名单：仅接受 http(s)，且以可执行/压缩扩展名结尾（拒绝指向 HTML 页面的错误配置）
    if !is_safe_download_url(&url) {
        return Ok(DownloadResult {
            ok: false,
            path: String::new(),
            sha256_ok: false,
            error: Some("下载地址不合法（须为 http(s) 且指向可执行文件）".into()),
        });
    }

    let agent = ureq::AgentBuilder::new().timeout(DOWNLOAD_TIMEOUT).build();
    let resp = agent
        .get(&url)
        .set("User-Agent", "CDiskSavior-updater")
        .call()
        .map_err(|e| format!("下载失败: {e}"))?;

    let total = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let mut reader = resp.into_reader();

    let dir = update_temp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let path = dir.join("CDiskSavior-Portable.exe.new");

    let mut file = std::fs::File::create(&path).map_err(|e| format!("创建临时文件失败: {e}"))?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;

    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("下载中断: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| format!("写入失败: {e}"))?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        if downloaded % (1024 * 1024) == 0 || downloaded == total {
            let pct = if total > 0 {
                (downloaded as f64 / total as f64 * 100.0) as u32
            } else {
                0
            };
            let _ = app.emit(
                "update:download-progress",
                serde_json::json!({ "downloaded": downloaded, "total": total, "percent": pct }),
            );
        }
    }

    // SHA256 校验（无 sha256 时跳过，宽松策略）
    let sha256_ok = if sha256.is_empty() {
        true
    } else {
        let digest = hasher.finalize();
        let hex: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
        hex.eq_ignore_ascii_case(&sha256)
    };

    if !sha256_ok {
        let _ = std::fs::remove_file(&path);
        return Ok(DownloadResult {
            ok: false,
            path: String::new(),
            sha256_ok: false,
            error: Some("SHA256 校验失败，下载包可能损坏或被篡改".into()),
        });
    }

    Ok(DownloadResult {
        ok: true,
        path: path.to_string_lossy().to_string(),
        sha256_ok,
        error: None,
    })
}

/// 应用更新：写 PowerShell 自替换脚本并执行，主程序随即退出
///
/// 脚本职责：等待旧进程退出 → 备份旧 exe → 用 .new 覆盖 → 校验 → 重启 / 失败回滚
#[tauri::command]
pub fn apply_update(app: AppHandle, new_path: String) -> Result<(), String> {
    let current_exe = std::env::current_exe().map_err(|e| format!("无法定位当前程序: {e}"))?;
    let new_path = PathBuf::from(new_path);

    if !new_path.exists() {
        return Err("更新包不存在".into());
    }

    let dir = update_temp_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let script = dir.join("apply.ps1");

    let ps1 = format!(
        r#"$ErrorActionPreference = 'Stop'
$exe = '{exe}'
$new = '{new}'
$bak = '{bak}'

# 1. 等待旧进程退出（最多 60 秒）
$deadline = (Get-Date).AddSeconds(60)
while ((Get-Date) -lt $deadline) {{
    $running = Get-Process -Name 'cdisk-savior' -ErrorAction SilentlyContinue
    if (-not $running) {{ break }}
    Start-Sleep -Milliseconds 500
}}

# 2. 备份旧 exe
if (Test-Path $exe) {{
    Copy-Item $exe $bak -Force
}}

# 3. 替换
try {{
    Copy-Item $new $exe -Force
    Remove-Item $new -Force
}} catch {{
    if (Test-Path $bak) {{ Copy-Item $bak $exe -Force }}
    exit 1
}}

# 4. 重启应用
Start-Process $exe
Remove-Item $bak -Force -ErrorAction SilentlyContinue
exit 0
"#,
        exe = current_exe.to_string_lossy().replace('\\', "\\\\").replace('\'', "''"),
        new = new_path.to_string_lossy().replace('\\', "\\\\").replace('\'', "''"),
        bak = current_exe
            .with_extension("exe.bak")
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('\'', "''"),
    );

    std::fs::write(&script, ps1).map_err(|e| format!("写替换脚本失败: {e}"))?;

    // 启动独立 PowerShell 进程执行脚本，然后主程序退出
    let _ = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &format!(
                "Start-Process powershell -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','{}' -WindowStyle Hidden",
                script.to_string_lossy().replace('\'', "''")
            ),
        ])
        .spawn();

    // 立即退出主程序，让脚本接管替换
    app.exit(0);
    Ok(())
}

fn is_safe_download_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && (lower.ends_with(".exe") || lower.ends_with(".zip") || lower.ends_with(".msi"))
}

fn update_temp_dir() -> PathBuf {
    std::env::temp_dir().join("cdisk_update")
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
