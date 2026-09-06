#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod certify;
mod live_index;
mod motion;
mod renderer;

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::Instant;
use zerofind::{EntryKind, FileEntry, SearchHit, roots_from_environment, search_cancellable};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hdc = *mut c_void;
type Hgdiobj = *mut c_void;
type Hbrush = *mut c_void;
type Hregion = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;

const WM_COMMAND: u32 = 0x0111;
const WM_ACTIVATEAPP: u32 = 0x001c;
const WM_MOUSEWHEEL: u32 = 0x020a;
const WM_DPICHANGED: u32 = 0x02e0;
const WM_CTLCOLOREDIT: u32 = 0x0133;
const WM_APP_METADATA: u32 = 0x8004;
const WM_APP_INDEX_ERROR: u32 = 0x8005;
const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000f;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_TIMER: u32 = 0x0113;
const WM_KEYDOWN: u32 = 0x0100;
const WM_CHAR: u32 = 0x0102;
const WM_HOTKEY: u32 = 0x0312;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_NCHITTEST: u32 = 0x0084;
const WM_APP_INDEX_PROGRESS: u32 = 0x8001;
const WM_APP_INDEX_READY: u32 = 0x8002;
const WM_APP_SEARCH_READY: u32 = 0x8003;

const VK_RETURN: usize = 0x0d;
const VK_ESCAPE: usize = 0x1b;
const VK_UP: usize = 0x26;
const VK_DOWN: usize = 0x28;
const VK_F2: usize = 0x71;
const VK_MENU: i32 = 0x12;
const VK_CONTROL: i32 = 0x11;
const VK_SHIFT: i32 = 0x10;
const VK_LWIN: i32 = 0x5b;
const VK_RWIN: i32 = 0x5c;

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_WIN: u32 = 0x0008;
const MOD_NOREPEAT: u32 = 0x4000;

const WS_POPUP: u32 = 0x8000_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SWP_NOACTIVATE: u32 = 0x0010;
const LWA_ALPHA: u32 = 0x0000_0002;
const DT_LEFT: u32 = 0x0000;
const DT_CENTER: u32 = 0x0001;
const DT_VCENTER: u32 = 0x0004;
const DT_SINGLELINE: u32 = 0x0020;
const DT_END_ELLIPSIS: u32 = 0x8000;
const TRANSPARENT: i32 = 1;
const SRCCOPY: u32 = 0x00cc_0020;
const FW_NORMAL: i32 = 400;
const FW_SEMIBOLD: i32 = 600;
const DEFAULT_CHARSET: u32 = 1;
const CLEARTYPE_QUALITY: u32 = 5;
const HTCAPTION: isize = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PaintStruct {
    hdc: Hdc,
    erase: i32,
    paint: Rect,
    restore: i32,
    inc_update: i32,
    reserved: [u8; 32],
}

#[repr(C)]
struct Msg {
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
    time: u32,
    point: Point,
    private: u32,
}

#[repr(C)]
struct WndClassExW {
    size: u32,
    style: u32,
    wnd_proc: Option<unsafe extern "system" fn(Hwnd, u32, Wparam, Lparam) -> Lresult>,
    class_extra: i32,
    window_extra: i32,
    instance: Hinstance,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: Hbrush,
    menu_name: *const u16,
    class_name: *const u16,
    icon_small: *mut c_void,
}

#[repr(C)]
struct Margins {
    left: i32,
    right: i32,
    top: i32,
    bottom: i32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn RegisterClassExW(class: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        title: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: Hinstance,
        param: *mut c_void,
    ) -> Hwnd;
    fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> Lresult;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn SetForegroundWindow(hwnd: Hwnd) -> i32;
    fn SetFocus(hwnd: Hwnd) -> Hwnd;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn GetMessageW(message: *mut Msg, hwnd: Hwnd, min: u32, max: u32) -> i32;
    fn TranslateMessage(message: *const Msg) -> i32;
    fn DispatchMessageW(message: *const Msg) -> Lresult;
    fn PostQuitMessage(exit_code: i32);
    fn PostMessageW(hwnd: Hwnd, message: u32, wparam: Wparam, lparam: Lparam) -> i32;
    fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> Hdc;
    fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
    fn LoadCursorW(instance: Hinstance, cursor_name: *const u16) -> *mut c_void;
    fn RegisterHotKey(hwnd: Hwnd, id: i32, modifiers: u32, key: u32) -> i32;
    fn UnregisterHotKey(hwnd: Hwnd, id: i32) -> i32;
    fn GetKeyState(key: i32) -> i16;
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetWindowPos(
        hwnd: Hwnd,
        insert_after: Hwnd,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        flags: u32,
    ) -> i32;
    fn SetLayeredWindowAttributes(hwnd: Hwnd, color_key: u32, alpha: u8, flags: u32) -> i32;
    fn SetTimer(hwnd: Hwnd, id: usize, milliseconds: u32, callback: *mut c_void) -> usize;
    fn KillTimer(hwnd: Hwnd, id: usize) -> i32;
    fn SetWindowRgn(hwnd: Hwnd, region: Hregion, redraw: i32) -> i32;
    fn SetProcessDpiAwarenessContext(context: *mut c_void) -> i32;
}

#[repr(C)]
struct MonitorInfo {
    size: u32,
    monitor: Rect,
    work: Rect,
    flags: u32,
}
#[link(name = "user32")]
unsafe extern "system" {
    fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetWindowTextLengthW(hwnd: Hwnd) -> i32;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
    fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
    fn CallWindowProcW(
        proc: isize,
        hwnd: Hwnd,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn GetForegroundWindow() -> Hwnd;
    fn MonitorFromWindow(hwnd: Hwnd, flags: u32) -> *mut c_void;
    fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
    fn SystemParametersInfoW(action: u32, param: u32, value: *mut c_void, flags: u32) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
}
#[link(name = "gdi32")]
unsafe extern "system" {
    fn SetBkColor(hdc: Hdc, color: u32) -> u32;
    fn SetMapMode(hdc: Hdc, mode: i32) -> i32;
    fn SetWindowExtEx(hdc: Hdc, x: i32, y: i32, old: *mut c_void) -> i32;
    fn SetViewportExtEx(hdc: Hdc, x: i32, y: i32, old: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: Hdc) -> Hdc;
    fn DeleteDC(hdc: Hdc) -> i32;
    fn CreateCompatibleBitmap(hdc: Hdc, width: i32, height: i32) -> *mut c_void;
    fn SelectObject(hdc: Hdc, object: Hgdiobj) -> Hgdiobj;
    fn DeleteObject(object: Hgdiobj) -> i32;
    fn CreateSolidBrush(color: u32) -> Hbrush;
    fn CreateRoundRectRgn(
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        ellipse_width: i32,
        ellipse_height: i32,
    ) -> Hregion;
    fn FillRgn(hdc: Hdc, region: Hregion, brush: Hbrush) -> i32;
    fn FrameRgn(hdc: Hdc, region: Hregion, brush: Hbrush, width: i32, height: i32) -> i32;
    fn BitBlt(
        dest: Hdc,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        source: Hdc,
        source_x: i32,
        source_y: i32,
        rop: u32,
    ) -> i32;
    fn SetTextColor(hdc: Hdc, color: u32) -> u32;
    fn SetBkMode(hdc: Hdc, mode: i32) -> i32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face: *const u16,
    ) -> *mut c_void;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn DrawTextW(hdc: Hdc, text: *const u16, count: i32, rect: *mut Rect, format: u32) -> i32;
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: Hwnd,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show: i32,
    ) -> isize;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(module: *const u16) -> Hinstance;
    fn LoadLibraryW(file_name: *const u16) -> Hinstance;
    fn GetProcAddress(module: Hinstance, procedure_name: *const u8) -> *mut c_void;
}

