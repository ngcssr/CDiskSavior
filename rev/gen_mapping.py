# -*- coding: utf-8 -*-
"""生成三源码合并的命令映射表（CSV + Markdown）"""
import csv, os, json

OUT = os.path.dirname(os.path.abspath(__file__))

# 统一模块
MODULES = {
    "dashboard": "M1 总览",
    "insight":   "M2 磁盘透视",
    "clean":     "M3 一键清理",
    "relocate":  "M4 空间腾挪",
    "slim":      "M5 系统瘦身",
    "tune":      "M6 启动与内存",
    "advanced":  "M7 高级维护",
    "settings":  "M8 设置与工具",
}

# (来源, 原命令, 统一模块, 统一命令名, 处置, 说明)
# 处置: keep=原名保留 / rename=改名 / merge=语义重复合并
R = []
def add(src, cmd, mod, uni, disp, note=""):
    R.append((src, cmd, MODULES[mod], uni, disp, note))

# ---------------- light-c (84) ----------------
LC = [
    # 总览
    ("get_disk_info","dashboard","get_disk_info","keep","卷容量/文件系统"),
    ("get_disk_health","dashboard","get_disk_health","keep","SMART/健康"),
    ("get_health_score","dashboard","get_health_score","keep","综合健康评分"),
    ("get_system_info","dashboard","get_system_info","keep","OS/CPU/内存概览"),
    ("get_local_drives","dashboard","get_drives","merge","与 disk-butler 的 get_drives 语义重复 → 合并为一个驱动列表接口"),
    # 磁盘透视
    ("scan_disk_growth","insight","scan_disk_growth","keep","快照对比增长"),
    ("cancel_disk_growth_scan","insight","cancel_disk_growth_scan","keep",""),
    ("get_disk_growth_directory_details","insight","get_disk_growth_directory_details","keep",""),
    ("get_disk_growth_file_details","insight","get_disk_growth_file_details","keep",""),
    ("get_disk_growth_export_tree","insight","get_disk_growth_export_tree","keep",""),
    ("save_disk_growth_html","insight","save_disk_growth_html","keep","导出 HTML"),
    ("scan_hotspot","insight","scan_hotspot","keep","桌面/下载等热点目录"),
    ("cancel_hotspot_scan","insight","cancel_hotspot_scan","keep",""),
    ("scan_large_files","insight","scan_large_files","keep","大文件 Top-N"),
    ("cancel_large_file_scan","insight","cancel_large_file_scan","keep",""),
    ("scan_path_direct","insight","scan_path_direct","keep","指定路径直接扫描"),
    # 一键清理
    ("scan_junk_files","clean","scan_junk_files","keep","常规垃圾"),
    ("scan_deep_junk_files","clean","scan_deep_junk_files","keep","深度垃圾"),
    ("cancel_deep_junk_scan","clean","cancel_deep_junk_scan","keep",""),
    ("get_deep_junk_category_page","clean","get_deep_junk_category_page","keep","分类分页"),
    ("scan_category","clean","scan_category","keep","按分类扫描"),
    ("get_categories","clean","get_categories","keep",""),
    ("scan_social_cache","clean","scan_social_cache","keep","10 款社交/IM 缓存"),
    ("scan_uninstall_leftovers","clean","scan_uninstall_leftovers","keep","卸载残留"),
    ("check_leftover_safety","clean","check_leftover_safety","keep","残留安全性判定"),
    ("list_leftover_whitelist","clean","list_leftover_whitelist","keep",""),
    ("add_leftover_whitelist_entry","clean","add_leftover_whitelist_entry","keep",""),
    ("remove_leftover_whitelist_entry","clean","remove_leftover_whitelist_entry","keep",""),
    ("delete_files","clean","delete_files","keep","基础删除"),
    ("enhanced_delete_files","clean","enhanced_delete_files","keep","强化删除(解占用)"),
    ("delete_deep_junk_files","clean","delete_deep_junk_files","keep",""),
    ("delete_leftover_folders","clean","delete_leftover_folders","keep",""),
    ("delete_leftovers_permanent","clean","delete_leftovers_permanent","keep","**永久删除**"),
    ("cleanup_directory_contents","clean","cleanup_directory_contents","keep","清空目录内容"),
    ("get_cleanup_history","clean","get_cleanup_stats","merge","与 disk-butler 的 get_cleanup_stats 语义重复 → 统一为一份清理台账"),
    ("record_cleanup_action","clean","record_cleanup_action","keep",""),
    ("open_recycle_bin","clean","open_recycle_bin","merge","**唯一同名冲突**：与 disk-butler 完全同义 → 保留一份实现"),
    # 系统瘦身
    ("get_system_slim_status","slim","get_system_slim_status","keep","休眠/WinSxS 现状"),
    ("enable_hibernation","slim","enable_hibernation","keep",""),
    ("disable_hibernation","slim","disable_hibernation","keep",""),
    ("cleanup_winsxs","slim","cleanup_winsxs","keep","DISM 组件清理"),
    ("cleanup_winsxs_resetbase","slim","cleanup_winsxs_resetbase","keep","ResetBase 不可逆"),
    ("rebuild_search_index","slim","rebuild_search_index","keep",""),
    ("open_virtual_memory_settings","slim","open_virtual_memory_settings","keep",""),
    ("open_storage_settings","slim","open_storage_settings","keep",""),
    ("open_disk_cleanup","slim","open_disk_cleanup","keep","调起系统 cleanmgr"),
    ("scan_old_drivers","slim","scan_old_drivers","keep","旧驱动备份"),
    ("delete_old_drivers","slim","delete_old_drivers","keep",""),
    ("restore_all_driver_backups","slim","restore_all_driver_backups","keep",""),
    ("open_driver_backup_dir","slim","open_driver_backup_dir","keep",""),
    ("scan_registry_redundancy","slim","scan_registry_redundancy","keep","注册表冗余"),
    ("delete_registry_entries","slim","delete_registry_entries","keep","**高风险**"),
    ("open_registry_backup_dir","slim","open_registry_backup_dir","keep",""),
    ("scan_shell_icons","slim","scan_shell_icons","keep","图标缓存"),
    ("remove_shell_icon","slim","remove_shell_icon","keep",""),
    ("restore_shell_icon","slim","restore_shell_icon","keep",""),
    ("unlock_shell_icon","slim","unlock_shell_icon","keep",""),
    ("open_shell_icon_backup_dir","slim","open_shell_icon_backup_dir","keep",""),
    ("open_shell_icon_log","slim","open_shell_icon_log","keep",""),
    ("open_shell_icon_registry","slim","open_shell_icon_registry","keep",""),
    ("restart_explorer","slim","restart_explorer","keep",""),
    ("scan_context_menu","slim","scan_context_menu","keep","右键菜单项"),
    ("delete_context_menu_entries","slim","delete_context_menu_entries","keep",""),
    ("scan_ai_model_assets","slim","scan_ai_model_assets","keep","AI 模型资产"),
    ("delete_ai_model","slim","delete_ai_model","keep",""),
    # 启动与内存
    ("open_startup_manager","tune","open_startup_manager","keep","调起系统启动项管理"),
    # 工具/设置
    ("check_admin_privilege","settings","check_admin_privilege","keep",""),
    ("check_admin_for_path","settings","check_admin_for_path","keep",""),
    ("get_data_directory","settings","get_data_directory","keep","应用自身数据目录"),
    ("set_data_directory","settings","set_data_directory","keep",""),
    ("get_storage_location_info","settings","get_storage_location_info","keep",""),
    ("get_storage_write_diagnostic","settings","get_storage_write_diagnostic","keep",""),
    ("list_clearable_data_items","settings","list_clearable_data_items","keep",""),
    ("clear_local_data","settings","clear_local_data","keep",""),
    ("clear_selected_local_data","settings","clear_selected_local_data","keep",""),
    ("migrate_legacy_portable_data","settings","migrate_legacy_portable_data","keep",""),
    ("get_distribution_channel","settings","get_distribution_channel","keep",""),
    ("open_logs_folder","settings","open_logs_folder","keep",""),
    ("pick_folder_dialog","settings","pick_folder_dialog","keep","可改用 tauri-plugin-dialog"),
    ("open_file","settings","open_file","keep","可改用 tauri-plugin-opener"),
    ("open_in_folder","settings","reveal_in_folder","merge","与 tauri-plugin-opener 的 reveal_item_in_dir 重复 → 统一走插件"),
    ("format_size","settings","format_size","keep","建议下沉到前端工具函数"),
    ("get_physical_size","settings","get_physical_size","keep",""),
    ("verify_integrity","settings","verify_integrity","keep","自检"),
]
for c,m,u,d,n in LC: add("light-c",c,m,u,d,n)

