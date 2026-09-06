use std::hint::black_box;
use std::path::PathBuf;
use std::time::Instant;
use zerofind::{EntryKind, FileEntry, percentile_ns, search};

pub fn fixture(size: usize) -> Vec<FileEntry> {
    let seeds = [
        "PROJECT_GAUSS.pdf",
        "PROJECT_GAUSS_정리.hwp",
        "Gauss",
        "Gauss.exe",
        "GaussInstaller.exe",
        "Gauss Setup.msi",
        "Gauss.lnk",
        "문서_가우스_노트.txt",
        "annual_project_report.xlsx",
        "photography_archive.jpg",
        "readme.md",
        "계약서_문서.pdf",
    ];
    (0..size)
        .filter_map(|index| {
            let seed = seeds[index % seeds.len()];
            let path = PathBuf::from(format!(
                r"C:\fixture\group_{:05}\{:07}_{}",
                index / 500,
                index,
                seed
            ));
            let kind = if seed == "Gauss" {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            FileEntry::new(path, kind)
        })
        .collect()
}

fn main() {
    let size = std::env::args()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(100_000usize);
    let entries = fixture(size);
    let queries = [
        "gauss",
        "project",
        "a",
        "1",
        "가우스",
        "문서",
        "annual_project_report",
    ];
    let iterations = if size >= 1_000_000 { 80 } else { 240 };
    println!(
        "{{\"spike\":\"B\",\"status\":\"measured\",\"dataset\":\"deterministic multilingual fixture\",\"entries\":{},\"iterations\":{},\"queries\":[",
        entries.len(),
        iterations
    );
    for (query_index, query) in queries.iter().enumerate() {
        for _ in 0..12 {
            black_box(search(&entries, query, 20));
        }
        let mut samples = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            let hits = black_box(search(black_box(&entries), black_box(query), 20));
            black_box(hits);
            samples.push(start.elapsed().as_nanos());
        }
        let mut percentile_samples = samples.clone();
        let p50 = percentile_ns(&mut percentile_samples, 0.50);
        let p95 = percentile_ns(&mut percentile_samples, 0.95);
        let p99 = percentile_ns(&mut percentile_samples, 0.99);
        let max = *samples.iter().max().unwrap_or(&0);
        println!(
            "{{\"query\":\"{}\",\"p50_us\":{:.3},\"p95_us\":{:.3},\"p99_us\":{:.3},\"max_us\":{:.3}}}{}",
            query,
            p50 as f64 / 1000.0,
            p95 as f64 / 1000.0,
            p99 as f64 / 1000.0,
            max as f64 / 1000.0,
            if query_index + 1 == queries.len() {
                ""
            } else {
                ","
            }
        );
    }
    println!("]}}");
}
