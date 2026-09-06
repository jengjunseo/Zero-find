//! Opt-in integration runner for this executable's own controls and file fixture.
//! Does not send input to any other application or open user files.
use super::*;
use std::io::Write;
struct Run {
    stage: usize,
    started: Instant,
    step: Instant,
    report: std::fs::File,
    cpu: u64,
}
static RUN: OnceLock<Mutex<Run>> = OnceLock::new();
static CAPTURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[link(name = "user32")]
unsafe extern "system" {
    fn GetWindowRgn(hwnd: Hwnd, region: Hregion) -> i32;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn PtInRegion(region: Hregion, x: i32, y: i32) -> i32;
}
pub fn start(hwnd: Hwnd) {
    let Some(path) = std::env::var_os("ZEROFIND_CERT_REPORT") else {
        return;
    };
    let Ok(report) = std::fs::File::create(path) else {
        return;
    };
    let _ = RUN.set(Mutex::new(Run {
        stage: 0,
        started: Instant::now(),
        step: Instant::now(),
        report,
        cpu: 0,
    }));
    unsafe {
        SetTimer(hwnd, 2, 16, null_mut());
    }
}
fn query(text: &str) {
    if let Some(edit) = with_state_value(|s| s.edit) {
        unsafe {
            SetWindowTextW(edit as Hwnd, wide(text).as_ptr());
        }
    }
}
pub fn tick(hwnd: Hwnd) {
    let Some(run) = RUN.get() else { return };
    let Ok(mut r) = run.lock() else { return };
    if r.step.elapsed().as_secs() > 15 {
        let stage = r.stage;
        let _ = writeln!(r.report, "FAIL stage={stage} timeout");
        unsafe {
            KillTimer(hwnd, 2);
            DestroyWindow(hwnd);
        }
        return;
    }
    let snapshot = with_state_value(|s| {
        (
            s.ready_count,
            s.hits.len(),
            s.searching,
            s.awaiting_paint,
            s.scroll,
            s.selected,
            s.motion,
            s.collapsed,
            s.last_paint_us,
        )
    });
    let Some((count, hits, searching, painting, scroll, selected, motion, collapsed, paint_us)) =
        snapshot
    else {
        return;
    };
    match r.stage {
        0 if count > 0 => {
            let elapsed = r.started.elapsed().as_micros();
            let _ = writeln!(
                r.report,
                "index_ready_after_ui_us={elapsed},entries={count}"
            );
            query("보고서");
        }
        1 if !searching && !painting && hits >= 20 && motion.height == motion.target_height => {
            CAPTURE.store(true, Ordering::Relaxed);
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            let _ = writeln!(
                r.report,
                "PASS korean_query,hits={hits},query_to_first_row_paint_us={paint_us}"
            );
            for _ in 0..15 {
                handle_keydown(hwnd, VK_DOWN);
            }
        }
        2 if selected == 15 && scroll > 0 => {
            let _ = writeln!(
                r.report,
                "PASS keyboard_scroll,selected={selected},scroll={scroll}"
            );
            query("배포자료");
        }
        3 if !searching && !painting && hits == 1 => {
            let _ = writeln!(
                r.report,
                "PASS one_result,query_to_first_row_paint_us={paint_us}"
            );
            query("가");
            query("가우");
            query("가우스");
        }
        4 if !searching && !painting && hits == 3 => {
            let _ = writeln!(
                r.report,
                "PASS superseded_queries_and_three_results,paint_us={paint_us}"
            );
            query("no-such-zerofind-certification-file");
        }
        5 if !searching && !painting && hits == 0 => {
            let _ = writeln!(r.report, "PASS no_results");
            collapse_island(hwnd);
        }
        6 if collapsed && motion.width < 700. => {
            let _ = writeln!(r.report, "PASS collapse_started");
            show_island(hwnd);
            query("보고서");
        }
        7 if !collapsed && !searching && !painting && motion.width == 720. => {
            let _ = writeln!(r.report, "PASS interrupted_collapse_and_reopen");
            collapse_island(hwnd);
        }
        8 if collapsed && motion.width == 64. && motion.height == 64. => {
            let region = unsafe { CreateRoundRectRgn(0, 0, 1, 1, 1, 1) };
            let shape = unsafe { GetWindowRgn(hwnd, region) };
            let corner = unsafe { PtInRegion(region, 0, 0) };
            let center = unsafe { PtInRegion(region, 32, 32) };
            unsafe {
                DeleteObject(region);
            }
            let _ = writeln!(
                r.report,
                "{} actual_window_region,shape={shape},corner={corner},center={center}",
                if shape > 0 && corner == 0 && center != 0 {
                    "PASS"
                } else {
                    "FAIL"
                }
            );
            if let Some(metrics) = zerofind::mft::process_metrics() {
                r.cpu = metrics.cpu_time_100ns;
                let _ = writeln!(
                    r.report,
                    "resident_working_set_bytes={}",
                    metrics.working_set_bytes
                );
            }
            unsafe {
                KillTimer(hwnd, 2);
                SetTimer(hwnd, 2, 3000, null_mut());
            }
        }
        9 => {
            if let Some(metrics) = zerofind::mft::process_metrics() {
                let cpu = metrics.cpu_time_100ns.saturating_sub(r.cpu);
                let elapsed = r.step.elapsed().as_secs_f64();
                let _ = writeln!(
                    r.report,
                    "idle_cpu_ms={},idle_wall_seconds={elapsed:.3}",
                    cpu as f64 / 10000.
                );
            }
            let _ = writeln!(r.report, "PASS executable_certification_complete");
            let _ = r.report.sync_all();
            unsafe {
                KillTimer(hwnd, 2);
                DestroyWindow(hwnd);
            }
            return;
        }
        _ => return,
    }
    r.stage += 1;
    r.step = Instant::now();
}

