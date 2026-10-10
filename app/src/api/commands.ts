/**
 * CDisk Savior · 统一命令接口层
 *
 * 设计文档 §4 的 116 个统一命令按模块分组。
 * P1 骨架阶段：后端已注册 23 条（disk-butler 公开集），
 * 其余命令调用会得到后端的"P1 骨架占位"错误——前端据此渲染占位状态。
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import type { ModuleId } from "../store";

/** 统一 invoke：透传 Tauri IPC */
async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return tauriInvoke<T>(cmd, args);
}

// ============ M1 总览（8） ============
export const api_dashboard = {
  get_drives: () => invoke<DriveInfo[]>("get_drives"),
  get_disk_info: () => invoke<DiskInfo>("get_disk_info"),
  get_disk_health: () => invoke<DiskHealth[]>("get_disk_health"),
  get_health_score: () => invoke<HealthScore>("get_health_score"),
  get_system_info: () => invoke<SystemInfo>("get_system_info"),
  memory_report: () => invoke<MemoryReport>("memory_report"),
  pagefile_check: () => invoke<PagefileCheck>("pagefile_check"),
};

// ============ M2 磁盘透视（14） ============
export const api_insight = {
  scan_drive: (root: string) => invoke<TreeNode>("scan_drive", { root }),
  scan_dir: (path: string) => invoke<TreeNode>("scan_dir", { path }),
  load_scan_cache: (root: string) => invoke<ScanCache | null>("load_scan_cache", { root }),
  scan_disk_growth: (drive: string, maxEntries?: number) => invoke<DiskGrowthScanResponse>("scan_disk_growth", { drive, max_change_entries: maxEntries }),
  cancel_disk_growth_scan: () => invoke<void>("cancel_disk_growth_scan"),
  get_disk_growth_directory_details: (p: string, offset?: number, limit?: number) => invoke<GrowthDirDetails>("get_disk_growth_directory_details", { path: p, offset, limit }),
  get_disk_growth_file_details: (p: string, offset?: number, limit?: number, drive?: string) => invoke<GrowthFileDetails>("get_disk_growth_file_details", { path: p, offset, limit, drive }),
  get_disk_growth_export_tree: (paths: string[], maxDepth?: number) => invoke<GrowthTree>("get_disk_growth_export_tree", { paths, max_depth: maxDepth }),
  save_disk_growth_html: (content: string, name: string) => invoke<string | null>("save_disk_growth_html", { content, default_file_name: name, dialog_title: "导出增长报告" }),
  scan_hotspot: (path?: string) => invoke<HotspotReport>("scan_hotspot", { path }),
  cancel_hotspot_scan: () => invoke<void>("cancel_hotspot_scan"),
  create_disk_snapshot: (drive: string) => invoke<number>("create_disk_snapshot", { drive }),
  compare_disk_growth: (drive: string) => invoke<GrowthReport>("compare_disk_growth", { drive }),
  has_disk_snapshot: (drive: string) => invoke<boolean>("has_disk_snapshot", { drive }),
  scan_large_files: (root: string, minSize?: number) => invoke<LargeFileReport>("scan_large_files", { root, min_size: minSize }),
  cancel_large_file_scan: () => invoke<void>("cancel_large_file_scan"),
  scan_path_direct: (p: string) => invoke<TreeNode>("scan_path_direct", { path: p }),
};