# ---------------- disk-butler (23) ----------------
DB = [
    ("get_drives","dashboard","get_drives","merge","驱动列表合并落点（保留本实现）"),
    ("memory_report","dashboard","memory_report","keep","进程内存排行"),
    ("pagefile_check","dashboard","pagefile_check","keep","页面文件一致性"),
    ("scan_drive","insight","scan_drive","keep","TreeMap 主扫描"),
    ("scan_dir","insight","scan_dir","keep","下钻"),
    ("load_scan_cache","insight","load_scan_cache","keep","秒显上次结果"),
    ("list_cleanup_items","clean","list_cleanup_items","keep","三层策略清单"),
    ("run_cleanup","clean","run_cleanup","keep","一键清理"),
    ("scan_orphan_dirs","clean","scan_orphan_dirs","keep","孤儿 AppData 目录"),
    ("clean_orphan_dirs","clean","clean_orphan_dirs","keep","**与 M4 迁移互斥，见冲突 C1**"),
    ("get_cleanup_stats","clean","get_cleanup_stats","merge","清理台账合并落点"),
    ("list_startup_items","tune","list_startup_items","keep","启动项枚举"),
    ("set_startup_enabled","tune","set_startup_enabled","keep","启用/禁用"),
    ("analyze_system_clean","advanced","analyze_system_clean","keep","DISM 只读分析"),
    ("run_deep_analyze","advanced","run_deep_analyze","keep","深度分析"),
    ("run_deep_clean","advanced","run_deep_clean","keep",""),
    ("run_deep_clean_system","advanced","run_deep_clean_system","keep","需 UAC"),
    ("collect_rules","advanced","collect_rules","keep","规则采集报告"),
    ("scan_service_available","settings","scan_service_available","keep","扫描服务状态"),
    ("repair_scan_service","settings","repair_scan_service","keep","一键修复服务"),
    ("open_apps_settings","settings","open_apps_settings","keep",""),
    ("set_always_on_top","settings","set_always_on_top","keep","窗口置顶"),
    ("open_recycle_bin","clean","open_recycle_bin","merge","同名冲突，保留一份"),
]
for c,m,u,d,n in DB: add("disk-butler",c,m,u,d,n)

