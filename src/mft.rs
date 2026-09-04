//! Windows-only measurement helpers for Spike A and fixed-volume discovery.

use crate::{EntryKind, FileEntry};
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, c_void};
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

type Handle = *mut c_void;
type Bool = i32;

const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
const GENERIC_READ: u32 = 0x8000_0000;
const FILE_SHARE_READ: u32 = 0x0000_0001;
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
const OPEN_EXISTING: u32 = 3;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const DRIVE_FIXED: u32 = 3;
const FSCTL_ENUM_USN_DATA: u32 = 0x0009_00b3;

#[repr(C)]
struct MftEnumDataV0 {
    start_file_reference_number: u64,
    low_usn: i64,
    high_usn: i64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FileTime {
    low: u32,
    high: u32,
}

#[repr(C)]
struct ProcessMemoryCounters {
    cb: u32,
    page_fault_count: u32,
    peak_working_set_size: usize,
    working_set_size: usize,
    quota_peak_paged_pool_usage: usize,
    quota_paged_pool_usage: usize,
    quota_peak_non_paged_pool_usage: usize,
    quota_non_paged_pool_usage: usize,
    pagefile_usage: usize,
    peak_pagefile_usage: usize,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template: Handle,
    ) -> Handle;
    fn DeviceIoControl(
        device: Handle,
        control_code: u32,
        in_buffer: *mut c_void,
        in_buffer_size: u32,
        out_buffer: *mut c_void,
        out_buffer_size: u32,
        bytes_returned: *mut u32,
        overlapped: *mut c_void,
    ) -> Bool;
    fn CloseHandle(handle: Handle) -> Bool;
    fn GetLogicalDrives() -> u32;
    fn GetDriveTypeW(root_path_name: *const u16) -> u32;
    fn GetVolumeInformationW(
        root_path_name: *const u16,
        volume_name: *mut u16,
        volume_name_size: u32,
        serial: *mut u32,
        maximum_component_length: *mut u32,
        file_system_flags: *mut u32,
        file_system_name: *mut u16,
        file_system_name_size: u32,
    ) -> Bool;
    fn GetCurrentProcess() -> Handle;
    fn GetProcessTimes(
        process: Handle,
        creation: *mut FileTime,
        exit: *mut FileTime,
        kernel: *mut FileTime,
        user: *mut FileTime,
    ) -> Bool;
}

#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: Handle,
        counters: *mut ProcessMemoryCounters,
        size: u32,
    ) -> Bool;
}

struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

#[derive(Clone)]
struct RawEntry {
    id: u64,
    parent: u64,
    name: String,
    attributes: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessMetrics {
    pub peak_working_set_bytes: usize,
    pub working_set_bytes: usize,
    pub cpu_time_100ns: u64,
}

pub fn process_metrics() -> Option<ProcessMetrics> {
    unsafe {
        let process = GetCurrentProcess();
        let mut counters: ProcessMemoryCounters = zeroed();
        counters.cb = size_of::<ProcessMemoryCounters>() as u32;
        if GetProcessMemoryInfo(process, &mut counters, counters.cb) == 0 {
            return None;
        }
        let mut creation = FileTime { low: 0, high: 0 };
        let mut exit = creation;
        let mut kernel = creation;
        let mut user = creation;
        if GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) == 0 {
            return None;
        }
        let filetime = |time: FileTime| (u64::from(time.high) << 32) | u64::from(time.low);
        Some(ProcessMetrics {
            peak_working_set_bytes: counters.peak_working_set_size,
            working_set_bytes: counters.working_set_size,
            cpu_time_100ns: filetime(kernel) + filetime(user),
        })
    }
}

pub fn fixed_ntfs_roots() -> Vec<PathBuf> {
    let mask = unsafe { GetLogicalDrives() };
    (0..26)
        .filter(|bit| mask & (1 << bit) != 0)
        .filter_map(|bit| {
            let letter = (b'A' + bit as u8) as char;
            let root = format!("{letter}:\\");
            let wide = wide_null(&root);
            if unsafe { GetDriveTypeW(wide.as_ptr()) } != DRIVE_FIXED {
                return None;
            }
            let mut fs_name = [0u16; 32];
            let ok = unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(),
                    null_mut(),
                    0,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    fs_name.as_mut_ptr(),
                    fs_name.len() as u32,
                )
            };
            if ok == 0 {
                return None;
            }
            let end = fs_name
                .iter()
                .position(|&x| x == 0)
                .unwrap_or(fs_name.len());
            let fs = String::from_utf16_lossy(&fs_name[..end]);
            fs.eq_ignore_ascii_case("NTFS").then(|| PathBuf::from(root))
        })
        .collect()
}

