//! light-c 自研 MFT 引擎移植（C6「并存」路线回补）
//!
//! 与 disk-butler 栈（ntfs-reader，服务化目录树秒扫）并存，服务三个高频场景：
//! - 大文件扫描：USN 枚举 + $MFT 顺序读 + Top-N 候选（`scan_top_files`）
//! - 热点分析：全目录聚合统计（`full_drive_stats`）
//! - 增长基线/明细：目录级大小 + 全量文件清单（同 `full_drive_stats`）
//!
//! 与 light-c 原版的差异：
//! - winapi FFI 全部迁 windows 0.62（与 C2 后的工程统一）
//! - 三场景共用同一份引擎代码（light-c 在 disk_growth/mft_scan.rs 里重复了一份
//!   NtfsFileSizeReader，本移植只保留单实现）
//! - 需要管理员权限（打开 `\\.\C:` 裸卷是系统硬性要求）；调用方负责回退遍历路径

#![cfg(target_os = "windows")]

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::fs;
use std::os::windows::fs::{FileExt, OpenOptionsExt};
use std::path::PathBuf;

use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE, WIN32_ERROR};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const FSCTL_ENUM_USN_DATA: u32 = (9 << 16) | (44 << 2) | 3;
const ERROR_HANDLE_EOF: WIN32_ERROR = WIN32_ERROR(38);
const USN_BUFFER_SIZE: usize = 1024 * 1024;
const MFT_METADATA_MAX: u64 = 25;
const MFT_READ_CHUNK: usize = 16 * 1024 * 1024;
const WINDOWS_TO_UNIX_SECONDS: i64 = 11_644_473_600;
const FILE_RECORD_FLAG_IN_USE: u16 = 0x0001;
const FILE_RECORD_FLAG_DIRECTORY: u16 = 0x0002;
/// FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS（云端占位文件）
const CLOUD_PLACEHOLDER_ATTRIBUTES: u32 = 0x0004_0000 | 0x0040_0000;
const GENERIC_READ: u32 = 0x8000_0000;

#[repr(C)]
#[allow(non_snake_case)]
pub struct MftEnumDataV0 {
    pub StartFileReferenceNumber: u64,
    pub LowUsn: i64,
    pub HighUsn: i64,
}

#[derive(Clone)]
pub struct MftEntry {
    pub mft_id: u64,
    pub parent_id: u64,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Clone, Copy)]
pub struct MftFileMetadata {
    pub size: u64,
    pub modified: i64,
    pub is_cloud_placeholder: bool,
}

#[derive(Clone)]
pub struct MftTopFileCandidate {
    pub mft_id: u64,
    pub size: u64,
    pub modified: i64,
}

#[derive(Clone)]
struct DataRun {
    start_lcn: i64,
    cluster_count: u64,
}

pub struct NtfsFileMetadataReader {
    volume: fs::File,
    bytes_per_sector: usize,
    cluster_size: u64,
    file_record_size: usize,
    mft_runs: Vec<DataRun>,
}

// ---------- 卷访问与枚举 ----------