# ---------------- 新增：WindowsClear 移植 (eframe→Tauri 命令) ----------------
NEW = [
    ("scan_relocate_targets","relocate","scan_relocate_targets","new","扫描大文件夹（源: core/scanner.rs scan_large_folders）"),
    ("list_locking_processes","relocate","list_locking_processes","new","检查占用（core/proc_mgr.rs check_locking_processes*，RestartManager）"),
    ("kill_locking_process","relocate","kill_locking_process","new","结束占用进程（core/proc_mgr.rs kill_process）"),
    ("start_migration","relocate","start_migration","new","迁移+建 Junction（core/mover.rs move_and_link）"),
    ("pause_migration","relocate","pause_migration","new","暂停（egui 主线程状态机需改为后端任务）"),
    ("resume_migration","relocate","resume_migration","new","继续"),
    ("cancel_migration","relocate","cancel_migration","new","取消"),
    ("restore_migration","relocate","restore_migration","new","回滚（core/mover.rs restore_migration）"),
    ("list_migration_history","relocate","list_migration_history","new","迁移历史（core/history.rs）"),
    ("remove_migration_record","relocate","remove_migration_record","new",""),
    ("get_relocate_config","relocate","get_relocate_config","new","读写现有 config.json 格式"),
    ("save_relocate_config","relocate","save_relocate_config","new","**必须兼容用户已有的 target_root=G:\\Yugongyipan 配置**"),
]
for c,m,u,d,n in NEW: add("WindowsClear",c,m,u,d,n)

# 输出 CSV
csv_path = os.path.join(OUT, "命令映射表.csv")
with open(csv_path, "w", newline="", encoding="utf-8-sig") as f:
    w = csv.writer(f)
    w.writerow(["来源", "原命令", "统一模块", "统一命令名", "处置", "说明"])
    w.writerows(R)

# 统计
from collections import Counter
by_src = Counter(r[0] for r in R)
by_mod = Counter(r[2] for r in R)
disp   = Counter(r[4] for r in R)
uniq   = {r[3] for r in R}
merged = [r for r in R if r[4] == "merge"]

print(f"总条目 {len(R)}  唯一统一命令 {len(uniq)}")
print("按来源:", dict(by_src))
print("按处置:", dict(disp))
print("按模块:")
for k in sorted(by_mod): print(f"   {k:16s} {by_mod[k]:3d}")
print("\n合并项:")
for r in merged: print(f"   {r[0]:12s} {r[1]:26s} -> {r[3]}   ({r[5]})")
print("\nCSV ->", csv_path)

# 生成 Markdown 片段（按模块分组）
md = []
for k in sorted(MODULES):
    name = MODULES[k]
    rows = [r for r in R if r[2] == name]
    md.append(f"\n### {name}（{len(rows)} 条）\n")
    md.append("| 来源 | 原命令 | 统一命令名 | 处置 | 说明 |")
    md.append("| --- | --- | --- | --- | --- |")
    for r in rows:
        md.append(f"| {r[0]} | `{r[1]}` | `{r[3]}` | {r[4]} | {r[5]} |")
open(os.path.join(OUT, "命令映射表.md"), "w", encoding="utf-8").write("\n".join(md))
print("MD  ->", os.path.join(OUT, "命令映射表.md"))