#[repr(C)]
struct BitmapHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bits: u16,
    compression: u32,
    image_size: u32,
    x: i32,
    y: i32,
    used: u32,
    important: u32,
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn GetDIBits(
        dc: Hdc,
        bitmap: Hgdiobj,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BitmapHeader,
        usage: u32,
    ) -> i32;
}
/// Diagnostic copy of our own paint buffer, not a desktop screenshot. It excludes
/// the native EDIT child and DWM backdrop; those still require live inspection.
pub fn capture_render(dc: Hdc, bitmap: Hgdiobj, width: i32, height: i32) {
    if !CAPTURE.swap(false, Ordering::Relaxed) {
        return;
    }
    if renderer::enabled() {
        return;
    }
    let Some(report) = std::env::var_os("ZEROFIND_CERT_REPORT") else {
        return;
    };
    if width <= 0 || height <= 0 || width > 8192 || height > 8192 {
        return;
    }
    let size = width as usize * height as usize * 4;
    let mut header = BitmapHeader {
        size: 40,
        width,
        height: -height,
        planes: 1,
        bits: 32,
        compression: 0,
        image_size: size as u32,
        x: 0,
        y: 0,
        used: 0,
        important: 0,
    };
    let mut pixels = vec![0u8; size];
    if unsafe {
        GetDIBits(
            dc,
            bitmap,
            0,
            height as u32,
            pixels.as_mut_ptr().cast(),
            &mut header,
            0,
        )
    } == 0
    {
        return;
    }
    let path = PathBuf::from(report).with_extension("bmp");
    let Ok(mut file) = std::fs::File::create(path) else {
        return;
    };
    let mut data = Vec::new();
    data.extend_from_slice(b"BM");
    data.extend_from_slice(&(54 + size as u32).to_le_bytes());
    data.extend_from_slice(&[0u8; 4]);
    data.extend_from_slice(&54u32.to_le_bytes());
    data.extend_from_slice(&40u32.to_le_bytes());
    data.extend_from_slice(&width.to_le_bytes());
    data.extend_from_slice(&(-height).to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&32u16.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(size as u32).to_le_bytes());
    data.extend_from_slice(&[0u8; 16]);
    data.extend_from_slice(&pixels);
    let _ = file.write_all(&data);
}
