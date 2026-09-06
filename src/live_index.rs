//! User-token directory notifications. Notifications are hints; visible filesystem
//! state is authoritative. Overflow triggers a replacement scan, never stale replay.
use std::{
    ffi::{OsString, c_void},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use zerofind::{EntryKind, FileEntry, is_plain_directory, scan_visible_roots};
static REFRESH: AtomicU64 = AtomicU64::new(0);
pub fn request_refresh() {
    REFRESH.fetch_add(1, Ordering::Relaxed);
}
#[derive(Clone, Copy)]
pub enum Event {
    Ready,
    Progress,
    Error,
}
type Handle = *mut c_void;
#[repr(C)]
struct Overlapped {
    internal: usize,
    internal_high: usize,
    offset: u32,
    offset_high: u32,
    event: Handle,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *mut c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CreateEventW(security: *mut c_void, manual: i32, initial: i32, name: *const u16) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
    fn ReadDirectoryChangesW(
        handle: Handle,
        buffer: *mut c_void,
        len: u32,
        subtree: i32,
        filter: u32,
        returned: *mut u32,
        overlap: *mut Overlapped,
        completion: *mut c_void,
    ) -> i32;
    fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    fn GetOverlappedResult(
        handle: Handle,
        overlap: *mut Overlapped,
        bytes: *mut u32,
        wait: i32,
    ) -> i32;
    fn CancelIoEx(handle: Handle, overlap: *mut Overlapped) -> i32;
    fn ResetEvent(handle: Handle) -> i32;
}
struct Owned(Handle);
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn start(
    roots: Vec<PathBuf>,
    entries: Arc<RwLock<Vec<FileEntry>>>,
    report: impl Fn(Event, usize) + Send + Sync + 'static,
) {
    let report = Arc::new(report);
    if roots.is_empty() {
        report(Event::Error, 0);
        return;
    }
    // Remove duplicate and nested roots: one owner for each indexed subtree.
    let mut roots = roots
        .into_iter()
        .filter(|p| is_plain_directory(p))
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|p| !all.iter().any(|other| p != other && p.starts_with(other)));
    if roots.is_empty() {
        report(Event::Error, 0);
        return;
    }
    for root in roots {
        let entries = entries.clone();
        let report = report.clone();
        std::thread::spawn(move || {
            loop {
                if watch(&root, &entries, &*report).is_err() {
                    if let Ok(mut map) = entries.write() {
                        map.retain(|e| !e.path.starts_with(&root));
                    }
                    report(Event::Error, 0);
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    }
}
fn replace(root: &Path, entries: &RwLock<Vec<FileEntry>>, report: &impl Fn(Event, usize)) {
    let fresh = scan_visible_roots(&[root.to_path_buf()], |n| report(Event::Progress, n));
    if let Ok(mut map) = entries.write() {
        map.retain(|e| !e.path.starts_with(root));
        map.extend(fresh);
        report(Event::Ready, map.len());
    }
}
fn watch(
    root: &Path,
    entries: &RwLock<Vec<FileEntry>>,
    report: &impl Fn(Event, usize),
) -> Result<(), ()> {
    if !is_plain_directory(root) {
        return Err(());
    }
    let name = root
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            1,
            7,
            std::ptr::null_mut(),
            3,
            0x4200_0000,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(());
    }
    let handle = Owned(handle);
    let event = Owned(unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) });
    if event.0.is_null() {
        return Err(());
    }
    let mut overlap = Overlapped {
        internal: 0,
        internal_high: 0,
        offset: 0,
        offset_high: 0,
        event: event.0,
    };
    let mut buffer = vec![0u32; 16384];
    let mut initial = true;
    let mut generation = REFRESH.load(Ordering::Relaxed);
    loop {
        unsafe {
            ResetEvent(event.0);
        }
        if unsafe {
            ReadDirectoryChangesW(
                handle.0,
                buffer.as_mut_ptr().cast(),
                65536,
                1,
                0x1 | 0x2 | 0x100,
                std::ptr::null_mut(),
                &mut overlap,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(());
        }
        if initial {
            // Watch is armed before bootstrap, so changes during the walk are replayed.
            zerofind::scan_visible_batches(&[root.to_path_buf()], |batch| {
                if let Ok(mut map) = entries.write() {
                    map.extend(batch);
                    report(Event::Ready, map.len());
                }
            });
            if let Ok(map) = entries.read() {
                report(Event::Ready, map.len());
            }
            initial = false;
        }
        loop {
            let wait = unsafe { WaitForSingleObject(event.0, 1000) };
            if wait == 0 {
                break;
            }
            if wait != 258 {
                unsafe {
                    CancelIoEx(handle.0, &mut overlap);
                    let mut ignored = 0;
                    GetOverlappedResult(handle.0, &mut overlap, &mut ignored, 1);
                }
                return Err(());
            }
            let latest = REFRESH.load(Ordering::Relaxed);
            if latest != generation {
                generation = latest;
                replace(root, entries, report);
            }
        }
        let mut bytes = 0;
        if unsafe { GetOverlappedResult(handle.0, &mut overlap, &mut bytes, 0) } == 0 {
            return Err(());
        }
        // Byte slicing is bounded by the actual allocated buffer; records parsed without casts.
        if bytes == 0 || bytes > 65536 {
            replace(root, entries, report);
            continue;
        }
        let raw =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<u8>(), bytes as usize) };
        let Some(names) = parse_names(raw) else {
            replace(root, entries, report);
            continue;
        };
        let mut updates = Vec::new();
        for name in names {
            let relative = PathBuf::from(name);
            if relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                continue;
            }
            let path = root.join(relative);
            let mut fresh = Vec::new();
            if visible_parent(root, &path)
                && let Ok(meta) = std::fs::symlink_metadata(&path)
            {
                let kind = if meta.is_dir() {
                    EntryKind::Directory
                } else {
                    EntryKind::File
                };
                if let Some(entry) = FileEntry::new(path.clone(), kind) {
                    fresh.push(entry);
                }
                if is_plain_directory(&path) {
                    fresh.extend(scan_visible_roots(std::slice::from_ref(&path), |_| {}));
                }
            }
            updates.push((path, fresh));
        }
        if let Ok(mut map) = entries.write() {
            // Compare native path components, not lossy strings. Notification
            // old/new names retain the casing from directory enumeration.
            for (path, fresh) in updates {
                map.retain(|e| !e.path.starts_with(&path));
                map.extend(fresh);
            }
            report(Event::Ready, map.len());
        }
    }
}
fn visible_parent(root: &Path, path: &Path) -> bool {
    let mut current = path.parent();
    while let Some(parent) = current {
        if !parent.starts_with(root) {
            return false;
        }
        if !is_plain_directory(parent) || std::fs::read_dir(parent).is_err() {
            return false;
        }
        if parent == root {
            return true;
        }
        current = parent.parent();
    }
    false
}
fn parse_names(bytes: &[u8]) -> Option<Vec<OsString>> {
    let mut names = Vec::new();
    let mut offset: usize = 0;
    loop {
        let header = bytes.get(offset..offset.checked_add(12)?)?;
        let next = u32::from_le_bytes(header[0..4].try_into().ok()?) as usize;
        let len = u32::from_le_bytes(header[8..12].try_into().ok()?) as usize;
        if !len.is_multiple_of(2) {
            return None;
        }
        let end = offset.checked_add(12)?.checked_add(len)?;
        let name = bytes
            .get(offset + 12..end)?
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>();
        names.push(OsString::from_wide(&name));
        if next == 0 {
            return Some(names);
        }
        if next < 12 + len || !next.is_multiple_of(4) {
            return None;
        }
        offset = offset.checked_add(next)?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_create_rename_delete_and_directory_move_are_searchable() {
        let root = std::env::temp_dir().join(format!("zerofind-watch-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let entries = Arc::new(RwLock::new(Vec::new()));
        let (tx, rx) = std::sync::mpsc::channel();
        start(vec![root.clone()], entries.clone(), move |event, _| {
            if matches!(event, Event::Ready) {
                let _ = tx.send(());
            }
        });
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let wait = |name: &str, present: bool| {
            let started = std::time::Instant::now();
            loop {
                let found = entries.read().unwrap().iter().any(|e| e.name == name);
                if found == present {
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(10),
                    "freshness timeout: {name}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        let created = std::time::Instant::now();
        std::fs::write(root.join("가우스.txt"), "fixture").unwrap();
        wait("가우스.txt", true);
        println!("create_searchable_us={}", created.elapsed().as_micros());
        let renamed = std::time::Instant::now();
        std::fs::rename(root.join("가우스.txt"), root.join("보고서.txt")).unwrap();
        wait("가우스.txt", false);
        wait("보고서.txt", true);
        println!("rename_updated_us={}", renamed.elapsed().as_micros());
        std::fs::create_dir(root.join("before")).unwrap();
        std::fs::write(root.join("before/배포자료.txt"), "fixture").unwrap();
        wait("배포자료.txt", true);
        std::fs::rename(root.join("before"), root.join("after")).unwrap();
        wait("before", false);
        wait("after", true);
        let started = std::time::Instant::now();
        while !entries
            .read()
            .unwrap()
            .iter()
            .any(|e| e.name == "배포자료.txt" && e.path.parent().unwrap().ends_with("after"))
        {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        let deleted = std::time::Instant::now();
        std::fs::remove_file(root.join("보고서.txt")).unwrap();
        wait("보고서.txt", false);
        println!("delete_disappeared_us={}", deleted.elapsed().as_micros());
        std::fs::remove_dir_all(root.join("after")).unwrap();
        wait("배포자료.txt", false);
        // Only this test's uniquely named temporary fixture is removed.
        std::fs::remove_dir(&root).unwrap();
    }
    #[test]
    fn malformed_notification_is_rejected() {
        assert!(parse_names(&[0; 11]).is_none());
        let mut b = vec![0u8; 12];
        b[8] = 255;
        assert!(parse_names(&b).is_none());
        b[8] = 0;
        b[0] = 4;
        assert!(parse_names(&b).is_none());
    }
}