/// Enumerates one NTFS MFT via `FSCTL_ENUM_USN_DATA` and reconstructs its primary paths.
/// This is an evidence spike, not the product's visibility-filtered indexer.
pub fn enumerate_volume(drive: &str) -> io::Result<Vec<FileEntry>> {
    let drive = drive.trim_end_matches(['\\', '/']);
    let drive = if drive.ends_with(':') {
        drive.to_string()
    } else {
        format!("{drive}:")
    };
    let volume = wide_null(&format!(r"\\.\{drive}"));
    let handle = unsafe {
        CreateFileW(
            volume.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null_mut(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = OwnedHandle(handle);
    let mut request = MftEnumDataV0 {
        start_file_reference_number: 0,
        low_usn: 0,
        high_usn: i64::MAX,
    };
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut records = HashMap::<u64, RawEntry>::new();

    loop {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_ENUM_USN_DATA,
                (&mut request as *mut MftEnumDataV0).cast(),
                size_of::<MftEnumDataV0>() as u32,
                buffer.as_mut_ptr().cast(),
                buffer.len() as u32,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if matches!(error.raw_os_error(), Some(18) | Some(38)) {
                break;
            }
            return Err(error);
        }
        if returned <= 8 {
            break;
        }
        request.start_file_reference_number = read_u64(&buffer, 0).unwrap_or(u64::MAX);
        let mut offset = 8usize;
        while offset + 60 <= returned as usize {
            let Some(length) = read_u32(&buffer, offset).map(|x| x as usize) else {
                break;
            };
            if length < 60 || offset + length > returned as usize {
                break;
            }
            let major = read_u16(&buffer, offset + 4).unwrap_or(0);
            if major == 2 || major == 3 {
                let id = read_u64(&buffer, offset + 8).unwrap_or(0);
                let parent = read_u64(&buffer, offset + 16).unwrap_or(0);
                let attributes = read_u32(&buffer, offset + 52).unwrap_or(0);
                let name_len = read_u16(&buffer, offset + 56).unwrap_or(0) as usize;
                let name_offset = read_u16(&buffer, offset + 58).unwrap_or(0) as usize;
                if name_len.is_multiple_of(2) && name_offset + name_len <= length {
                    let start = offset + name_offset;
                    let wide_name: Vec<u16> = buffer[start..start + name_len]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                        .collect();
                    let name = String::from_utf16_lossy(&wide_name);
                    records.insert(
                        id,
                        RawEntry {
                            id,
                            parent,
                            name,
                            attributes,
                        },
                    );
                }
            }
            offset += length;
        }
        if request.start_file_reference_number == u64::MAX {
            break;
        }
    }

    let root = PathBuf::from(format!("{drive}\\"));
    let mut output = Vec::with_capacity(records.len());
    for item in records.values() {
        if item.name == "." || item.name.is_empty() {
            continue;
        }
        if let Some(path) = reconstruct(item.id, &records, &root) {
            let kind = if item.attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            if let Some(entry) = FileEntry::new(path, kind) {
                output.push(entry);
            }
        }
    }
    Ok(output)
}

fn reconstruct(id: u64, records: &HashMap<u64, RawEntry>, root: &Path) -> Option<PathBuf> {
    let mut current = id;
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    for _ in 0..1024 {
        if !seen.insert(current) {
            break;
        }
        let item = records.get(&current)?;
        if item.name != "." && !item.name.is_empty() {
            parts.push(item.name.as_str());
        }
        if item.parent == current || !records.contains_key(&item.parent) {
            break;
        }
        current = item.parent;
    }
    let mut path = root.to_path_buf();
    for part in parts.into_iter().rev() {
        path.push(part);
    }
    Some(path)
}

fn read_u16(buffer: &[u8], offset: usize) -> Option<u16> {
    let bytes = buffer.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32(buffer: &[u8], offset: usize) -> Option<u32> {
    let bytes = buffer.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(bytes.try_into().ok()?))
}

fn read_u64(buffer: &[u8], offset: usize) -> Option<u64> {
    let bytes = buffer.get(offset..offset + 8)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}

fn wide_null(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