// ============ M3 一键清理（27） ============
export const api_clean = {
  list_cleanup_items: () => invoke<CleanupScan>("list_cleanup_items"),
  run_cleanup: (ids: string[]) => invoke<CleanupReport>("run_cleanup", { ids }),
  get_cleanup_stats: () => invoke<CleanupStats>("get_cleanup_stats"),
  scan_orphan_dirs: () => invoke<OrphanScan>("scan_orphan_dirs"),
  clean_orphan_dirs: (paths: string[]) => invoke<ResidueReport>("clean_orphan_dirs", { paths }),
  // 深度垃圾扫描（light-c deep_junk 移植）
  scan_deep_junk_files: () => invoke<DeepScanResult>("scan_deep_junk_files"),
  cancel_deep_junk_scan: () => invoke<void>("cancel_deep_junk_scan"),
  delete_deep_junk_files: (ids: string[]) => invoke<DeepDeleteReport>("delete_deep_junk_files", { ids }),
  scan_junk_files: (categories?: string[]) => invoke<JunkReport>("scan_junk_files", { categories }),
  get_deep_junk_category_page: (cat: string, offset?: number, limit?: number) => invoke<DeepJunkPage>("get_deep_junk_category_page", { category: cat, offset, limit }),
  scan_category: (cat: string) => invoke<CategoryReport>("scan_category", { category: cat }),
  get_categories: () => invoke<Category[]>("get_categories"),
  scan_social_cache: () => invoke<SocialScan>("scan_social_cache"),
  delete_social_cache: (app: string, categories: string[]) => invoke<SocialDeleteReport>("delete_social_cache", { app, categories }),
  scan_uninstall_leftovers: () => invoke<LeftoverScan>("scan_uninstall_leftovers"),
  check_leftover_safety: (p: string) => invoke<LeftoverSafety>("check_leftover_safety", { path: p }),
  delete_leftovers: (paths: string[]) => invoke<LeftoverDeleteReport>("delete_leftovers", { paths }),
  list_leftover_whitelist: () => invoke<string[]>("list_leftover_whitelist"),
  add_leftover_whitelist_entry: (e: string) => invoke<void>("add_leftover_whitelist_entry", { entry: e }),
  remove_leftover_whitelist_entry: (e: string) => invoke<void>("remove_leftover_whitelist_entry", { entry: e }),
  delete_files: (paths: string[]) => invoke<DeletePathsReport>("delete_files", { paths }),
  enhanced_delete_files: (paths: string[]) => invoke<DeletePathsReport>("enhanced_delete_files", { paths }),
  delete_leftover_folders: (paths: string[]) => invoke<DeletePathsReport>("delete_leftover_folders", { paths }),
  delete_leftovers_permanent: (paths: string[]) => invoke<DeletePathsReport>("delete_leftovers_permanent", { paths }),
  cleanup_directory_contents: (p: string) => invoke<DeletePathsReport>("cleanup_directory_contents", { path: p }),
  record_cleanup_action: (entries: unknown[]) => invoke<string>("record_cleanup_action", { entries }),
  open_recycle_bin: () => invoke<void>("open_recycle_bin"),
};

// ============ M4 空间腾挪（12，WindowsClear 移植） ============
export const api_relocate = {
  scan_relocate_targets: () => invoke<RelocateTarget[]>("scan_relocate_targets"),
  list_locking_processes: (p: string) => invoke<LockingProcess[]>("list_locking_processes", { path: p }),
  kill_locking_process: (pid: number) => invoke<void>("kill_locking_process", { pid }),
  start_migration: (sources: string[], targetRoot: string, killPids: number[]) =>
    invoke<{ freed: number; records: MigrationRecord[] }>("start_migration", { sources, targetRoot, killPids }),
  pause_migration: () => invoke<void>("pause_migration"),
  resume_migration: () => invoke<void>("resume_migration"),
  cancel_migration: () => invoke<void>("cancel_migration"),
  restore_migration: (recordId: string) => invoke<void>("restore_migration", { recordId }),
  list_migration_history: () => invoke<MigrationRecord[]>("list_migration_history"),
  remove_migration_record: (recordId: string) => invoke<void>("remove_migration_record", { recordId }),
  get_relocate_config: () => invoke<RelocateConfig>("get_relocate_config"),
  save_relocate_config: (cfg: RelocateConfig) => invoke<void>("save_relocate_config", { configValue: cfg }),
};

