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
    let query = normalize(query);
    if query.is_empty() || limit == 0 {
        return Vec::new();
    }

    #[derive(Clone, Copy, Eq, PartialEq)]
    struct Candidate {
        index: usize,
        score: i32,
        match_start: usize,
        name_len: usize,
    }

    // Reverse score order so `peek` is the current worst Top-K candidate.
    impl Ord for Candidate {
        fn cmp(&self, other: &Self) -> Ordering {
            other
                .score
                .cmp(&self.score)
                .then_with(|| self.name_len.cmp(&other.name_len))
                .then_with(|| self.index.cmp(&other.index))
        }
    }
    impl PartialOrd for Candidate {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }

    let mut candidates = BinaryHeap::<Candidate>::with_capacity(limit);
    for (index, entry) in entries.iter().enumerate() {
        if index & 0x7ff == 0 && latest_epoch.load(AtomicOrdering::Relaxed) != epoch {
            return Vec::new();
        }
        let Some(match_start) = entry.normalized_name.find(&query) else {
            continue;
        };
        let score = rank(entry, &query, match_start);
        let candidate = Candidate {
            index,
            score,
            match_start,
            name_len: entry.name.len(),
        };
        if candidates.len() < limit {
            candidates.push(candidate);
        } else if let Some(worst) = candidates.peek()
            && (candidate.score > worst.score
                || (candidate.score == worst.score && candidate.name_len < worst.name_len))
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

    score + entry.rank_bias - entry.name.chars().count().min(240) as i32
}

/// Walks only namespaces the current process can enumerate. Reparse targets are not followed.
pub fn scan_visible_roots<F>(roots: &[PathBuf], mut progress: F) -> Vec<FileEntry>
where
    F: FnMut(usize),
{
    let mut entries = Vec::new();
    let mut pending = roots.to_vec();
    let mut visited = HashSet::new();

    while let Some(directory) = pending.pop() {
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
            if file_type.is_dir() && !file_type.is_symlink() {
                pending.push(path);
            }
            if entries.len() % 10_000 == 0 {
                progress(entries.len());
            }
        }
    }
    entries
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
        if !roots.is_empty() {
            return roots;
        }
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
}

