use std::collections::HashMap;
use std::hint::black_box;
use std::time::Instant;
use zerofind::percentile_ns;

fn naive(parents: &[usize], candidates: &[usize]) -> usize {
    let mut walks = 0;
    for &candidate in candidates {
        let mut node = candidate;
        while node != 0 {
            walks += 1;
            node = parents[node];
        }
    }
    walks
}

fn memoized(parents: &[usize], candidates: &[usize]) -> (usize, usize) {
    let mut visible = HashMap::new();
    visible.insert(0, true);
    let mut walks = 0;
    for &candidate in candidates {
        let mut node = candidate;
        let mut trail = Vec::new();
        while !visible.contains_key(&node) {
            walks += 1;
            trail.push(node);
            node = parents[node];
        }
        for item in trail {
            visible.insert(item, true);
        }
    }
    (walks, visible.len())
}

fn main() {
    let depths = [5usize, 10, 25, 100, 500];
    let candidate_counts = [20usize, 50, 100];
    let iterations = 600;
    println!(
        "{{\"spike\":\"C\",\"status\":\"measured\",\"iterations\":{},\"results\":[",
        iterations
    );
    let mut row = 0;
    for &depth in &depths {
        for &count in &candidate_counts {
            let mut parents = vec![0usize; depth * count + 1];
            for branch in 0..count {
                let base = branch * depth;
                for level in 1..=depth {
                    parents[base + level] = if level == 1 { 0 } else { base + level - 1 };
                }
            }
            let shared: Vec<_> = (0..count).map(|_| depth).collect();
            let mixed: Vec<_> = (0..count)
                .map(|index| {
                    if index < count / 2 {
                        depth
                    } else {
                        index * depth + depth
                    }
                })
                .collect();
            let unrelated: Vec<_> = (0..count).map(|i| i * depth + depth).collect();
            for (locality, candidates) in [
                ("shared", shared),
                ("mixed", mixed),
                ("unrelated", unrelated),
            ] {
                let mut naive_samples = Vec::with_capacity(iterations);
                let mut memo_samples = Vec::with_capacity(iterations);
                let naive_walks = naive(&parents, &candidates);
                let (memo_walks, unique) = memoized(&parents, &candidates);
                for _ in 0..iterations {
                    let started = Instant::now();
                    black_box(naive(black_box(&parents), black_box(&candidates)));
                    naive_samples.push(started.elapsed().as_nanos());
                    let started = Instant::now();
                    black_box(memoized(black_box(&parents), black_box(&candidates)));
                    memo_samples.push(started.elapsed().as_nanos());
                }
                let naive_p99 = percentile_ns(&mut naive_samples, 0.99);
                let memo_p99 = percentile_ns(&mut memo_samples, 0.99);
                if row > 0 {
                    println!(",");
                }
                print!(
                    "{{\"depth\":{},\"candidates\":{},\"locality\":\"{}\",\"naive_p99_us\":{:.3},\"memo_p99_us\":{:.3},\"naive_walks\":{},\"memo_walks\":{},\"unique_ancestors\":{}}}",
                    depth,
                    count,
                    locality,
                    naive_p99 as f64 / 1000.0,
                    memo_p99 as f64 / 1000.0,
                    naive_walks,
                    memo_walks,
                    unique
                );
                row += 1;
            }
        }
    }
    println!("]}}");
}