// ============ M5 系统瘦身（28） ============
export const api_slim = {
  get_system_slim_status: () => invoke<SlimStatus>("get_system_slim_status"),
  enable_hibernation: () => invoke<void>("enable_hibernation"),
  disable_hibernation: () => invoke<void>("disable_hibernation"),
  cleanup_winsxs: () => invoke<DismResult>("cleanup_winsxs"),
  cleanup_winsxs_resetbase: () => invoke<DismResult>("cleanup_winsxs_resetbase"),
  rebuild_search_index: () => invoke<void>("rebuild_search_index"),
  open_virtual_memory_settings: () => invoke<void>("open_virtual_memory_settings"),
  open_storage_settings: () => invoke<void>("open_storage_settings"),
  open_disk_cleanup: () => invoke<void>("open_disk_cleanup"),
  scan_old_drivers: () => invoke<DriverEntry[]>("scan_old_drivers"),
  delete_old_drivers: (ids: string[]) => invoke<DeleteResult>("delete_old_drivers", { ids }),
  restore_all_driver_backups: () => invoke<DriverRestoreResult>("restore_all_driver_backups"),
  open_driver_backup_dir: () => invoke<void>("open_driver_backup_dir"),
  scan_registry_redundancy: () => invoke<RegistryScanResult>("scan_registry_redundancy"),
  delete_registry_entries: (entries: RegistryEntry[]) => invoke<RegistryDeleteResult>("delete_registry_entries", { entries }),
  open_registry_backup_dir: () => invoke<void>("open_registry_backup_dir"),
  scan_shell_icons: () => invoke<ShellIconInfo[]>("scan_shell_icons"),
  remove_shell_icon: (target: ShellIconTarget, mode: number) => invoke<ShellIconOperationResult>("remove_shell_icon", { target, mode }),
  restore_shell_icon: (target: ShellIconTarget) => invoke<ShellIconOperationResult>("restore_shell_icon", { target }),
  unlock_shell_icon: (target: ShellIconTarget) => invoke<ShellIconOperationResult>("unlock_shell_icon", { target }),
  open_shell_icon_backup_dir: () => invoke<void>("open_shell_icon_backup_dir"),
  open_shell_icon_log: () => invoke<void>("open_shell_icon_log"),
  open_shell_icon_registry: (target: ShellIconTarget) => invoke<void>("open_shell_icon_registry", { target }),
  restart_explorer: () => invoke<void>("restart_explorer"),
  scan_context_menu: () => invoke<ContextMenuScanResult>("scan_context_menu"),
  delete_context_menu_entries: (ids: string[]) => invoke<ContextMenuDeleteResult>("delete_context_menu_entries", { ids }),
  scan_ai_model_assets: () => invoke<AiModelEntry[]>("scan_ai_model_assets"),
  delete_ai_model: (id: string) => invoke<DeleteResult>("delete_ai_model", { id }),
};

// ============ M6 启动与内存（3） ============
export const api_tune = {
  list_startup_items: () => invoke<StartupItem[]>("list_startup_items"),
  set_startup_enabled: (id: string, enabled: boolean) => invoke<void>("set_startup_enabled", { id, enabled }),
  open_startup_manager: () => invoke<void>("open_startup_manager"),
};

// ============ M7 高级维护（5） ============
export const api_advanced = {
  analyze_system_clean: () => invoke<{ windowsTemp: { path: string; size: number }; updateDownload: { path: string; size: number }; totalSize: number }>("analyze_system_clean"),
  run_deep_analyze: () => invoke<{ items: unknown[]; totalSize: number }>("run_deep_analyze"),
  run_deep_clean: () => invoke<DeepCleanReport>("run_deep_clean"),
  run_deep_clean_system: () => invoke<SystemCleanReport>("run_deep_clean_system"),
  collect_rules: (includeDrives: boolean) => invoke<CollectResult>("collect_rules", { include_drives: includeDrives }),
};

