//! ZeroFind's deliberately small, measurable search core.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

#[cfg(windows)]
pub mod mft;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    File,
    Directory,
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    pub normalized_name: String,
    pub path: PathBuf,
    pub kind: EntryKind,
    rank_bias: i32,
}

impl FileEntry {
    pub fn new(path: PathBuf, kind: EntryKind) -> Option<Self> {
        let name = path.file_name()?.to_string_lossy().into_owned();
        if name.is_empty() {
            return None;
        }
        let normalized_name = normalize(&name);
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        let mut rank_bias = match kind {
            EntryKind::File => 650,
            EntryKind::Directory => 550,
        };
        if ["exe", "msi", "lnk", "com", "bat", "cmd"]
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
        {
            rank_bias -= 2_200;
        }
        if normalized_name.starts_with("setup")
            || normalized_name.starts_with("installer")
            || normalized_name.starts_with("install_")
            || normalized_name.starts_with("uninstall")
        {
            rank_bias -= 1_800;
        }
        rank_bias -= name.chars().count().min(240) as i32;
        Some(Self {
            name,
            normalized_name,
            path,
            kind,
            rank_bias,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SearchHit {
    pub entry: FileEntry,
    pub score: i32,
    pub match_start: usize,
}

pub fn normalize(text: &str) -> String {
    text.trim().to_lowercase()
}

pub fn search(entries: &[FileEntry], query: &str, limit: usize) -> Vec<SearchHit> {
    search_cancellable(entries, query, limit, 0, &AtomicU64::new(0))
}

pub fn search_cancellable(
    entries: &[FileEntry],
    query: &str,
    limit: usize,
    epoch: u64,
    latest_epoch: &AtomicU64,
) -> Vec<SearchHit> {
    search_impl(entries, query, limit, epoch, latest_epoch, true)
}

/// Measurement control: identical ranking and heap with std substring matching.
#[doc(hidden)]
pub fn search_reference(entries: &[FileEntry], query: &str, limit: usize) -> Vec<SearchHit> {
    search_impl(entries, query, limit, 0, &AtomicU64::new(0), false)
}

fn search_impl(
    entries: &[FileEntry],
    query: &str,
    limit: usize,
    epoch: u64,
    latest_epoch: &AtomicU64,
    reuse: bool,
) -> Vec<SearchHit> {
    let query = normalize(query);
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    struct Candidate<'a> {
        index: usize,
        score: i32,
        match_start: usize,
        name_len: usize,
        name_key: &'a str,
        path_key: &'a Path,
    }

    // Reverse score order so `peek` is the current worst Top-K candidate.
    impl Ord for Candidate<'_> {
        fn cmp(&self, other: &Self) -> Ordering {
            other
                .score
                .cmp(&self.score)
                .then_with(|| self.name_len.cmp(&other.name_len))
                .then_with(|| self.name_key.cmp(other.name_key))
                .then_with(|| self.path_key.cmp(other.path_key))
        }
    }
    impl PartialOrd for Candidate<'_> {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut candidates = BinaryHeap::<Candidate>::with_capacity(limit);
    // Reuse query preprocessing across filenames. Both strings are valid UTF-8
    // and the needle is nonempty, so matches have the same boundaries as str::find.
    let finder = memchr::memmem::Finder::new(query.as_bytes());
    for (index, entry) in entries.iter().enumerate() {
        if index & 0x7ff == 0 && latest_epoch.load(AtomicOrdering::Relaxed) != epoch {
            return Vec::new();
        }
        let Some(match_start) = (if reuse {
            finder.find(entry.normalized_name.as_bytes())
        } else {
            entry.normalized_name.find(&query)
        }) else {
            continue;
        };
        let score = rank(entry, &query, match_start);
        let candidate = Candidate {
            index,
            score,
            match_start,
            name_len: entry.name.len(),
            name_key: &entry.normalized_name,
            path_key: &entry.path,
        };
        if candidates.len() < limit {
            candidates.push(candidate);
        } else if let Some(worst) = candidates.peek()
            && candidate < *worst
        {
            candidates.pop();
            candidates.push(candidate);
        }
    }

    let mut hits: Vec<_> = candidates
        .into_iter()
        .map(|candidate| SearchHit {
            entry: entries[candidate.index].clone(),
            score: candidate.score,
            match_start: candidate.match_start,
        })
        .collect();
    hits.sort_unstable_by(compare_hits);
    hits
}

fn compare_hits(left: &SearchHit, right: &SearchHit) -> Ordering {
    right
        .score
        .cmp(&left.score)
        .then_with(|| left.entry.name.len().cmp(&right.entry.name.len()))
        .then_with(|| left.entry.normalized_name.cmp(&right.entry.normalized_name))
        .then_with(|| left.entry.path.cmp(&right.entry.path))
}

fn rank(entry: &FileEntry, query: &str, match_start: usize) -> i32 {
    let mut score = if entry.normalized_name == query {
        12_000
    } else if match_start == 0 {
        9_000
    } else {
        6_000 - (match_start.min(400) as i32 * 4)
    };

    if match_start > 0 {
        let boundary = entry.normalized_name[..match_start]
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_alphanumeric());
        if boundary {
            score += 850;
        }
    }

    score + entry.rank_bias
}

/// Walks only namespaces the current process can enumerate. Reparse targets are not followed.
pub fn scan_visible_roots<F>(roots: &[PathBuf], mut progress: F) -> Vec<FileEntry>
where
    F: FnMut(usize),
{
    let mut entries = Vec::new();
    scan_visible_batches(roots, |batch| {
        entries.extend(batch);
        progress(entries.len());
    });
    entries
}

/// Streams bounded visible batches so the UI can search while bootstrapping.
pub fn scan_visible_batches<F>(roots: &[PathBuf], mut publish: F)
where
    F: FnMut(Vec<FileEntry>),
{
    let mut entries = Vec::new();
    let mut pending = roots.to_vec();
    let mut visited = HashSet::new();

    while let Some(directory) = pending.pop() {
        // Reject junctions as well as symlinks, including explicitly supplied roots.
        if !is_plain_directory(&directory) {
            continue;
        }
        let key = normalize(&directory.to_string_lossy());
        if !visited.insert(key) {
            continue;
        }
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            let path = child.path();
            let Ok(file_type) = child.file_type() else {
                continue;
            };
            let kind = if file_type.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            if let Some(entry) = FileEntry::new(path.clone(), kind) {
                entries.push(entry);
            }
            if file_type.is_dir() && is_plain_directory(&path) {
                pending.push(path);
            }
            if entries.len() >= 10_000 {
                publish(std::mem::take(&mut entries));
            }
        }
    }
    if !entries.is_empty() {
        publish(entries);
    }
}