#[derive(Clone, Copy)]
struct Hotkey {
    modifiers: u32,
    key: u32,
}

struct SearchRequest {
    epoch: u64,
    query: String,
}
struct SearchResponse {
    epoch: u64,
    hits: Vec<SearchHit>,
    elapsed_us: u128,
}

struct AppState {
    latest_epoch: Arc<AtomicU64>,
    search_tx: mpsc::Sender<SearchRequest>,
    epoch: u64,
    query: String,
    hits: Vec<SearchHit>,
    selected: usize,
    status: String,
    hotkey: Hotkey,
    capture_hotkey: bool,
    edit: isize,
    edit_proc: isize,
    edit_font: isize,
    edit_brush: isize,
    collapsed: bool,
    scroll: usize,
    dpi: u32,
    motion: motion::Motion,
    last_frame: Instant,
    reduced_motion: bool,
    max_rows: usize,
    ready_count: usize,
    searching: bool,
    metadata: Vec<String>,
    meta_tx: mpsc::Sender<(u64, Vec<SearchHit>)>,
    query_started: Instant,
    awaiting_paint: bool,
    suppress_char: bool,
    last_paint_us: u128,
    ime_composing: bool,
    preserve_selection: Option<PathBuf>,
}

static APP: OnceLock<Mutex<AppState>> = OnceLock::new();

fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(-4isize as *mut c_void);
        let _ = windows::Win32::UI::Controls::BufferedPaintInit();
    }
    let instance = unsafe { GetModuleHandleW(null()) };
    let class_name = wide("ZeroFind.SearchIsland");
    let class = WndClassExW {
        size: std::mem::size_of::<WndClassExW>() as u32,
        style: 0x0003 | 0x0008,
        wnd_proc: Some(window_proc),
        class_extra: 0,
        window_extra: 0,
        instance,
        icon: null_mut(),
        cursor: unsafe { LoadCursorW(null_mut(), 32512usize as *const u16) },
        background: null_mut(),
        menu_name: null(),
        class_name: class_name.as_ptr(),
        icon_small: null_mut(),
    };
    if unsafe { RegisterClassExW(&class) } == 0 {
        return;
    }

    let width = 720;
    let height = 132;
    let x = (unsafe { GetSystemMetrics(0) } - width) / 2;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST
                | if renderer::enabled() {
                    0
                } else {
                    WS_EX_LAYERED
                }
                | if std::env::var_os("ZEROFIND_DIAGNOSTIC_WINDOW").is_some() {
                    0x0004_0000
                } else {
                    WS_EX_TOOLWINDOW
                },
            class_name.as_ptr(),
            wide("ZeroFind").as_ptr(),
            WS_POPUP,
            x,
            110,
            width,
            height,
            null_mut(),
            null_mut(),
            instance,
            null_mut(),
        )
    };
    if hwnd.is_null() {
        return;
    }

    configure_visuals(hwnd, width, height);
    let hotkey = load_hotkey();
    let (meta_tx, meta_rx) = mpsc::channel();
    let edit = unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide("").as_ptr(),
            0x4000_0000 | 0x1000_0000 | 0x0080,
            74,
            28,
            590,
            32,
            hwnd,
            10usize as *mut c_void,
            instance,
            null_mut(),
        )
    };
    let edit_proc = unsafe { SetWindowLongPtrW(edit, -4, edit_proc as *const () as isize) };
    unsafe {
        SendMessageW(
            edit,
            0x1501,
            1,
            wide("파일이나 폴더 이름 검색").as_ptr() as isize,
        );
        SendMessageW(edit, 0x00c5, 4096, 0);
    }
    let mut animations: i32 = 1;
    unsafe {
        SystemParametersInfoW(0x1042, 0, (&mut animations as *mut i32).cast(), 0);
    }
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let entries = Arc::new(RwLock::new(Vec::new()));
    let latest_epoch = Arc::new(AtomicU64::new(0));
    let (search_tx, search_rx) = mpsc::channel();
    APP.set(Mutex::new(AppState {
        latest_epoch: latest_epoch.clone(),
        search_tx,
        epoch: 0,
        query: String::new(),
        hits: Vec::new(),
        selected: 0,
        status: "파일 목록을 준비하는 중…".to_string(),
        hotkey,
        capture_hotkey: false,
        edit: edit as isize,
        edit_proc,
        edit_font: 0,
        edit_brush: unsafe { CreateSolidBrush(rgb(227, 240, 250)) } as isize,
        collapsed: false,
        scroll: 0,
        dpi,
        motion: motion::Motion::new(720., 132.),
        last_frame: Instant::now(),
        reduced_motion: animations == 0,
        max_rows: 8,
        ready_count: 0,
        searching: false,
        metadata: Vec::new(),
        meta_tx,
        query_started: Instant::now(),
        awaiting_paint: false,
        suppress_char: false,
        last_paint_us: 0,
        ime_composing: false,
        preserve_selection: None,
    }))
    .ok();

    let registered =
        unsafe { RegisterHotKey(hwnd, 1, hotkey.modifiers | MOD_NOREPEAT, hotkey.key) } != 0;
    if !registered {
        with_state(|state| state.status = "단축키가 사용 중입니다 · F2로 변경".to_string());
    }
    start_search_worker(
        hwnd as isize,
        entries.clone(),
        latest_epoch.clone(),
        search_rx,
    );
    start_metadata_worker(hwnd as isize, latest_epoch, meta_rx);
    start_indexer(hwnd as isize, entries);
    update_edit_font();
    place_on_monitor(hwnd);

    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(edit);
    }
    if std::env::args().any(|arg| arg == "--certify") {
        certify::start(hwnd);
    }

    let mut message: Msg = unsafe { std::mem::zeroed() };
    while unsafe { GetMessageW(&mut message, null_mut(), 0, 0) } > 0 {
        unsafe {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

unsafe extern "system" fn window_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: Wparam,
    lparam: Lparam,
) -> Lresult {
    match message {
        WM_ERASEBKGND => return 1,
        WM_PAINT => {
            paint(hwnd);
            return 0;
        }
        WM_TIMER => {
            if wparam == 2 {
                certify::tick(hwnd);
                return 0;
            }
            animate(hwnd);
            return 0;
        }
        WM_HOTKEY => {
            if with_state_value(|s| s.collapsed).unwrap_or(false)
                || unsafe { IsWindowVisible(hwnd) } == 0
            {
                show_island(hwnd);
            } else {
                collapse_island(hwnd);
            }
            return 0;
        }
        WM_ACTIVATEAPP if wparam == 0 => {
            collapse_island(hwnd);
            return 0;
        }
        0x0232 | 0x007e | 0x001a => {
            // End of a drag, display topology, or work-area/accessibility change.
            update_monitor_limits(hwnd);
            let mut animations = 1i32;
            unsafe {
                SystemParametersInfoW(0x1042, 0, (&mut animations as *mut i32).cast(), 0);
            }
            with_state(|s| s.reduced_motion = animations == 0);
            resize_for_content(hwnd);
            return 0;
        }
        WM_DPICHANGED => {
            with_state(|s| s.dpi = (wparam as u32 & 0xffff).max(96));
            if lparam != 0 {
                let r = unsafe { *(lparam as *const Rect) };
                unsafe {
                    SetWindowPos(
                        hwnd,
                        null_mut(),
                        r.left,
                        r.top,
                        r.right - r.left,
                        r.bottom - r.top,
                        SWP_NOACTIVATE | 4,
                    );
                }
            }
            update_edit_font();
            update_monitor_limits(hwnd);
            resize_for_content(hwnd);
            return 0;
        }
        WM_COMMAND if wparam & 0xffff == 10 && wparam >> 16 == 0x0300 => {
            let edit = lparam as Hwnd;
            let len = unsafe { GetWindowTextLengthW(edit) }.max(0) as usize;
            let mut text = vec![0u16; len + 1];
            let read = unsafe { GetWindowTextW(edit, text.as_mut_ptr(), text.len() as i32) }.max(0)
                as usize;
            with_state(|s| s.query = String::from_utf16_lossy(&text[..read]));
            queue_search(hwnd);
            resize_for_content(hwnd);
            return 0;
        }
        WM_CTLCOLOREDIT => {
            unsafe {
                SetTextColor(wparam as Hdc, rgb(31, 52, 73));
                SetBkColor(wparam as Hdc, rgb(227, 240, 250));
            }
            return with_state_value(|s| s.edit_brush).unwrap_or(0);
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam >> 16) as u16 as i16;
            with_state(|s| {
                if delta > 0 {
                    s.scroll = s.scroll.saturating_sub(3);
                } else {
                    s.scroll = (s.scroll + 3).min(s.hits.len().saturating_sub(s.max_rows));
                }
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_APP_METADATA => {
            if lparam != 0 {
                let response = unsafe { Box::from_raw(lparam as *mut (u64, Vec<String>)) };
                with_state(|s| {
                    if s.epoch == response.0 {
                        s.metadata = response.1;
                    }
                });
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            return 0;
        }
        WM_APP_INDEX_ERROR => {
            with_state(|s| s.status = "일부 위치의 변경 감지 중단 · F5로 다시 읽기".into());
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }

        WM_KEYDOWN => {
            if handle_keydown(hwnd, wparam) {
                return 0;
            }
        }
        WM_CHAR => {
            if handle_char(hwnd, wparam as u32) {
                return 0;
            }
        }
        WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
            if with_state_value(|s| s.collapsed).unwrap_or(false) {
                show_island(hwnd);
                return 0;
            }
            let y = ((lparam >> 16) as u16 as i16 as i32) * 96
                / with_state_value(|s| s.dpi).unwrap_or(96) as i32;
            let valid = with_state_value(|s| {
                y >= 94 && y < 94 + (s.hits.len().min(s.max_rows) as i32 * 56)
            })
            .unwrap_or(false);
            if valid {
                with_state(|s| s.selected = s.scroll + ((y - 94) / 56) as usize);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                if message == WM_LBUTTONDBLCLK {
                    open_selected(hwnd, false);
                }
            } else {
                if let Some(edit) = with_state_value(|s| s.edit) {
                    unsafe {
                        SetFocus(edit as Hwnd);
                    }
                }
            }
            return 0;
        }
        WM_NCHITTEST => {
            let x = lparam as u16 as i16 as i32;
            let y = (lparam >> 16) as u16 as i16 as i32;
            let mut rect = Rect::default();
            unsafe {
                GetWindowRect(hwnd, &mut rect);
            }
            let dpi = with_state_value(|s| s.dpi).unwrap_or(96) as i32;
            if x - rect.left > 64 * dpi / 96 && y - rect.top < 18 * dpi / 96 {
                return HTCAPTION;
            }
            return 1;
        }

        WM_APP_INDEX_PROGRESS => {
            with_state(|state| {
                state.status = format!("파일 목록 준비 중 · {}개", format_count(wparam))
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_APP_INDEX_READY => {
            let selection =
                with_state_value(|s| s.hits.get(s.selected).map(|h| h.entry.path.clone()))
                    .flatten();
            with_state(|state| {
                state.ready_count = wparam;
                state.status = format!(
                    "{}개 항목 준비됨 · {}",
                    format_count(wparam),
                    hotkey_label(state.hotkey)
                )
            });
            queue_search(hwnd);
            with_state(|s| s.preserve_selection = selection);
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_APP_SEARCH_READY => {
            if lparam != 0 {
                let response = unsafe { Box::from_raw(lparam as *mut SearchResponse) };
                let accepted = with_state_value(|s| response.epoch == s.epoch).unwrap_or(false);
                if accepted {
                    with_state(|s| {
                        s.hits = response.hits;
                        if let Some(path) = s.preserve_selection.take() {
                            s.selected = s
                                .hits
                                .iter()
                                .position(|h| h.entry.path == path)
                                .unwrap_or(0);
                            s.scroll = s.selected.saturating_sub(s.max_rows.saturating_sub(1));
                        }
                        s.searching = false;
                        s.metadata.clear();
                        s.selected = s.selected.min(s.hits.len().saturating_sub(1));
                        s.scroll = s.scroll.min(s.hits.len().saturating_sub(s.max_rows));
                        s.status = if s.hits.is_empty() {
                            "일치하는 파일이 없습니다".into()
                        } else {
                            format!(
                                "상위 {}개 · 검색 {:.2} ms",
                                s.hits.len(),
                                response.elapsed_us as f64 / 1000.
                            )
                        };
                        s.awaiting_paint = true;
                        let _ = s.meta_tx.send((s.epoch, s.hits.clone()));
                    });
                    resize_for_content(hwnd);
                    unsafe {
                        InvalidateRect(hwnd, null(), 0);
                    }
                }
            }
            return 0;
        }

        WM_DESTROY => {
            renderer::shutdown();
            unsafe {
                let _ = windows::Win32::UI::Controls::BufferedPaintUnInit();
                UnregisterHotKey(hwnd, 1);
                KillTimer(hwnd, 1);
                PostQuitMessage(0);
            }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

unsafe extern "system" fn edit_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if message == WM_PAINT
        && let Some(old) = with_state_value(|s| s.edit_proc)
    {
        paint_edit_on_glass(hwnd, old);
        return 0;
    }
    if message == 0x010d {
        with_state(|s| s.ime_composing = true);
    }
    if message == 0x010e {
        with_state(|s| s.ime_composing = false);
    }
    let composing = with_state_value(|s| s.ime_composing).unwrap_or(false);
    if !composing
        && (message == WM_KEYDOWN
            || (message == 0x0104 && with_state_value(|s| s.capture_hotkey).unwrap_or(false)))
    {
        with_state(|s| s.suppress_char = false);
        let parent = unsafe { GetParent(hwnd) };
        if handle_keydown(parent, wparam) {
            with_state(|s| s.suppress_char = true);
            return 0;
        }
        if wparam == 0x41 && unsafe { GetKeyState(VK_CONTROL) } < 0 {
            unsafe {
                SendMessageW(hwnd, 0x00b1, 0, -1);
            }
            return 0;
        }
    }
    if matches!(message, WM_CHAR | 0x0106)
        && (matches!(wparam, VK_RETURN | VK_ESCAPE)
            || with_state_value(|s| s.capture_hotkey || s.suppress_char).unwrap_or(false))
    {
        return 0;
    }
    let old = with_state_value(|s| s.edit_proc).unwrap_or(0);
    if old != 0 {
        unsafe { CallWindowProcW(old, hwnd, message, wparam, lparam) }
    } else {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }
}

fn paint_edit_on_glass(hwnd: Hwnd, old: isize) {
    use windows::Win32::{Foundation::RECT, Graphics::Gdi::HDC, UI::Controls::*};
    let mut ps: PaintStruct = unsafe { std::mem::zeroed() };
    let dc = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut rect = Rect::default();
    let mut buffered_dc = HDC::default();
    unsafe {
        GetClientRect(hwnd, &mut rect);
        let r = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let buffer = BeginBufferedPaint(HDC(dc), &r, BPBF_TOPDOWNDIB, None, &mut buffered_dc);
        if buffer != 0 {
            CallWindowProcW(old, hwnd, 0x0318, buffered_dc.0 as usize, 0x0c);
            if GetWindowTextLengthW(hwnd) == 0
                && !with_state_value(|s| s.ime_composing).unwrap_or(false)
            {
                let font = SendMessageW(hwnd, 0x0031, 0, 0);
                let old_font = SelectObject(buffered_dc.0, font as Hgdiobj);
                SetBkMode(buffered_dc.0, TRANSPARENT);
                SetTextColor(buffered_dc.0, rgb(113, 140, 161));
                let placeholder = wide("파일이나 폴더 이름 검색");
                DrawTextW(
                    buffered_dc.0,
                    placeholder.as_ptr(),
                    -1,
                    &mut rect,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
                SelectObject(buffered_dc.0, old_font);
            }
            let _ = BufferedPaintSetAlpha(buffer, None, 255);
            let _ = EndBufferedPaint(buffer, true);
        } else {
            CallWindowProcW(old, hwnd, 0x0318, dc as usize, 0x0c);
        }
        EndPaint(hwnd, &ps);
    }
}
#[link(name = "user32")]
unsafe extern "system" {
    fn GetParent(hwnd: Hwnd) -> Hwnd;
}

fn handle_keydown(hwnd: Hwnd, key: usize) -> bool {
    let capturing = with_state_value(|state| state.capture_hotkey).unwrap_or(false);
    if capturing {
        if key == VK_ESCAPE {
            with_state(|state| {
                state.capture_hotkey = false;
                state.status = "단축키 변경 취소됨".to_string();
            });
        } else if !matches!(
            key as i32,
            VK_CONTROL | VK_MENU | VK_SHIFT | VK_LWIN | VK_RWIN
        ) {
            capture_hotkey(hwnd, key as u32);
        }
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
        return true;
    }

    match key {
        VK_ESCAPE => {
            collapse_island(hwnd);
            true
        }
        0x74 => {
            live_index::request_refresh();
            with_state(|s| s.status = "파일 목록 새로 고치는 중…".into());
            true
        }
        VK_F2 => {
            with_state(|state| {
                state.capture_hotkey = true;
                state.status = "새 단축키를 누르세요 · Esc 취소".to_string();
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            true
        }
        VK_UP => {
            with_state(|state| {
                state.selected = state.selected.saturating_sub(1);
                state.scroll = state.scroll.min(state.selected);
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            true
        }
        VK_DOWN => {
            with_state(|state| {
                if state.selected + 1 < state.hits.len() {
                    state.selected += 1;
                    if state.selected >= state.scroll + state.max_rows {
                        state.scroll = state.selected + 1 - state.max_rows;
                    }
                }
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            true
        }
        VK_RETURN => {
            let ctrl = unsafe { GetKeyState(VK_CONTROL) } < 0;
            open_selected(hwnd, ctrl);
            true
        }
        _ => false,
    }
}

fn handle_char(_hwnd: Hwnd, _character: u32) -> bool {
    false
}

fn queue_search(hwnd: Hwnd) {
    with_state(|state| {
        state.epoch += 1;
        state.latest_epoch.store(state.epoch, Ordering::Relaxed);
        state.selected = 0;
        state.preserve_selection = None;
        state.scroll = 0;
        state.metadata.clear();
        state.query_started = Instant::now();
        state.searching = !state.query.trim().is_empty();
        if state.query.trim().is_empty() {
            state.hits.clear();
            state.status = format!(
                "{}개 항목 · {}",
                format_count(state.ready_count),
                hotkey_label(state.hotkey)
            );
        } else {
            state.status = "검색 중…".to_string();
            let _ = state.search_tx.send(SearchRequest {
                epoch: state.epoch,
                query: state.query.clone(),
            });
        }
    });
    unsafe {
        InvalidateRect(hwnd, null(), 0);
    }
}

fn start_search_worker(
    hwnd: isize,
    entries: Arc<RwLock<Vec<FileEntry>>>,
    latest: Arc<AtomicU64>,
    receiver: mpsc::Receiver<SearchRequest>,
) {
    thread::spawn(move || {
        while let Ok(mut request) = receiver.recv() {
            while let Ok(newer) = receiver.try_recv() {
                request = newer;
            }
            let started = Instant::now();
            let hits = entries
                .read()
                .map(|items| {
                    search_cancellable(&items, &request.query, 100, request.epoch, &latest)
                })
                .unwrap_or_default();
            if latest.load(Ordering::Relaxed) != request.epoch {
                continue;
            }
            let response = Box::new(SearchResponse {
                epoch: request.epoch,
                hits,
                elapsed_us: started.elapsed().as_micros(),
            });
            let raw = Box::into_raw(response);
            if unsafe { PostMessageW(hwnd as Hwnd, WM_APP_SEARCH_READY, 0, raw as isize) } == 0 {
                unsafe {
                    drop(Box::from_raw(raw));
                }
            }
        }
    });
}

fn start_indexer(hwnd: isize, entries: Arc<RwLock<Vec<FileEntry>>>) {
    live_index::start(
        roots_from_environment(),
        entries,
        move |event, count| unsafe {
            PostMessageW(
                hwnd as Hwnd,
                match event {
                    live_index::Event::Ready => WM_APP_INDEX_READY,
                    live_index::Event::Progress => WM_APP_INDEX_PROGRESS,
                    live_index::Event::Error => WM_APP_INDEX_ERROR,
                },
                count,
                0,
            );
        },
    );
}

fn start_metadata_worker(
    hwnd: isize,
    latest: Arc<AtomicU64>,
    receiver: mpsc::Receiver<(u64, Vec<SearchHit>)>,
) {
    thread::spawn(move || {
        while let Ok(mut request) = receiver.recv() {
            while let Ok(newer) = receiver.try_recv() {
                request = newer;
            }
            let mut details = Vec::new();
            for hit in request.1 {
                if latest.load(Ordering::Relaxed) != request.0 {
                    break;
                }
                let value = std::fs::metadata(&hit.entry.path)
                    .ok()
                    .map(|m| {
                        let size = if m.is_dir() {
                            "폴더".into()
                        } else if m.len() >= 1_048_576 {
                            format!("{:.1} MB", m.len() as f64 / 1_048_576.)
                        } else if m.len() >= 1024 {
                            format!("{:.0} KB", m.len() as f64 / 1024.)
                        } else {
                            format!("{} B", m.len())
                        };
                        let date = m.modified().ok().map(format_modified).unwrap_or_default();
                        format!("{date}  ·  {size}")
                    })
                    .unwrap_or_else(|| "정보 없음".into());
                details.push(value);
            }
            if latest.load(Ordering::Relaxed) != request.0 {
                continue;
            }
            let raw = Box::into_raw(Box::new((request.0, details)));
            if unsafe { PostMessageW(hwnd as Hwnd, WM_APP_METADATA, 0, raw as isize) } == 0 {
                unsafe {
                    drop(Box::from_raw(raw));
                }
            }
        }
    });
}

#[repr(C)]
#[derive(Default)]
struct SystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn FileTimeToSystemTime(file: *const u64, system: *mut SystemTime) -> i32;
    fn SystemTimeToTzSpecificLocalTime(
        zone: *const c_void,
        utc: *const SystemTime,
        local: *mut SystemTime,
    ) -> i32;
    fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
}
fn format_modified(time: std::time::SystemTime) -> String {
    let Ok(duration) = time.duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let ticks = (duration.as_secs() + 11_644_473_600) * 10_000_000;
    let mut utc = SystemTime::default();
    let mut local = SystemTime::default();
    unsafe {
        if FileTimeToSystemTime(&ticks, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(null(), &utc, &mut local) == 0
        {
            return String::new();
        }
    }
    format!("{:04}.{:02}.{:02}", local.year, local.month, local.day)
}

fn capture_hotkey(hwnd: Hwnd, key: u32) {
    let mut modifiers = 0u32;
    unsafe {
        if GetKeyState(VK_CONTROL) < 0 {
            modifiers |= MOD_CONTROL;
        }
        if GetKeyState(VK_MENU) < 0 {
            modifiers |= MOD_ALT;
        }
        if GetKeyState(VK_SHIFT) < 0 {
            modifiers |= MOD_SHIFT;
        }
        if GetKeyState(VK_LWIN) < 0 || GetKeyState(VK_RWIN) < 0 {
            modifiers |= MOD_WIN;
        }
    }
    if modifiers == 0 {
        with_state(|state| state.status = "Ctrl, Alt, Shift 중 하나와 함께 누르세요".to_string());
        return;
    }
    let old = with_state_value(|state| state.hotkey).unwrap_or(Hotkey {
        modifiers: MOD_CONTROL | MOD_SHIFT,
        key: 0x20,
    });
    unsafe {
        UnregisterHotKey(hwnd, 1);
    }
    let candidate = Hotkey { modifiers, key };
    if unsafe { RegisterHotKey(hwnd, 1, modifiers | MOD_NOREPEAT, key) } != 0 {
        let saved = save_hotkey(candidate);
        with_state(|state| {
            state.hotkey = candidate;
            state.capture_hotkey = false;
            state.status = if saved {
                format!("단축키 저장됨 · {}", hotkey_label(candidate))
            } else {
                "단축키 적용됨 · 설정 저장 실패".into()
            };
        });
    } else {
        unsafe {
            RegisterHotKey(hwnd, 1, old.modifiers | MOD_NOREPEAT, old.key);
        }
        with_state(|state| {
            state.capture_hotkey = false;
            state.status = "사용 중인 단축키입니다 · F2로 다시 변경".to_string();
        });
    }
}

fn load_hotkey() -> Hotkey {
    let fallback = Hotkey {
        modifiers: MOD_CONTROL | MOD_SHIFT,
        key: 0x20,
    };
    let Some(path) = settings_path() else {
        return fallback;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return fallback;
    };
    let mut result = fallback;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("modifiers=") {
            result.modifiers = value.parse().unwrap_or(result.modifiers);
        }
        if let Some(value) = line.strip_prefix("key=") {
            result.key = value.parse().unwrap_or(result.key);
        }
    }
    if result.modifiers == 0 || result.modifiers & !15 != 0 || !(1..=254).contains(&result.key) {
        return fallback;
    }
    if !text.lines().any(|line| line == "version=2")
        && result.key == 0x20
        && result.modifiers == (MOD_CONTROL | MOD_ALT)
    {
        return fallback;
    }
    result
}

fn save_hotkey(hotkey: Hotkey) -> bool {
    use std::io::Write;
    let Some(path) = settings_path() else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&temp)?;
        write!(
            file,
            "version=2\nmodifiers={}\nkey={}\n",
            hotkey.modifiers, hotkey.key
        )?;
        file.sync_all()
    })();
    if written.is_err() {
        return false;
    }
    unsafe {
        MoveFileExW(
            wide(temp.as_os_str()).as_ptr(),
            wide(path.as_os_str()).as_ptr(),
            1 | 8,
        ) != 0
    }
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("ZeroFind").join("settings.ini"))
}

fn display_path(path: &std::path::Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_string()
}
fn shell_path(path: &std::path::Path) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if units.starts_with(&[92, 92, 63, 92]) && units.len() < 264 {
        PathBuf::from(std::ffi::OsString::from_wide(&units[4..]))
    } else {
        path.to_path_buf()
    }
}

fn open_selected(hwnd: Hwnd, containing: bool) {
    let target = with_state_value(|state| {
        if state.searching {
            return None;
        }
        state
            .hits
            .get(state.selected)
            .map(|hit| hit.entry.path.clone())
    })
    .flatten();
    let Some(path) = target else { return };
    let destination = if containing {
        path.parent().unwrap_or(&path).to_path_buf()
    } else {
        path
    };
    let operation = wide("open");
    let shell_destination = shell_path(&destination);
    let file = wide(shell_destination.as_os_str());
    let result = unsafe {
        ShellExecuteW(
            hwnd,
            operation.as_ptr(),
            file.as_ptr(),
            null(),
            null(),
            SW_SHOW,
        )
    };
    if result > 32 {
        unsafe {
            ShowWindow(hwnd, SW_HIDE);
        }
    } else {
        with_state(|state| {
            state.status = "파일을 열지 못했습니다 · 이동 또는 삭제됐을 수 있습니다".to_string()
        });
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

fn show_island(hwnd: Hwnd) {
    let hidden = unsafe { IsWindowVisible(hwnd) } == 0;
    if hidden {
        place_on_monitor(hwnd);
    }
    with_state(|s| s.collapsed = false);
    resize_for_content(hwnd);
    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        if let Some(edit) = with_state_value(|s| s.edit) {
            ShowWindow(edit as Hwnd, SW_SHOW);
            SetFocus(edit as Hwnd);
        }
        InvalidateRect(hwnd, null(), 0);
    }
}
fn collapse_island(hwnd: Hwnd) {
    with_state(|s| {
        s.collapsed = true;
        s.capture_hotkey = false;
    });
    if let Some(edit) = with_state_value(|s| s.edit) {
        unsafe {
            ShowWindow(edit as Hwnd, SW_HIDE);
        }
    }
    resize_for_content(hwnd);
}
fn configure_visuals(hwnd: Hwnd, width: i32, height: i32) {
    unsafe {
        SetLayeredWindowAttributes(hwnd, 0, 246, LWA_ALPHA);
        apply_dwm_backdrop(hwnd);
        let region = CreateRoundRectRgn(0, 0, width, height, 40, 40);
        if SetWindowRgn(hwnd, region, 1) == 0 {
            DeleteObject(region);
        }
    }
}
fn update_edit_font() {
    let Some((dpi, edit, old)) = with_state_value(|s| (s.dpi, s.edit, s.edit_font)) else {
        return;
    };
    unsafe {
        let font = CreateFontW(
            -(21 * dpi as i32 / 96),
            0,
            0,
            0,
            FW_NORMAL,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            0,
            0,
            CLEARTYPE_QUALITY,
            0,
            wide("Segoe UI").as_ptr(),
        );
        with_state(|s| s.edit_font = font as isize);
        SendMessageW(edit as Hwnd, 0x30, font as usize, 1);
        if old != 0 {
            DeleteObject(old as Hgdiobj);
        }
    }
}

fn work_area(hwnd: Hwnd) -> Rect {
    let monitor = unsafe { MonitorFromWindow(hwnd, 2) };
    let mut info = MonitorInfo {
        size: std::mem::size_of::<MonitorInfo>() as u32,
        monitor: Rect::default(),
        work: Rect::default(),
        flags: 0,
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } != 0 {
        info.work
    } else {
        Rect {
            left: 0,
            top: 0,
            right: unsafe { GetSystemMetrics(0) },
            bottom: unsafe { GetSystemMetrics(1) },
        }
    }
}
fn update_monitor_limits(hwnd: Hwnd) {
    let work = work_area(hwnd);
    with_state(|s| {
        let height = (work.bottom - work.top) * 96 / s.dpi as i32;
        s.max_rows = ((height - 220) / 56).clamp(1, 8) as usize;
    });
}
fn place_on_monitor(hwnd: Hwnd) {
    let work = work_area(unsafe { GetForegroundWindow() });
    let scale = with_state_value(|s| s.dpi as f64 / 96.).unwrap_or(1.);
    let width = (720. * scale) as i32;
    let x = work.left + ((work.right - work.left - width) / 2).max(0);
    let y = work.top + ((work.bottom - work.top) / 7).min(120);
    unsafe {
        SetWindowPos(
            hwnd,
            null_mut(),
            x,
            y,
            width,
            (132. * scale) as i32,
            SWP_NOACTIVATE | 4,
        );
    }
    update_monitor_limits(hwnd);
    resize_for_content(hwnd);
}

unsafe fn apply_dwm_backdrop(hwnd: Hwnd) {
    type SetAttribute = unsafe extern "system" fn(Hwnd, u32, *const c_void, u32) -> i32;
    type ExtendFrame = unsafe extern "system" fn(Hwnd, *const Margins) -> i32;
    let module = unsafe { LoadLibraryW(wide("dwmapi.dll").as_ptr()) };
    if module.is_null() {
        return;
    }
    let set_ptr = unsafe { GetProcAddress(module, c"DwmSetWindowAttribute".as_ptr().cast()) };
    let extend_ptr =
        unsafe { GetProcAddress(module, c"DwmExtendFrameIntoClientArea".as_ptr().cast()) };
    if !set_ptr.is_null() {
        let set: SetAttribute = unsafe { std::mem::transmute(set_ptr) };
        let corners: u32 = 2;
        let backdrop: u32 = 3;
        let dark: i32 = 0;
        unsafe {
            set(hwnd, 33, (&corners as *const u32).cast(), 4);
            set(hwnd, 38, (&backdrop as *const u32).cast(), 4);
            set(hwnd, 20, (&dark as *const i32).cast(), 4);
        }
    }
    if !extend_ptr.is_null() {
        let extend: ExtendFrame = unsafe { std::mem::transmute(extend_ptr) };
        let margins = Margins {
            // GDI produces no valid per-pixel alpha. Extending glass across
            // the client makes those pixels disappear under DWM composition.
            left: if renderer::enabled() { -1 } else { 0 },
            right: if renderer::enabled() { -1 } else { 0 },
            top: if renderer::enabled() { -1 } else { 0 },
            bottom: if renderer::enabled() { -1 } else { 0 },
        };
        unsafe {
            extend(hwnd, &margins);
        }
    }
}

fn resize_for_content(hwnd: Hwnd) {
    with_state(|s| {
        s.motion.target_width = if s.collapsed { 64. } else { 720. };
        s.motion.target_height = if s.collapsed {
            64.
        } else if s.hits.is_empty() {
            132.
        } else {
            126. + s.hits.len().min(s.max_rows) as f64 * 56.
        };
        // The first actionable row gets space immediately; only the remaining
        // structure animates. Never hold ready data behind a geometry transition.
        if !s.collapsed && !s.hits.is_empty() {
            s.motion.height = s.motion.height.max(146.);
        }
        s.last_frame = Instant::now();
    });
    // Paint data immediately. Geometry follows it, without delaying the result message.
    unsafe {
        SetTimer(hwnd, 1, 16, null_mut());
    }
    animate(hwnd);
}
fn animate(hwnd: Hwnd) {
    let mut rect = Rect::default();
    unsafe {
        GetWindowRect(hwnd, &mut rect);
    }
    let work = work_area(hwnd);
    let values = with_state_value(|s| {
        (
            s.motion,
            s.last_frame,
            s.reduced_motion,
            s.dpi,
            s.edit,
            s.collapsed,
        )
    });
    let Some((mut motion, last, reduced, dpi, edit, collapsed)) = values else {
        return;
    };
    let done = motion.step(last.elapsed().as_secs_f64().max(0.001), reduced);
    let scale = dpi as f64 / 96.;
    let width = ((motion.width * scale).round() as i32).min(work.right - work.left);
    let height = ((motion.height * scale).round() as i32).min(work.bottom - work.top);
    let x = rect.left.clamp(work.left, work.right - width);
    let y = rect.top.clamp(work.top, work.bottom - height);
    with_state(|s| {
        s.motion = motion;
        s.last_frame = Instant::now();
    });
    unsafe {
        SetWindowPos(hwnd, null_mut(), x, y, width, height, SWP_NOACTIVATE | 4);
        let region = CreateRoundRectRgn(
            0,
            0,
            width,
            height,
            (40. * scale) as i32,
            (40. * scale) as i32,
        );
        if SetWindowRgn(hwnd, region, 0) == 0 {
            DeleteObject(region);
        }
        if !collapsed {
            SetWindowPos(
                edit as Hwnd,
                null_mut(),
                (74. * scale) as i32,
                (31. * scale) as i32,
                (width - (104. * scale) as i32).max(1),
                (30. * scale) as i32,
                SWP_NOACTIVATE | 4,
            );
        }
        InvalidateRect(hwnd, null(), 0);
        if done {
            KillTimer(hwnd, 1);
        }
    }
}

fn paint(hwnd: Hwnd) {
    let mut first_row_painted = false;
    let mut ps: PaintStruct = unsafe { std::mem::zeroed() };
    let target = unsafe { BeginPaint(hwnd, &mut ps) };
    let mut client = Rect::default();
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let memory = unsafe { CreateCompatibleDC(target) };
    let bitmap =
        unsafe { CreateCompatibleBitmap(target, client.right.max(1), client.bottom.max(1)) };
    if memory.is_null() || bitmap.is_null() {
        unsafe {
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            if !memory.is_null() {
                DeleteDC(memory);
            }
            EndPaint(hwnd, &ps);
        }
        return;
    }
    let old = unsafe { SelectObject(memory, bitmap) };
    let dpi = with_state_value(|s| s.dpi).unwrap_or(96) as i32;
    let attempted_gpu = renderer::enabled();
    let gpu = renderer::begin(hwnd, client.right, client.bottom, dpi as u32);
    if attempted_gpu && !gpu {
        unsafe {
            apply_dwm_backdrop(hwnd);
        }
    }
    let width = client.right * 96 / dpi;
    let height = client.bottom * 96 / dpi;
    unsafe {
        SetMapMode(memory, 8);
        SetWindowExtEx(memory, 96, 96, null_mut());
        SetViewportExtEx(memory, dpi, dpi, null_mut());
    }
    fill_round(
        memory,
        Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        40,
        rgb(218, 234, 246),
        Some(rgb(192, 214, 231)),
    );
    let snapshot = with_state_value(|s| {
        (
            s.collapsed,
            s.hits.clone(),
            s.selected,
            s.scroll,
            s.max_rows,
            s.status.clone(),
            s.metadata.clone(),
            s.query.is_empty(),
        )
    });
    if let Some((collapsed, hits, selected, scroll, rows, status, metadata, empty)) = snapshot {
        if collapsed {
            draw_search_icon(memory, 32, 32);
        } else {
            fill_round(
                memory,
                Rect {
                    left: 16,
                    top: 17,
                    right: (width - 16).max(17),
                    bottom: 79,
                },
                28,
                rgb(227, 240, 250),
                None,
            );
            draw_search_icon(memory, 44, 48);
            if width > 280 {
                if hits.is_empty() {
                    draw_text(
                        memory,
                        if empty && status.contains("개 항목") {
                            "이름으로 찾고, Enter로 열기"
                        } else {
                            &status
                        },
                        Rect {
                            left: 28,
                            top: 88,
                            right: width - 28,
                            bottom: 120,
                        },
                        13,
                        FW_NORMAL,
                        rgb(79, 103, 122),
                        DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                    );
                } else {
                    for (row, hit) in hits.iter().skip(scroll).take(rows).enumerate() {
                        let index = scroll + row;
                        let top = 94 + row as i32 * 56;
                        if top + 50 > height {
                            break;
                        }
                        first_row_painted = true;
                        if index == selected {
                            fill_round(
                                memory,
                                Rect {
                                    left: 14,
                                    top,
                                    right: width - 14,
                                    bottom: top + 52,
                                },
                                16,
                                rgb(241, 248, 254),
                                Some(rgb(199, 220, 238)),
                            );
                            fill_round(
                                memory,
                                Rect {
                                    left: 18,
                                    top: top + 16,
                                    right: 21,
                                    bottom: top + 36,
                                },
                                3,
                                rgb(62, 126, 181),
                                None,
                            );
                        }
                        let marker = if hit.entry.kind == EntryKind::Directory {
                            "DIR".to_owned()
                        } else {
                            hit.entry
                                .path
                                .extension()
                                .and_then(|v| v.to_str())
                                .unwrap_or("FILE")
                                .chars()
                                .take(4)
                                .collect::<String>()
                                .to_uppercase()
                        };
                        fill_round(
                            memory,
                            Rect {
                                left: 30,
                                top: top + 9,
                                right: 66,
                                bottom: top + 43,
                            },
                            10,
                            rgb(205, 225, 241),
                            None,
                        );
                        draw_text(
                            memory,
                            &marker,
                            Rect {
                                left: 30,
                                top: top + 9,
                                right: 66,
                                bottom: top + 43,
                            },
                            9,
                            FW_SEMIBOLD,
                            rgb(60, 102, 137),
                            DT_CENTER | DT_SINGLELINE | DT_VCENTER,
                        );
                        draw_text(
                            memory,
                            &hit.entry.name,
                            Rect {
                                left: 80,
                                top: top + 4,
                                right: width - 30,
                                bottom: top + 28,
                            },
                            17,
                            FW_NORMAL,
                            rgb(28, 49, 67),
                            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | 0x800,
                        );
                        draw_text(
                            memory,
                            &display_path(&hit.entry.path),
                            Rect {
                                left: 80,
                                top: top + 29,
                                right: width - 224,
                                bottom: top + 48,
                            },
                            12,
                            FW_NORMAL,
                            rgb(82, 107, 126),
                            DT_LEFT | DT_SINGLELINE | DT_VCENTER | 0x4000 | 0x800,
                        );
                        if let Some(detail) = metadata.get(index) {
                            draw_text(
                                memory,
                                detail,
                                Rect {
                                    left: width - 216,
                                    top: top + 29,
                                    right: width - 30,
                                    bottom: top + 48,
                                },
                                11,
                                FW_NORMAL,
                                rgb(88, 111, 129),
                                2 | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                            );
                        }
                    }
                    let footer = format!(
                        "{}–{} / {}  ·  {}",
                        scroll + 1,
                        (scroll + rows).min(hits.len()),
                        hits.len(),
                        status
                    );
                    if height >= 126 + hits.len().min(rows) as i32 * 56 {
                        draw_text(
                            memory,
                            &footer,
                            Rect {
                                left: 28,
                                top: height - 29,
                                right: width - 28,
                                bottom: height - 7,
                            },
                            11,
                            FW_NORMAL,
                            rgb(92, 116, 134),
                            DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                        );
                    }
                }
            }
        }
    }
    unsafe {
        SetMapMode(memory, 1);
        let presented = renderer::finish();
        if !presented {
            BitBlt(
                target,
                0,
                0,
                client.right,
                client.bottom,
                memory,
                0,
                0,
                SRCCOPY,
            );
            if gpu {
                apply_dwm_backdrop(hwnd);
                InvalidateRect(hwnd, null(), 0);
            }
        }
        SelectObject(memory, old);
        certify::capture_render(memory, bitmap, client.right, client.bottom);
        DeleteObject(bitmap);
        DeleteDC(memory);
        EndPaint(hwnd, &ps);
    }
    with_state(|s| {
        if s.awaiting_paint && !s.collapsed && (s.hits.is_empty() || first_row_painted) {
            s.awaiting_paint = false;
            s.last_paint_us = s.query_started.elapsed().as_micros();
            // This is CPU paint completion, not a compositor/present timestamp.
            if let Some(path) = std::env::var_os("ZEROFIND_TIMING_LOG") {
                use std::io::Write;
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                {
                    let _ = writeln!(
                        file,
                        "query_to_paint_us={},hits={}",
                        s.query_started.elapsed().as_micros(),
                        s.hits.len()
                    );
                }
            }
        }
    });
}

fn fill_round(hdc: Hdc, rect: Rect, radius: i32, color: u32, border: Option<u32>) {
    if renderer::round(rect, radius, color, border) {
        return;
    }
    unsafe {
        let region = CreateRoundRectRgn(
            rect.left,
            rect.top,
            rect.right + 1,
            rect.bottom + 1,
            radius,
            radius,
        );
        let brush = CreateSolidBrush(color);
        FillRgn(hdc, region, brush);
        DeleteObject(brush);
        if let Some(border_color) = border {
            let border_brush = CreateSolidBrush(border_color);
            FrameRgn(hdc, region, border_brush, 1, 1);
            DeleteObject(border_brush);
        }
        DeleteObject(region);
    }
}

fn draw_search_icon(hdc: Hdc, x: i32, y: i32) {
    draw_text_face(
        hdc,
        "\u{e721}",
        Rect {
            left: x - 13,
            top: y - 14,
            right: x + 15,
            bottom: y + 15,
        },
        23,
        FW_NORMAL,
        rgb(65, 120, 165),
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        "Segoe MDL2 Assets",
    );
}

fn draw_text(hdc: Hdc, text: &str, rect: Rect, size: i32, weight: i32, color: u32, format: u32) {
    draw_text_face(hdc, text, rect, size, weight, color, format, "Segoe UI");
}

#[allow(clippy::too_many_arguments)]
fn draw_text_face(
    hdc: Hdc,
    text: &str,
    mut rect: Rect,
    size: i32,
    weight: i32,
    color: u32,
    format: u32,
    face: &'static str,
) {
    if renderer::text(text, rect, size, weight, color, format, face) {
        return;
    }
    type Fonts = std::collections::HashMap<(i32, i32, &'static str), isize>;
    static FONTS: OnceLock<Mutex<Fonts>> = OnceLock::new();
    let font = *FONTS
        .get_or_init(|| Mutex::new(Fonts::new()))
        .lock()
        .unwrap()
        .entry((size, weight, face))
        .or_insert_with(|| unsafe {
            let face = wide(face);
            CreateFontW(
                -size,
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                0,
                0,
                CLEARTYPE_QUALITY,
                0,
                face.as_ptr(),
            ) as isize
        }) as Hgdiobj;
    let old = unsafe { SelectObject(hdc, font) };
    let value = wide(text);
    unsafe {
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, color);
        DrawTextW(
            hdc,
            value.as_ptr(),
            text.encode_utf16().count() as i32,
            &mut rect,
            format,
        );
        SelectObject(hdc, old);
    }
}

fn hotkey_label(hotkey: Hotkey) -> String {
    let mut parts = Vec::new();
    if hotkey.modifiers & MOD_CONTROL != 0 {
        parts.push("Ctrl".to_string());
    }
    if hotkey.modifiers & MOD_ALT != 0 {
        parts.push("Alt".to_string());
    }
    if hotkey.modifiers & MOD_SHIFT != 0 {
        parts.push("Shift".to_string());
    }
    if hotkey.modifiers & MOD_WIN != 0 {
        parts.push("Win".to_string());
    }
    let key = match hotkey.key {
        0x20 => "Space".to_string(),
        0x70..=0x87 => format!("F{}", hotkey.key - 0x6f),
        0x30..=0x39 | 0x41..=0x5a => char::from_u32(hotkey.key).unwrap_or('?').to_string(),
        other => format!("Key {other}"),
    };
    parts.push(key);
    parts.join("+")
}

fn format_count(count: usize) -> String {
    let digits = count.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn rgb(red: u8, green: u8, blue: u8) -> u32 {
    u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
}

fn wide<S: AsRef<OsStr>>(value: S) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn with_state(action: impl FnOnce(&mut AppState)) {
    if let Some(app) = APP.get()
        && let Ok(mut state) = app.lock()
    {
        action(&mut state);
    }
}

fn with_state_value<T>(action: impl FnOnce(&AppState) -> T) -> Option<T> {
    APP.get()?.lock().ok().map(|state| action(&state))
}