// ============ M8 设置与工具（22） ============
export const api_settings = {
  check_admin_privilege: () => invoke<boolean>("check_admin_privilege"),
  check_admin_for_path: (p: string) => invoke<boolean>("check_admin_for_path", { path: p }),
  get_data_directory: () => invoke<string>("get_data_directory"),
  set_data_directory: (p: string) => invoke<void>("set_data_directory", { path: p }),
  get_storage_location_info: () => invoke<StorageLocationInfo>("get_storage_location_info"),
  get_storage_write_diagnostic: () => invoke<WriteDiagnostic>("get_storage_write_diagnostic"),
  list_clearable_data_items: () => invoke<ClearableItem[]>("list_clearable_data_items"),
  clear_local_data: () => invoke<ClearLocalDataResult>("clear_local_data"),
  clear_selected_local_data: (keys: string[]) => invoke<ClearLocalDataResult>("clear_selected_local_data", { keys }),
  migrate_legacy_portable_data: () => invoke<{ found: boolean; legacyDir?: string; imported: string[]; skipped: string[]; message?: string }>("migrate_legacy_portable_data"),
  get_distribution_channel: () => invoke<string>("get_distribution_channel"),
  open_logs_folder: () => invoke<void>("open_logs_folder"),
  pick_folder_dialog: (title: string) => invoke<string | null>("pick_folder_dialog", { title }),
  // open_file 走后端命令（R10：排除可执行扩展名）；
  // reveal_in_folder 走 tauri-plugin-opener（映射表处置，纯选择不执行）
  open_file: (p: string) => invoke<void>("open_file", { path: p }),
  reveal_in_folder: (p: string) => revealItemInDir(p),
  format_size: (bytes: number) => invoke<string>("format_size", { bytes }),
  get_physical_size: (p: string) => invoke<number>("get_physical_size", { path: p }),
  verify_integrity: () => invoke<IntegrityReport>("verify_integrity"),
  scan_service_available: () => invoke<boolean>("scan_service_available"),
  repair_scan_service: () => invoke<void>("repair_scan_service"),
  open_apps_settings: () => invoke<void>("open_apps_settings"),
  set_always_on_top: (v: boolean) => invoke<void>("set_always_on_top", { value: v }),
};

export const api: Record<ModuleId, Record<string, (...a: any[]) => any>> = {
  dashboard: api_dashboard,
  insight: api_insight,
  clean: api_clean,
  relocate: api_relocate,
  slim: api_slim,
  tune: api_tune,
  advanced: api_advanced,
  settings: api_settings,
};

