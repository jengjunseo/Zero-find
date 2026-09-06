//! Alternating paired comparison isolates query preprocessing from ranking changes.
#[allow(dead_code)]
#[path = "spike_b.rs"]
mod fixture;
use std::{hint::black_box, time::Instant};
use zerofind::{percentile_ns, search, search_reference};
fn main() {
    let count = std::env::args()
        .nth(1)
        .and_then(|v| v.parse().ok())
        .unwrap_or(100_000);
    let limit = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let started = Instant::now();
    let entries = fixture::fixture(count);
    let build_us = started.elapsed().as_micros();
    let queries = [
        "gauss",
        "project",
        "a",
        "1",
        "가우스",
        "문서",
        "annual_project_report",
        "not-present",
        "0000999",
    ];
    println!(
        "{{\"entries\":{count},\"limit\":{limit},\"fixture_build_us\":{build_us},\"iterations\":80,\"method\":\"alternating paired, identical ranking\",\"queries\":["
    );
    for (qi, query) in queries.iter().enumerate() {
        let expected = search_reference(&entries, query, limit)
            .into_iter()
            .map(|h| (h.entry.path, h.score, h.match_start))
            .collect::<Vec<_>>();
        assert_eq!(
            expected,
            search(&entries, query, limit)
                .into_iter()
                .map(|h| (h.entry.path, h.score, h.match_start))
                .collect::<Vec<_>>()
        );
        let mut reference = Vec::new();
        let mut candidate = Vec::new();
        for iteration in 0..88 {
            for variant in 0..2 {
                let reuse = (iteration + variant) % 2 == 0;
                let start = Instant::now();
                black_box(if reuse {
                    search(black_box(&entries), query, limit)
                } else {
                    search_reference(black_box(&entries), query, limit)
                });
                let ns = start.elapsed().as_nanos();
                if iteration >= 8 {
                    if reuse {
                        candidate.push(ns);
                    } else {
                        reference.push(ns);
                    }
                }
            }
        }
        println!(
            "{{\"query\":\"{query}\",\"reference_p50_us\":{:.3},\"candidate_p50_us\":{:.3},\"reference_p99_us\":{:.3},\"candidate_p99_us\":{:.3}}}{}",
            percentile_ns(&mut reference, 0.5) as f64 / 1000.,
            percentile_ns(&mut candidate, 0.5) as f64 / 1000.,
            percentile_ns(&mut reference, 0.99) as f64 / 1000.,
            percentile_ns(&mut candidate, 0.99) as f64 / 1000.,
            if qi + 1 == queries.len() { "" } else { "," }
        );
    }
    println!("]}}");
}
