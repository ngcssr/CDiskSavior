fn main() {
    // R9/V20：自定义应用清单（longPathAware + PerMonitorV2 + Common Controls v6），
    // 走 tauri-build 的 WindowsAttributes::app_manifest 通道
    // （embed-manifest 会与 tauri 内嵌清单资源冲突；Attributes::app_manifest 是 ACL 用的）
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("cdisksavior.manifest"));
    let attrs = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attrs).expect("tauri build failed");
}
