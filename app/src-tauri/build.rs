fn main() {
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