pub fn open_volume(drive_letter: char) -> Result<HANDLE, String> {
    let p = format!("\\\\.\\{}:", drive_letter);
    let w: Vec<u16> = p.encode_utf16().chain(std::iter::once(0)).collect();
    let h = unsafe {
        CreateFileW(
            windows::core::PCWSTR(w.as_ptr()),
            GENERIC_READ,
            FILE_SHARE_MODE(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0),
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
        .map_err(|_| format!("无法打开卷设备 {} (错误码: {})", p, GetLastError().0))?
    };
    Ok(h)
}

pub fn close_volume(h: HANDLE) {
    unsafe {
        let _ = CloseHandle(h);
    }
}

/// USN 枚举全卷文件/目录记录；cb 返回 false 即取消
pub fn enumerate_usn_records_v2(
    h_device: HANDLE,
    cb: &impl Fn(usize) -> bool,
) -> Result<Vec<MftEntry>, String> {
    let mut entries: Vec<MftEntry> = Vec::new();
    let mut buf: Vec<u8> = vec![0u8; USN_BUFFER_SIZE];
    let mut processed: usize = 0;
    let mut frn: u64 = 0;
    loop {
        let mut br: u32 = 0;
        let mut ed = MftEnumDataV0 {
            StartFileReferenceNumber: frn,
            LowUsn: 0,
            HighUsn: i64::MAX,
        };
        let ok = unsafe {
            DeviceIoControl(
                h_device,
                FSCTL_ENUM_USN_DATA,
                Some(&mut ed as *mut _ as *const _),
                std::mem::size_of::<MftEnumDataV0>() as u32,
                Some(buf.as_mut_ptr() as *mut _),
                USN_BUFFER_SIZE as u32,
                Some(&mut br),
                None,
            )
        };
        if ok.is_err() {
            if unsafe { GetLastError() } == ERROR_HANDLE_EOF {
                break;
            }
            return Err("DeviceIoControl 失败".into());
        }
        if br == 0 {
            break;
        }
        if processed == 0 && !cb(0) {
            return Err("扫描已取消".into());
        }
        let mut offset: usize = 8;
        while offset < br as usize {
            let Some((rlen, mft_id, parent_id, file_name, is_dir, is_reparse)) =
                read_usn_record(&buf, offset, br as usize)
            else {
                break;
            };
            if !file_name.is_empty() && !is_reparse && mft_id > MFT_METADATA_MAX {
                entries.push(MftEntry {
                    mft_id,
                    parent_id,
                    name: file_name,
                    is_dir,
                });
            }
            processed += 1;
            if processed % 10_000 == 0 && !cb(processed) {
                return Err("扫描已取消".into());
            }
            offset += rlen;
        }
        // FRN 前进校验：设备返回的"下一 FRN"若未前进（回落/为 0），
        // 说明无法继续枚举，break 防死循环
        let next_frn = unsafe { std::ptr::read_unaligned(buf.as_ptr() as *const u64) };
        if next_frn == 0 || next_frn <= frn {
            break;
        }
        frn = next_frn;
    }
    cb(processed);
    Ok(entries)
}

fn read_usn_record(
    buffer: &[u8],
    offset: usize,
    bytes_returned: usize,
) -> Option<(usize, u64, u64, String, bool, bool)> {
    unsafe {
        let base = buffer.as_ptr().add(offset);
        let record_len = std::ptr::read_unaligned(base as *const u32) as usize;
        // USN_RECORD_V2 头固定 60 字节：record_len 过小（损坏记录）会导致
        // 下方 add(8/16/52/56/58) 越界读——必须先校验最小长度
        if record_len == 0 || record_len < 60 || offset + record_len > bytes_returned {
            return None;
        }

        let file_ref = std::ptr::read_unaligned(base.add(8) as *const u64);
        let parent_ref = std::ptr::read_unaligned(base.add(16) as *const u64);
        let attributes = std::ptr::read_unaligned(base.add(52) as *const u32);
        let name_len = std::ptr::read_unaligned(base.add(56) as *const u16) as usize / 2;
        let name_offset = std::ptr::read_unaligned(base.add(58) as *const u16) as usize;
        if name_offset + name_len * 2 > record_len {
            return None;
        }

        const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let name = if name_len > 0 {
            String::from_utf16_lossy(std::slice::from_raw_parts(
                buffer.as_ptr().add(offset + name_offset) as *const u16,
                name_len,
            ))
        } else {
            String::new()
        };
        let is_dir = (attributes & FILE_ATTRIBUTE_DIRECTORY) == FILE_ATTRIBUTE_DIRECTORY;
        let is_reparse =
            (attributes & FILE_ATTRIBUTE_REPARSE_POINT) == FILE_ATTRIBUTE_REPARSE_POINT;

        Some((
            record_len,
            file_ref & 0x0000_FFFF_FFFF_FFFF,
            parent_ref & 0x0000_FFFF_FFFF_FFFF,
            name,
            is_dir,
            is_reparse,
        ))
    }
}

/// 只为指定 id 集合懒解析全路径（TopN 候选用，避免几十万条路径分配）
pub fn rebuild_paths_for_ids(
    entries: &[MftEntry],
    drive_letter: char,
    target_ids: &HashSet<u64>,
) -> HashMap<u64, String> {
    let mut entry_by_id: HashMap<u64, &MftEntry> = HashMap::with_capacity(entries.len());
    for entry in entries {
        entry_by_id.insert(entry.mft_id, entry);
    }

    let mut paths = HashMap::with_capacity(target_ids.len() + 1);
    paths.insert(5, format!("{}:\\", drive_letter));

    for target_id in target_ids {
        let mut visiting = HashSet::new();
        let _ = resolve_path(*target_id, &entry_by_id, &mut paths, &mut visiting, 0);
    }

    paths
}

fn resolve_path(
    mft_id: u64,
    entry_by_id: &HashMap<u64, &MftEntry>,
    paths: &mut HashMap<u64, String>,
    visiting: &mut HashSet<u64>,
    depth: u32,
) -> Option<String> {
    // 深度上限：NTFS 路径最长约 32K 字符，病态/恶意父链可递归数千层
    // 耗尽栈；超限直接放弃解析（该目录不入 paths，不影响其他）
    if depth > 256 {
        return None;
    }
    if let Some(path) = paths.get(&mft_id) {
        return Some(path.clone());
    }
    if !visiting.insert(mft_id) {
        return None;
    }

    let entry = entry_by_id.get(&mft_id)?;
    let parent_path = resolve_path(entry.parent_id, entry_by_id, paths, visiting, depth + 1)?;
    let mut full_path = String::with_capacity(parent_path.len() + 1 + entry.name.len());
    full_path.push_str(&parent_path);
    if !parent_path.ends_with('\\') {
        full_path.push('\\');
    }
    full_path.push_str(&entry.name);
    paths.insert(mft_id, full_path.clone());
    visiting.remove(&mft_id);
    Some(full_path)
}

// ---------- 环境检测 ----------

pub fn is_elevated() -> bool {
    unsafe {
        let mut t = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut t).is_err() {
            return false;
        }
        let mut e = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut s: u32 = 0;
        let ok = GetTokenInformation(
            t,
            TokenElevation,
            Some(&mut e as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut s,
        );
        let _ = CloseHandle(t);
        ok.is_ok() && e.TokenIsElevated != 0
    }
}

pub fn is_ntfs(drive_letter: char) -> bool {
    use windows::Win32::Storage::FileSystem::GetVolumeInformationW;
    let r = format!("{}:\\", drive_letter);
    let w: Vec<u16> = r.encode_utf16().chain(std::iter::once(0)).collect();
    let mut fs_buf: [u16; 16] = [0; 16];
    if unsafe {
        GetVolumeInformationW(
            windows::core::PCWSTR(w.as_ptr()),
            None,
            None,
            None,
            None,
            Some(&mut fs_buf),
        )
    }
    .is_err()
    {
        return false;
    }
    String::from_utf16_lossy(&fs_buf)
        .trim_end_matches('\0')
        .eq_ignore_ascii_case("NTFS")
}

/// MFT 快路径可用性：NTFS + 已提权
pub fn mft_available(drive_letter: char) -> bool {
    is_elevated() && is_ntfs(drive_letter)
}

// ---------- $MFT 顺序读取 ----------

impl NtfsFileMetadataReader {
    pub fn open(drive_letter: char) -> Result<Self, String> {
        let volume_path = format!("\\\\.\\{}:", drive_letter);
        let volume = fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
            .open(&volume_path)
            .map_err(|error| format!("打开 NTFS 卷失败: {}", error))?;

        let mut boot_sector = [0u8; 512];
        volume
            .seek_read(&mut boot_sector, 0)
            .map_err(|error| format!("读取 NTFS 引导扇区失败: {}", error))?;

        let bytes_per_sector = read_u16(&boot_sector, 11)
            .ok_or_else(|| "NTFS 引导扇区缺少 bytes_per_sector".to_string())?
            as usize;
        let sectors_per_cluster = boot_sector
            .get(13)
            .copied()
            .ok_or_else(|| "NTFS 引导扇区缺少 sectors_per_cluster".to_string())?
            as usize;
        // 引导扇区损坏校验（防除零/取模/移位 panic）：
        // bytes_per_sector 与 sectors_per_cluster 必须非零（否则 cluster_size=0，
        // 后续 chunks_exact(0)/除法全部 panic）
        if bytes_per_sector == 0 || sectors_per_cluster == 0 {
            return Err("NTFS 引导扇区损坏：bytes_per_sector 或 sectors_per_cluster 为 0".to_string());
        }
        let cluster_size = (bytes_per_sector * sectors_per_cluster) as u64;
        let mft_lcn =
            read_i64(&boot_sector, 48).ok_or_else(|| "NTFS 引导扇区缺少 MFT LCN".to_string())?;
        let file_record_size = decode_file_record_size(
            *boot_sector
                .get(64)
                .ok_or_else(|| "NTFS 引导扇区缺少 FILE record 大小".to_string())? as i8,
            cluster_size,
        );

        let mut reader = Self {
            volume,
            bytes_per_sector,
            cluster_size,
            file_record_size,
            // 先用 boot sector 里的 $MFT 起点读取自身记录，再解析真正 data runs。
            mft_runs: vec![DataRun {
                start_lcn: mft_lcn,
                cluster_count: 24,
            }],
        };

        if let Some(runs) = reader.read_mft_data_runs() {
            reader.mft_runs = runs;
        }

        Ok(reader)
    }

    pub fn read_top_file_candidates(
        &self,
        top_limit: usize,
        progress_cb: &impl Fn(usize) -> bool,
    ) -> Result<Vec<MftTopFileCandidate>, String> {
        let mut heap: BinaryHeap<Reverse<(u64, u64, i64)>> = BinaryHeap::new();
        let mut logical_record_id = 0u64;

        for run in &self.mft_runs {
            let run_start = (run.start_lcn as u64)
                .checked_mul(self.cluster_size)
                .ok_or_else(|| "$MFT data run 起始位置溢出".to_string())?;
            let run_bytes = run
                .cluster_count
                .checked_mul(self.cluster_size)
                .ok_or_else(|| "$MFT data run 大小溢出".to_string())?;
            let records_in_run = run_bytes / self.file_record_size as u64;
            let mut bytes_read_in_run = 0u64;
            let mut buffer = vec![0u8; MFT_READ_CHUNK.max(self.file_record_size)];

            while bytes_read_in_run < run_bytes {
                let remaining = (run_bytes - bytes_read_in_run) as usize;
                let read_len = remaining.min(buffer.len());
                let aligned_read_len = read_len - (read_len % self.file_record_size);
                if aligned_read_len == 0 {
                    break;
                }

                self.volume
                    .seek_read(
                        &mut buffer[..aligned_read_len],
                        run_start + bytes_read_in_run,
                    )
                    .map_err(|error| format!("顺序读取 $MFT 失败: {}", error))?;

                let records_before_chunk = bytes_read_in_run / self.file_record_size as u64;
                for (record_index, record_bytes) in buffer[..aligned_read_len]
                    .chunks_exact(self.file_record_size)
                    .enumerate()
                {
                    let record_id = logical_record_id + records_before_chunk + record_index as u64;
                    if !looks_like_active_file(record_bytes) {
                        if record_id % 100_000 == 0 && !progress_cb(record_id as usize) {
                            return Err("扫描已取消".into());
                        }
                        continue;
                    }

                    let mut record = record_bytes.to_vec();
                    if self.apply_fixup(&mut record).is_some() {
                        if let Some(metadata) = parse_active_file_record_metadata(&record) {
                            // 云端占位文件不挤占 Top-N 名额（召回成本极高且不占本地空间）。
                            if metadata.size > 0 && !metadata.is_cloud_placeholder {
                                heap.push(Reverse((metadata.size, record_id, metadata.modified)));
                                if heap.len() > top_limit {
                                    heap.pop();
                                }
                            }
                        }
                    }

                    if record_id % 100_000 == 0 && !progress_cb(record_id as usize) {
                        return Err("扫描已取消".into());
                    }
                }

                bytes_read_in_run += aligned_read_len as u64;
            }

            logical_record_id += records_in_run;
        }

        let mut candidates: Vec<MftTopFileCandidate> = heap
            .into_iter()
            .map(|Reverse((size, mft_id, modified))| MftTopFileCandidate {
                mft_id,
                size,
                modified,
            })
            .collect();
        candidates.sort_by(|left, right| {
            right
                .size
                .cmp(&left.size)
                .then_with(|| left.mft_id.cmp(&right.mft_id))
        });
        Ok(candidates)
    }

    pub fn read_file_metadata_map(
        &self,
        wanted_mft_ids: &HashSet<u64>,
        progress_cb: &impl Fn(usize) -> bool,
    ) -> Result<HashMap<u64, MftFileMetadata>, String> {
        let mut metadata_by_id = HashMap::with_capacity(wanted_mft_ids.len());
        let mut logical_record_id = 0u64;

        for run in &self.mft_runs {
            let run_start = (run.start_lcn as u64)
                .checked_mul(self.cluster_size)
                .ok_or_else(|| "$MFT data run 起始位置溢出".to_string())?;
            let run_bytes = run
                .cluster_count
                .checked_mul(self.cluster_size)
                .ok_or_else(|| "$MFT data run 大小溢出".to_string())?;
            let records_in_run = run_bytes / self.file_record_size as u64;
            let mut bytes_read_in_run = 0u64;
            let mut buffer = vec![0u8; MFT_READ_CHUNK.max(self.file_record_size)];

            while bytes_read_in_run < run_bytes {
                let remaining = (run_bytes - bytes_read_in_run) as usize;
                let read_len = remaining.min(buffer.len());
                let aligned_read_len = read_len - (read_len % self.file_record_size);
                if aligned_read_len == 0 {
                    break;
                }

                self.volume
                    .seek_read(
                        &mut buffer[..aligned_read_len],
                        run_start + bytes_read_in_run,
                    )
                    .map_err(|error| format!("顺序读取 $MFT 失败: {}", error))?;

                let records_before_chunk = bytes_read_in_run / self.file_record_size as u64;
                for (record_index, record_bytes) in buffer[..aligned_read_len]
                    .chunks_exact(self.file_record_size)
                    .enumerate()
                {
                    let record_id = logical_record_id + records_before_chunk + record_index as u64;
                    if wanted_mft_ids.contains(&record_id) && looks_like_active_file(record_bytes) {
                        let mut record = record_bytes.to_vec();
                        if self.apply_fixup(&mut record).is_some() {
                            if let Some(metadata) = parse_active_file_record_metadata(&record) {
                                metadata_by_id.insert(record_id, metadata);
                            }
                        }
                    }

                    if record_id % 100_000 == 0 && !progress_cb(record_id as usize) {
                        return Err("扫描已取消".into());
                    }

                    if metadata_by_id.len() >= wanted_mft_ids.len() {
                        return Ok(metadata_by_id);
                    }
                }

                bytes_read_in_run += aligned_read_len as u64;
            }

            logical_record_id += records_in_run;
        }

        Ok(metadata_by_id)
    }

    fn read_mft_data_runs(&self) -> Option<Vec<DataRun>> {
        let mut record = vec![0u8; self.file_record_size];
        let first_run = self.mft_runs.first()?;
        let offset = (first_run.start_lcn as u64).checked_mul(self.cluster_size)?;
        self.volume.seek_read(&mut record, offset).ok()?;
        self.apply_fixup(&mut record)?;
        parse_file_record_data_runs(&record)
    }

    fn apply_fixup(&self, record: &mut [u8]) -> Option<()> {
        if record.len() < 8 || &record[0..4] != b"FILE" {
            return None;
        }

        let fixup_offset = read_u16(record, 4)? as usize;
        let fixup_count = read_u16(record, 6)? as usize;
        if fixup_count == 0 || fixup_offset + fixup_count * 2 > record.len() {
            return None;
        }

        let sectors_in_record = record.len() / self.bytes_per_sector;
        if fixup_count != sectors_in_record + 1 {
            return None;
        }

        let update_sequence = read_u16(record, fixup_offset)?;
        for sector_index in 0..sectors_in_record {
            let sector_tail = (sector_index + 1) * self.bytes_per_sector - 2;
            if read_u16(record, sector_tail)? != update_sequence {
                return None;
            }
            let replacement_offset = fixup_offset + (sector_index + 1) * 2;
            let replacement = read_u16(record, replacement_offset)?;
            write_u16(record, sector_tail, replacement)?;
        }

        Some(())
    }
}

// ---------- 文件记录解析 ----------

fn looks_like_active_file(record: &[u8]) -> bool {
    if record.len() < 24 || &record[0..4] != b"FILE" {
        return false;
    }
    let Some(flags) = read_u16(record, 22) else {
        return false;
    };
    (flags & FILE_RECORD_FLAG_IN_USE) != 0 && (flags & FILE_RECORD_FLAG_DIRECTORY) == 0
}

fn parse_active_file_record_metadata(record: &[u8]) -> Option<MftFileMetadata> {
    let flags = read_u16(record, 22)?;
    if (flags & FILE_RECORD_FLAG_IN_USE) == 0 || (flags & FILE_RECORD_FLAG_DIRECTORY) != 0 {
        return None;
    }
    parse_file_record_metadata(record)
}

fn parse_file_record_metadata(record: &[u8]) -> Option<MftFileMetadata> {
    let attributes_offset = read_u16(record, 20)? as usize;
    let mut offset = attributes_offset;
    let mut modified = 0i64;
    let mut file_attributes = 0u32;
    let mut unnamed_data_size = None;
    let mut fallback_data_size = None;

    while offset + 16 <= record.len() {
        let attribute_type = read_u32(record, offset)?;
        if attribute_type == 0xFFFF_FFFF {
            break;
        }

        let attribute_length = read_u32(record, offset + 4)? as usize;
        if attribute_length == 0 || offset + attribute_length > record.len() {
            return None;
        }

        if attribute_type == 0x10 && modified == 0 {
            if let Some((standard_modified, standard_attributes)) =
                parse_standard_info(record, offset)
            {
                modified = standard_modified;
                file_attributes = standard_attributes;
            }
        }

        if attribute_type == 0x80 {
            let name_length = *record.get(offset + 9)?;
            let size = parse_data_attribute_size(record, offset);
            if name_length == 0 {
                unnamed_data_size = size;
                break;
            } else if fallback_data_size.is_none() {
                // 只有没有主数据流时才保留备用数据流大小，避免 ADS 抢占普通文件大小。
                fallback_data_size = size;
            }
        }

        offset += attribute_length;
    }

    let size = unnamed_data_size.or(fallback_data_size)?;
    Some(MftFileMetadata {
        size,
        modified,
        is_cloud_placeholder: (file_attributes & CLOUD_PLACEHOLDER_ATTRIBUTES) != 0,
    })
}

fn parse_standard_info(record: &[u8], attribute_offset: usize) -> Option<(i64, u32)> {
    let value_offset = read_u16(record, attribute_offset + 20)? as usize;
    let value_start = attribute_offset + value_offset;
    let modified_filetime = read_u64(record, value_start + 8)?;
    let file_attributes = read_u32(record, value_start + 32)?;
    Some((filetime_to_unix_seconds(modified_filetime), file_attributes))
}

fn parse_data_attribute_size(record: &[u8], attribute_offset: usize) -> Option<u64> {
    let non_resident = *record.get(attribute_offset + 8)?;
    if non_resident == 0 {
        read_u32(record, attribute_offset + 16).map(|size| size as u64)
    } else {
        read_u64(record, attribute_offset + 48)
    }
}

fn parse_file_record_data_runs(record: &[u8]) -> Option<Vec<DataRun>> {
    let attributes_offset = read_u16(record, 20)? as usize;
    let mut offset = attributes_offset;

    while offset + 16 <= record.len() {
        let attribute_type = read_u32(record, offset)?;
        if attribute_type == 0xFFFF_FFFF {
            break;
        }

        let attribute_length = read_u32(record, offset + 4)? as usize;
        if attribute_length == 0 || offset + attribute_length > record.len() {
            return None;
        }

        if attribute_type == 0x80 && *record.get(offset + 8)? == 1 && *record.get(offset + 9)? == 0
        {
            let data_run_offset = read_u16(record, offset + 32)? as usize;
            let data_run_start = offset + data_run_offset;
            let data_run_end = offset + attribute_length;
            return parse_data_runs(record.get(data_run_start..data_run_end)?);
        }

        offset += attribute_length;
    }

    None
}

fn parse_data_runs(bytes: &[u8]) -> Option<Vec<DataRun>> {
    let mut runs = Vec::new();
    let mut offset = 0usize;
    let mut current_lcn = 0i64;

    while offset < bytes.len() {
        let header = *bytes.get(offset)?;
        offset += 1;
        if header == 0 {
            break;
        }

        let length_size = (header & 0x0F) as usize;
        let offset_size = (header >> 4) as usize;
        if length_size == 0
            || length_size > 8
            || offset_size == 0
            || offset_size > 8
            || offset + length_size + offset_size > bytes.len()
        {
            return None;
        }

        let cluster_count = read_unsigned_le(&bytes[offset..offset + length_size]);
        offset += length_size;
        let lcn_delta = read_signed_le(&bytes[offset..offset + offset_size]);
        offset += offset_size;

        current_lcn = current_lcn.checked_add(lcn_delta)?;
        if current_lcn < 0 {
            return None;
        }

        runs.push(DataRun {
            start_lcn: current_lcn,
            cluster_count,
        });
    }

    if runs.is_empty() {
        None
    } else {
        Some(runs)
    }
}

fn decode_file_record_size(raw: i8, cluster_size: u64) -> usize {
    if raw > 0 {
        raw as usize * cluster_size as usize
    } else {
        // 合法 NTFS 值为 -10/-11/-12（1024/2048/4096）。损坏引导扇区可能
        // 出现 raw=-128（-raw 溢出）或 raw<=-64（移位溢出 panic）——
        // 限定范围，非法值兜底 4096
        let shift = (-(raw as i32)) as u32;
        if shift == 0 || shift > 12 {
            return 4096;
        }
        1usize << shift
    }
}

fn filetime_to_unix_seconds(filetime: u64) -> i64 {
    let seconds = (filetime / 10_000_000) as i64 - WINDOWS_TO_UNIX_SECONDS;
    seconds.max(0)
}

// ---------- 小端读取工具 ----------

fn read_u16(buffer: &[u8], offset: usize) -> Option<u16> {
    let bytes = buffer.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn write_u16(buffer: &mut [u8], offset: usize, value: u16) -> Option<()> {
    let bytes = buffer.get_mut(offset..offset + 2)?;
    bytes.copy_from_slice(&value.to_le_bytes());
    Some(())
}

fn read_u32(buffer: &[u8], offset: usize) -> Option<u32> {
    let bytes = buffer.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64(buffer: &[u8], offset: usize) -> Option<u64> {
    let bytes = buffer.get(offset..offset + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_i64(buffer: &[u8], offset: usize) -> Option<i64> {
    let bytes = buffer.get(offset..offset + 8)?;
    Some(i64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn read_unsigned_le(bytes: &[u8]) -> u64 {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().enumerate() {
        value |= (*byte as u64) << (index * 8);
    }
    value
}

fn read_signed_le(bytes: &[u8]) -> i64 {
    if bytes.is_empty() {
        return 0;
    }
    // 8 字节已是完整 i64，直接按位解释即可；走下方 1i64<<(len*8) 会
    // 移位溢出（len==8 → <<64 非法）
    if bytes.len() == 8 {
        return read_unsigned_le(bytes) as i64;
    }

    let mut value = read_unsigned_le(bytes) as i64;
    let sign_bit = 1i64 << (bytes.len() * 8 - 1);
    if value & sign_bit != 0 {
        value -= 1i64 << (bytes.len() * 8);
    }
    value
}

// ============================================================================
// 场景适配器
// ============================================================================

/// 大文件 MFT 快路径：返回 (路径, 大小, 修改时间 s)，按大小降序
///
/// 移植自 light-c mft_bigfiles::scan_top_files_via_mft；风险/来源标签由调用方按自身规则补充。
pub fn scan_top_files(
    drive_letter: char,
    top_n: usize,
    on_progress: &dyn Fn(&str, usize) -> bool,
) -> Result<Vec<(String, u64, i64)>, String> {
    let drive_letter = drive_letter.to_ascii_uppercase();
    // 入口钳制：极大 top_n 会令候选池无界增长 → OOM
    let top_n = top_n.clamp(1, 5000);

    // Step 1: USN 枚举
    let h_device = open_volume(drive_letter)?;
    let enumerate_result = enumerate_usn_records_v2(h_device, &|processed| {
        if processed % 100_000 == 0 {
            on_progress("enumerate", processed)
        } else {
            true
        }
    });
    close_volume(h_device);
    let entries = enumerate_result?;
    if entries.is_empty() {
        return Err("USN 空".into());
    }

    // Step 2: 顺序读 $MFT，维护 Top-N*20 候选池（先不重建全盘路径）
    let metadata_reader = NtfsFileMetadataReader::open(drive_letter)?;
    let candidate_limit = top_n.saturating_mul(20).max(1000);
    let candidates = metadata_reader.read_top_file_candidates(candidate_limit, &|processed| {
        if processed % 100_000 == 0 {
            on_progress("metadata", processed)
        } else {
            true
        }
    })?;

    // Step 3: 只给候选文件重建路径
    let candidate_ids: HashSet<u64> = candidates.iter().map(|c| c.mft_id).collect();
    let paths = rebuild_paths_for_ids(&entries, drive_letter, &candidate_ids);

    // Step 4: 系统目录过滤 + 最终 Top-N
    const SKIP_PATH_SEGMENTS: &[&str] = &[
        "\\windows\\system32\\",
        "\\windows\\syswow64\\",
        "\\windows\\winsxs\\",
        "\\windows\\assembly\\",
        "\\windows\\microsoft.net\\",
    ];
    let mut heap: BinaryHeap<Reverse<(u64, u64)>> = BinaryHeap::new();
    for candidate in &candidates {
        let Some(path) = paths.get(&candidate.mft_id) else {
            continue;
        };
        let lower = path.to_ascii_lowercase();
        if SKIP_PATH_SEGMENTS.iter().any(|s| lower.contains(s)) {
            continue;
        }
        heap.push(Reverse((candidate.size, candidate.mft_id)));
        if heap.len() > top_n {
            heap.pop();
        }
    }

    let mut results: Vec<(String, u64, i64)> = heap
        .into_iter()
        .filter_map(|Reverse((_, mft_id))| {
            let path = paths.get(&mft_id)?;
            let candidate = candidates.iter().find(|c| c.mft_id == mft_id)?;
            Some((path.clone(), candidate.size, candidate.modified))
        })
        .collect();
    results.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(results)
}

/// 全目录聚合统计：目录路径 → (总大小, 文件数, 最新修改时间 ms)
pub type DirStatsMap = HashMap<PathBuf, (u64, usize, i64)>;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct FullDriveStats {
    pub dir_stats: DirStatsMap,
    /// 全部文件（路径, 大小, 修改时间 s）
    pub files: Vec<(String, u64, i64)>,
}

/// 全盘目录统计 + 可选文件清单（热点/增长共用；移植自 light-c hotspot mft_scanner 的聚合路径）
///
/// `with_files=false`：只算目录聚合（热点/基线用），省掉 65 万次文件路径拼接，
/// 与 light-c hotspot mft_scanner.scan_via_mft 同构——后者只产目录统计不产文件清单。
/// `with_files=true`：额外构建全部文件（路径,大小,mtime）（增长明细用）。
///
/// on_progress(stage, processed) -> bool；stage ∈ enumerate / index / metadata / aggregate
pub fn full_drive_stats(
    drive_letter: char,
    with_files: bool,
    on_progress: &dyn Fn(&str, usize) -> bool,
) -> Result<FullDriveStats, String> {
    let drive_letter = drive_letter.to_ascii_uppercase();

    // Step 1: USN 枚举
    let handle = open_volume(drive_letter)?;
    let enumerate_result = enumerate_usn_records_v2(handle, &|processed| {
        on_progress("enumerate", processed)
    });
    close_volume(handle);
    let entries = enumerate_result?;

    // Step 2: BFS 目录索引（全路径）
    if !on_progress("index", entries.len()) {
        return Err("扫描已取消".into());
    }
    let (directories, deepest_depth) = build_directory_index(&entries, drive_letter);

    // 文件目标：父目录在索引内的文件
    let mut file_ids: HashSet<u64> = HashSet::new();
    let mut parent_pairs: Vec<(u64, u64)> = Vec::new();
    for entry in entries.iter().filter(|e| !e.is_dir) {
        if directories.contains_key(&entry.parent_id) {
            file_ids.insert(entry.mft_id);
            parent_pairs.push((entry.mft_id, entry.parent_id));
        }
    }

    // Step 3: $MFT 顺序读元数据
    let metadata_reader = NtfsFileMetadataReader::open(drive_letter)?;
    let metadata_by_id = metadata_reader.read_file_metadata_map(&file_ids, &|processed| {
        on_progress("metadata", processed)
    })?;

    // Step 4: 自底向上聚合到全部祖先目录
    if !on_progress("aggregate", metadata_by_id.len()) {
        return Err("扫描已取消".into());
    }

    // 文件路径清单（仅增长明细需要）：拼接父目录路径 + 文件名
    let files = if with_files {
        let name_by_id: HashMap<u64, &str> = entries
            .iter()
            .filter(|e| !e.is_dir && e.mft_id != 0)
            .map(|e| (e.mft_id, e.name.as_str()))
            .collect();
        let mut v: Vec<(String, u64, i64)> = Vec::with_capacity(metadata_by_id.len());
        for (file_id, parent_id) in &parent_pairs {
            let Some(metadata) = metadata_by_id.get(file_id) else {
                continue;
            };
            if metadata.size == 0 {
                continue;
            }
            let Some(parent_path) = directories.get(parent_id).map(|d| &d.path) else {
                continue;
            };
            let Some(name) = name_by_id.get(file_id) else {
                continue;
            };
            v.push((
                parent_path.join(name).to_string_lossy().to_string(),
                metadata.size,
                metadata.modified,
            ));
        }
        v
    } else {
        Vec::new()
    };

    // 目录聚合
    #[derive(Clone, Copy, Default)]
    struct DirAcc {
        total_size: u64,
        file_count: usize,
        last_modified: i64, // ms（对外约定）
    }
    let mut acc: HashMap<u64, DirAcc> = HashMap::with_capacity(directories.len());
    let mut ids_by_depth = vec![Vec::<u64>::new(); deepest_depth as usize + 1];
    for (dir_id, dir) in &directories {
        if let Some(bucket) = ids_by_depth.get_mut(dir.depth as usize) {
            bucket.push(*dir_id);
        }
    }
    for (file_id, parent_id) in &parent_pairs {
        let Some(metadata) = metadata_by_id.get(file_id) else {
            continue;
        };
        if metadata.size == 0 {
            continue;
        }
        let st = acc.entry(*parent_id).or_default();
        st.total_size = st.total_size.saturating_add(metadata.size);
        st.file_count += 1;
        let ms = metadata.modified.saturating_mul(1000);
        if ms > st.last_modified {
            st.last_modified = ms;
        }
    }
    for depth in (0..ids_by_depth.len()).rev() {
        for dir_id in &ids_by_depth[depth] {
            let Some(child) = acc.get(dir_id).copied() else {
                continue;
            };
            let Some(dir) = directories.get(dir_id) else {
                continue;
            };
            if dir.parent_id == 0 || dir.parent_id == *dir_id {
                continue;
            }
            if directories.contains_key(&dir.parent_id) {
                let st = acc.entry(dir.parent_id).or_default();
                st.total_size = st.total_size.saturating_add(child.total_size);
                st.file_count += child.file_count;
                if child.last_modified > st.last_modified {
                    st.last_modified = child.last_modified;
                }
            }
        }
    }

    let dir_stats: DirStatsMap = acc
        .into_iter()
        .filter_map(|(dir_id, st)| {
            let dir = directories.get(&dir_id)?;
            Some((dir.path.clone(), (st.total_size, st.file_count, st.last_modified)))
        })
        .collect();

    Ok(FullDriveStats { dir_stats, files })
}

#[derive(Clone)]
struct DirectoryInfo {
    parent_id: u64,
    path: PathBuf,
    depth: u16,
}

fn build_directory_index(
    entries: &[MftEntry],
    drive_letter: char,
) -> (HashMap<u64, DirectoryInfo>, u16) {
    let mut directories = HashMap::new();
    let mut children: HashMap<u64, Vec<usize>> = HashMap::with_capacity(entries.len());
    let mut deepest_depth = 0u16;

    for (index, entry) in entries.iter().enumerate() {
        if entry.is_dir {
            children.entry(entry.parent_id).or_default().push(index);
        }
    }

    directories.insert(
        5,
        DirectoryInfo {
            parent_id: 0,
            path: PathBuf::from(format!("{}:\\", drive_letter)),
            depth: 0,
        },
    );

    let mut queue = VecDeque::new();
    if let Some(root_children) = children.get(&5) {
        queue.extend(root_children.iter().copied());
    }

    while let Some(index) = queue.pop_front() {
        let entry = &entries[index];
        if directories.contains_key(&entry.mft_id) {
            continue;
        }
        let Some(parent) = directories.get(&entry.parent_id) else {
            continue;
        };
        let path = parent.path.join(&entry.name);
        let depth = parent.depth.saturating_add(1);
        deepest_depth = deepest_depth.max(depth);
        directories.insert(
            entry.mft_id,
            DirectoryInfo {
                parent_id: entry.parent_id,
                path,
                depth,
            },
        );
        if let Some(next_children) = children.get(&entry.mft_id) {
            queue.extend(next_children.iter().copied());
        }
    }

    (directories, deepest_depth)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_file_record_size_negative_power() {
        assert_eq!(decode_file_record_size(-10, 4096), 1024);
        assert_eq!(decode_file_record_size(-12, 4096), 4096);
    }

    #[test]
    fn decode_file_record_size_positive_clusters() {
        assert_eq!(decode_file_record_size(2, 4096), 8192);
    }

    #[test]
    fn filetime_epoch_conversion() {
        // 1970-01-01T00:00:00Z 的 FILETIME
        assert_eq!(filetime_to_unix_seconds(116_444_736_000_000_000), 0);
        // 2020-01-01T00:00:00Z
        assert_eq!(filetime_to_unix_seconds(132_223_104_000_000_000), 1_577_836_800);
    }

    #[test]
    fn parse_data_runs_single_run() {
        // header 0x21: 1 字节长度 + 2 字节偏移；长度 0x20 clusters，起始 LCN +0x100
        let bytes = [0x21u8, 0x20, 0x00, 0x01, 0x00];
        let runs = parse_data_runs(&bytes).expect("解析 data runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].start_lcn, 0x100);
        assert_eq!(runs[0].cluster_count, 0x20);
    }

    #[test]
    fn parse_data_runs_relative_offset() {
        // 两段 run：LCN 0x100（+0x100，i16 [0x00,0x01]）→ LCN 0x80（-0x80，i16 [0x80,0xFF]）
        let bytes = [0x21u8, 0x10, 0x00, 0x01, 0x21, 0x08, 0x80, 0xFF, 0x00];
        let runs = parse_data_runs(&bytes).expect("解析相对偏移");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].start_lcn, 0x100);
        assert_eq!(runs[1].start_lcn, 0x80);
        assert_eq!(runs[1].cluster_count, 0x08);
    }

    #[test]
    fn read_signed_le_negative() {
        assert_eq!(read_signed_le(&[0x80]), -128);
        assert_eq!(read_signed_le(&[0xFF, 0xFF]), -1);
    }

    #[test]
    fn cloud_placeholder_mask() {
        assert_ne!(0x0004_0000 & CLOUD_PLACEHOLDER_ATTRIBUTES, 0);
        assert_ne!(0x0040_0000 & CLOUD_PLACEHOLDER_ATTRIBUTES, 0);
        assert_eq!(0x10 & CLOUD_PLACEHOLDER_ATTRIBUTES, 0);
    }
}