// ==== P1 骨架占位类型（接入真实后端时逐个替换为共享类型定义） ====
export type DriveInfo = { mount: string; label: string; total: number; free: number; fs: string; kind: string };
export type DiskInfo = { drives: DriveInfo[] };
export type SystemInfo = {
  os: string; cpu: string; arch: string; memoryTotal: number; memoryAvailable: number;
};
export type MemoryReport = {
  overview: { total: number; available: number; usedPercent: number; swapTotal?: number; swapAvailable?: number };
  processes: unknown[];
};
export type PagefileCheck = unknown;
export type TreeNode = {
  name: string; path: string; size: number; isDir: boolean; hasChildren: boolean;
  category: string; friendlyName?: string; description?: string; safety: string;
  children?: TreeNode[];
};
export type ScanCache = { scannedAt: number; root: string; tree: TreeNode };
// 增长明细与导出
export type DiskGrowthScanResponse = {
  growth: GrowthReport; totalFilesScanned: number; scanDurationMs: number; driveLetter: string;
};
export type GrowthDirDetails = {
  entries: { path: string; name: string; size: number; fileCount: number }[];
  totalCount: number; offset: number; limit: number;
};
export type GrowthFileDetails = {
  entries: { path: string; name: string; size: number; modified: number; isNew: boolean }[];
  totalCount: number; offset: number; limit: number;
};
export type GrowthTreeNode = {
  path: string; name: string; size: number; fileCount: number; children: GrowthTreeNode[];
};
export type GrowthTree = { nodes: GrowthTreeNode[]; maxDepth: number };
export type LargeFileEntry = {
  path: string; name: string; size: number; modified: number; dir: string;
};
export type LargeFileReport = {
  entries: LargeFileEntry[]; totalFound: number; scannedFiles: number;
  durationMs: number; cancelled: boolean; truncated: boolean;
};
export type CleanupScan = { items: CleanupItem[]; totalSize: number };
export type CleanupReport = { freed: number; freeBefore: number; freeAfter: number; errors: string[]; skipped: string[] };
export type CleanupStats = { totalFreed: number; totalRuns: number; lastAt: number };
export type OrphanScan = { confirmed: OrphanEntry[] };
export type ResidueReport = { freed: number; errors: string[] };
export type JunkReport = DeepScanResult;
export type DeepJunkReport = unknown;
export type DeepJunkPage = {
  category: string; totalCount: number; totalSize: number;
  offset: number; limit: number; files: { path: string; size: number }[];
};
export type CategoryReport = DeepCategoryResult;
export type Category = {
  id: string; name: string; description: string; riskLevel: number; systemManaged: boolean;
};
export type SocialCacheReport = unknown;
export type LeftoverReport = unknown;
export type DeleteResult = unknown;
export type DeletePathsReport = {
  successCount: number; failedCount: number; skippedCount: number;
  freed: number; errors: string[]; skipped: string[];
};
export type CleanupAction = unknown;
export type RelocateTarget = { label: string; source: string; targetSubdir: string; size: number; path: string; isJunction: boolean };
export type LockingProcess = { pid: number; name: string; path?: string };
export type MigrationRecord = {
  id: string; name: string; sourcePath: string; targetPath: string;
  sizeBytes: number; timestamp: number; batchId?: string; batchTimestamp?: number;
};
export type RelocateConfig = {
  version: number; targetRoot: string | null;
  scanSources: { enabled: boolean; label: string; path: string; targetSubdir: string }[];
  legacyImported: boolean;
};
export type SlimStatus = {
  hibernationEnabled: boolean; winsxsSize: number | null; searchIndexRunning: boolean;
  oldDriverCount: number; oldDriverTotalSize: number;
  registryRedundantCount: number; aiModelCount: number; aiModelTotalSize: number;
};
export type DismResult = unknown;
export type DriverEntry = { id: string; name: string; date: string; size: number; path: string };
export type DriverRestoreResult = {
  restoredCount: number; failedCount: number; details: string[];
};
// 注册表残留（HKCR\Applications 文件关联残留，铁证条件过滤）
export type RegistryScanResult = {
  entries: RegistryEntry[]; totalCount: number; scanDurationMs: number;
};
export type RegistryEntry = {
  path: string; name: string; associatedPath: string; issue: string;
};
export type RegistryDeleteResult = {
  backupPath: string; deletedCount: number; failedEntries: string[]; errors: string[];
};
// 外壳入口（此电脑/导航窗格第三方入口）
export type ShellEntryKind = "myComputer" | "navigationPane";
export type ShellIconTarget = {
  clsid: string; hive: string; registryView: string; entryKind: ShellEntryKind;
};
export type ShellIconInfo = {
  entryKind: ShellEntryKind; clsid: string; name: string;
  applicationName: string | null; regPath: string; hive: string; registryView: string;
  sourcePath: string | null; riskLevel: string; riskReason: string;
  isSystemProtected: boolean; isLocked: boolean;
};
export type ShellIconOperationResult = {
  success: boolean; message: string; backupPath: string | null; needsExplorerRefresh: boolean;
};
export type ShellIconReport = unknown;
// 右键菜单
export type ContextMenuScanResult = {
  entries: ContextMenuEntry[]; invalidCount: number; scanDurationMs: number;
};
export type ContextMenuEntry = {
  id: string; displayName: string; keyName: string; registryPath: string;
  regRoot: string; regSubpath: string; scope: string;
  iconPath: string | null; command: string | null; exePath: string | null;
  exeExists: boolean; needsAdmin: boolean; isSystemProtected: boolean; riskLevel: string;
};
export type ContextMenuDeleteResult = {
  deletedCount: number; failedCount: number;
  details: { id: string; success: boolean; error: string | null }[];
};
export type ContextMenuReport = unknown;
export type AiModelEntry = { id: string; name: string; path: string; size: number; lastModified: number };
export type StartupItem = {
  id: string; name: string; command: string; location: string;
  enabled: boolean; needsAdmin: boolean; memMb?: number;
  advice: string; reason?: string; icon?: string;
};
export type SystemAnalyzeReport = unknown;
export type DeepAnalyzeReport = unknown;
export type DeepCleanReport = unknown;
export type SystemCleanReport = unknown;
export type CollectResult = {
  mdPath: string; jsonPath: string; softwareCount: number;
  dirRows: number; driveRows: number; elapsedSecs: number;
};
export type StorageLocationInfo = {
  channel: "installer" | "portable"; defaultDir: string; dataDir: string;
  isCustomDir: boolean; configFile: string;
};
export type WriteDiagnostic = {
  dataDirWritable: boolean; configDirWritable: boolean; errors: string[];
};
export type ClearableItem = {
  id: string; name: string; description: string; size: number; fileCount: number;
};
export type ClearLocalDataResult = {
  clearedItems: number; freed: number; errors: string[];
};
export type IntegrityReport = unknown;

