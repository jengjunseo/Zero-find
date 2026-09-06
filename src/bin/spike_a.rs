use std::time::Instant;
use zerofind::mft::{enumerate_volume, process_metrics};

fn main() {
    let drive = std::env::args().nth(1).unwrap_or_else(|| "C:".to_string());
    let before = process_metrics();
    let started = Instant::now();
    match enumerate_volume(&drive) {
        Ok(entries) => {
            let elapsed = started.elapsed();
            let after = process_metrics();
            let cpu_ms = match (before, after) {
                (Some(a), Some(b)) => {
                    (b.cpu_time_100ns.saturating_sub(a.cpu_time_100ns)) as f64 / 10_000.0
                }
                _ => 0.0,
            };
            let peak = after.map(|m| m.peak_working_set_bytes).unwrap_or(0);
            let current = after.map(|m| m.working_set_bytes).unwrap_or(0);
            let per_second = entries.len() as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
            println!(
                "{{\"spike\":\"A\",\"status\":\"measured\",\"drive\":\"{}\",\"entries\":{},\"elapsed_ms\":{:.3},\"entries_per_sec\":{:.0},\"cpu_ms\":{:.3},\"peak_working_set_bytes\":{},\"working_set_bytes\":{}}}",
                drive.replace('"', ""),
                entries.len(),
                elapsed.as_secs_f64() * 1000.0,
                per_second,
                cpu_ms,
                peak,
                current
            );
        }
        Err(error) => {
            eprintln!(
                "{{\"spike\":\"A\",\"status\":\"not_measured\",\"drive\":\"{}\",\"error\":\"{}\",\"note\":\"FSCTL_ENUM_USN_DATA commonly requires elevation\"}}",
                drive.replace('"', ""),
                error.to_string().replace('"', "'")
            );
            std::process::exit(2);
        }
    }
}
