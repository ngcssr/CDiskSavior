fn main() {
    // 注入 git 提交序号作为应用版本号（与发布产物命名 v{N}-{hash} 一致，§14 版本化）
    // 编译时读取，联网更新检测用它比较 latest.json 的 version 字段
    let git_count = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());
    println!("cargo:rustc-env=GIT_VERSION={}", git_count);

    // R9/V20：自定义应用清单（longPathAware + PerMonitorV2 + Common Controls v6），
    // 走 tauri-build 的 WindowsAttributes::app_manifest 通道
    // （embed-manifest 会与 tauri 内嵌清单资源冲突；Attributes::app_manifest 是 ACL 用的）
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("cdisksavior.manifest"));
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("tauri build failed");

    // C 级防逆向：PE 缓解（CFG 控制流防护；msvc 链接器）
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        println!("cargo:rustc-link-arg=/GUARD:CF");
        // 移除 CodeView 调试目录（strip=symbols 后仍会残留 pdb 文件名引用）
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
    }
}