pub fn is_plain_directory(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return false;
    };
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.is_dir() && metadata.file_attributes() & 0x400 == 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_dir() && !metadata.is_symlink()
    }
}

pub fn roots_from_environment() -> Vec<PathBuf> {
    if let Some(value) = std::env::var_os("ZEROFIND_SCAN_ROOTS") {
        let roots: Vec<_> = value
            .to_string_lossy()
            .split(';')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(PathBuf::from)
            .filter(|path| path.exists())
            .collect();
        // An invalid diagnostic scope must never silently expand to all drives.
        return roots;
    }

    #[cfg(windows)]
    {
        let roots = mft::fixed_ntfs_roots();
        if !roots.is_empty() {
            return roots;
        }
    }

    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .into_iter()
        .collect()
}

pub fn parent_to_open(path: &Path, kind: EntryKind) -> PathBuf {
    if kind == EntryKind::Directory {
        path.to_path_buf()
    } else {
        path.parent().unwrap_or(path).to_path_buf()
    }
}

pub fn percentile_ns(samples: &mut [u128], percentile: f64) -> u128 {
    if samples.is_empty() {
        return 0;
    }
    samples.sort_unstable();
    let position = ((samples.len() - 1) as f64 * percentile).ceil() as usize;
    samples[position.min(samples.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, kind: EntryKind) -> FileEntry {
        FileEntry::new(PathBuf::from(path), kind).unwrap()
    }

    #[test]
    fn korean_substring_search_is_unicode_safe() {
        let entries = vec![entry(r"C:\문서\가우스 정리.hwp", EntryKind::File)];
        let hits = search(&entries, "가우스", 20);
        assert_eq!(hits[0].entry.name, "가우스 정리.hwp");
    }

    #[test]
    fn file_first_ranking_demotes_installers() {
        let entries = vec![
            entry(r"C:\GaussInstaller.exe", EntryKind::File),
            entry(r"C:\PROJECT_GAUSS.pdf", EntryKind::File),
            entry(r"C:\Gauss", EntryKind::Directory),
        ];
        let hits = search(&entries, "gauss", 20);
        assert_eq!(hits.last().unwrap().entry.name, "GaussInstaller.exe");
    }

    #[test]
    fn exact_match_wins() {
        let entries = vec![
            entry(r"C:\project-notes.txt", EntryKind::File),
            entry(r"C:\project", EntryKind::Directory),
        ];
        assert_eq!(search(&entries, "project", 20)[0].entry.name, "project");
    }

    #[test]
    fn reused_finder_agrees_with_unicode_string_search() {
        let names = [
            "가우스 보고서.txt",
            "배포자료🦀.pdf",
            "İstanbul_문서",
            "a\u{301} & b_가.txt",
            "👩‍💻.txt",
        ];
        for name in names {
            let text = normalize(name);
            let boundaries = text
                .char_indices()
                .map(|(i, _)| i)
                .chain(Some(text.len()))
                .collect::<Vec<_>>();
            for &a in &boundaries {
                for &b in &boundaries {
                    if a >= b {
                        continue;
                    }
                    let needle = &text[a..b];
                    let finder = memchr::memmem::Finder::new(needle.as_bytes());
                    assert_eq!(finder.find(text.as_bytes()), text.find(needle));
                }
            }
        }
    }
    #[test]
    fn limited_ties_are_independent_of_enumeration_order() {
        let mut entries = (0..60)
            .rev()
            .map(|i| entry(&format!(r"C:\group{i:02}\report.txt"), EntryKind::File))
            .collect::<Vec<_>>();
        let first = search(&entries, "report", 20)
            .into_iter()
            .map(|h| h.entry.path)
            .collect::<Vec<_>>();
        entries.reverse();
        assert_eq!(
            first,
            search(&entries, "report", 20)
                .into_iter()
                .map(|h| h.entry.path)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn cancelled_queries_and_empty_limits_have_no_results() {
        let entries = vec![entry(r"C:\보고서.txt", EntryKind::File)];
        assert!(search_cancellable(&entries, "보고서", 20, 1, &AtomicU64::new(2)).is_empty());
        assert!(search(&entries, "보고서", 0).is_empty());
        assert!(search(&entries, " ", 20).is_empty());
    }
}