export type CleanupItem = {
  id: string; name: string; description: string; size: number;
  path: string; category: string; safety: string;
};
export type OrphanEntry = { path: string; appName: string; size: number; note?: string };

// 深度垃圾扫描类型
export type DeepCategoryResult = {
  id: string; name: string; description: string; risk: number;
  size: number; fileCount: number; systemManaged: boolean;
  topFiles: { path: string; size: number }[];
};
export type DeepScanResult = {
  categories: DeepCategoryResult[]; totalSize: number; totalFiles: number;
  durationMs: number; cancelled: boolean;
};
export type DeepDeleteReport = { freed: number; errors: string[] };

// 卸载残留类型
export type LeftoverEntry = {
  path: string; name: string; size: number; score: number;
  reasons: string[]; lastModifiedDays: number;
};
export type LeftoverScan = { high: LeftoverEntry[]; suspicious: LeftoverEntry[] };
export type LeftoverSafety = {
  path: string; score: number; verdict: string; reasons: string[];
};
export type LeftoverDeleteReport = { freed: number; errors: string[]; skipped: string[] };

// 磁盘健康与评分
export type DiskHealth = {
  model: string; mediaType: string; busType: string;
  healthStatus: string; size: number; friendlyName: string;
};
export type HealthScore = {
  score: number; diskScore: number; hibernationScore: number; junkScore: number;
  diskFreePercent: number; hasHibernation: boolean; hibernationSize: number;
  junkSize: number; grade: string;
};

// 社交缓存类型
export type SocialCategory = {
  category: string; risk: string; size: number; fileCount: number; deletable: boolean;
};
export type SocialApp = {
  app: string; dataDirs: string[]; categories: SocialCategory[]; totalSize: number;
};
export type SocialScan = { apps: SocialApp[]; totalSize: number; deletableSize: number };
export type SocialDeleteReport = { freed: number; errors: string[] };

// 热点与增长
export type HotspotEntry = {
  path: string; name: string; totalSize: number; fileCount: number;
  lastModified: number; isCache: boolean; children: HotspotEntry[];
};
export type HotspotReport = { entries: HotspotEntry[]; scannedFolders: number; durationMs: number };
export type GrowthEntry = {
  path: string; name: string; oldSize: number; newSize: number;
  diff: number; diffPercent: number; level: string;
};
export type GrowthReport = {
  entries: GrowthEntry[]; totalGrowth: number; baselineAt: number;
  timeSpanHours: number; hasBaseline: boolean;
};
