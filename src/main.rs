#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::{OsStr, c_void};
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::Instant;
use zerofind::{
    EntryKind, FileEntry, SearchHit, roots_from_environment, scan_visible_roots, search_cancellable,
};

type Hwnd = *mut c_void;
type Hinstance = *mut c_void;
type Hdc = *mut c_void;
type Hgdiobj = *mut c_void;
type Hbrush = *mut c_void;
type Hregion = *mut c_void;
type Wparam = usize;
type Lparam = isize;
type Lresult = isize;

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

const VK_BACK: usize = 0x08;
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
    opacity: u8,
}

static APP: OnceLock<Mutex<AppState>> = OnceLock::new();

fn main() {
    unsafe {
        SetProcessDpiAwarenessContext(-4isize as *mut c_void);
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

    let width = 760;
    let height = 132;
    let x = (unsafe { GetSystemMetrics(0) } - width) / 2;
    let hwnd = unsafe {
        CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
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
        status: "Building your private file map…".to_string(),
        hotkey,
        capture_hotkey: false,
        opacity: 222,
    }))
    .ok();

    let registered =
        unsafe { RegisterHotKey(hwnd, 1, hotkey.modifiers | MOD_NOREPEAT, hotkey.key) } != 0;
    if !registered {
        with_state(|state| {
            state.status = "Global hotkey is in use — press F2 to choose another".to_string()
        });
    }
    start_search_worker(
        hwnd as isize,
        entries.clone(),
        latest_epoch.clone(),
        search_rx,
    );
    start_indexer(hwnd as isize, entries);

    unsafe {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SetTimer(hwnd, 1, 12, null_mut());
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
            let mut done = false;
            with_state(|state| {
                state.opacity = state.opacity.saturating_add(4).min(248);
                unsafe {
                    SetLayeredWindowAttributes(hwnd, 0, state.opacity, LWA_ALPHA);
                }
                done = state.opacity >= 248;
            });
            if done {
                unsafe {
                    KillTimer(hwnd, 1);
                }
            }
            return 0;
        }
        WM_HOTKEY => {
            if unsafe { IsWindowVisible(hwnd) } != 0 {
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
            } else {
                show_island(hwnd);
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
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            if y >= 94 {
                let index = ((y - 94) / 54) as usize;
                with_state(|state| {
                    if index < state.hits.len() {
                        state.selected = index;
                    }
                });
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
                if message == WM_LBUTTONDBLCLK {
                    open_selected(hwnd, false);
                }
            }
            return 0;
        }
        WM_NCHITTEST => {
            let y = ((lparam >> 16) & 0xffff) as i16 as i32;
            let mut rect = Rect::default();
            unsafe {
                GetClientRect(hwnd, &mut rect);
            }
            if y < 125 {
                return HTCAPTION;
            }
        }
        WM_APP_INDEX_PROGRESS => {
            with_state(|state| {
                state.status = format!("Mapping visible files… {} indexed", format_count(wparam))
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_APP_INDEX_READY => {
            with_state(|state| {
                state.status = format!(
                    "{} items ready  ·  {}",
                    format_count(wparam),
                    hotkey_label(state.hotkey)
                )
            });
            queue_search(hwnd);
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            return 0;
        }
        WM_APP_SEARCH_READY => {
            if lparam != 0 {
                let response = unsafe { Box::from_raw(lparam as *mut SearchResponse) };
                with_state(|state| {
                    if response.epoch == state.epoch {
                        state.hits = response.hits;
                        state.selected = state.selected.min(state.hits.len().saturating_sub(1));
                        state.status = format!(
                            "{} results in {:.2} ms",
                            state.hits.len(),
                            response.elapsed_us as f64 / 1000.0
                        );
                    }
                });
                resize_for_content(hwnd);
                unsafe {
                    InvalidateRect(hwnd, null(), 0);
                }
            }
            return 0;
        }
        WM_DESTROY => {
            unsafe {
                UnregisterHotKey(hwnd, 1);
                PostQuitMessage(0);
            }
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

fn handle_keydown(hwnd: Hwnd, key: usize) -> bool {
    let capturing = with_state_value(|state| state.capture_hotkey).unwrap_or(false);
    if capturing {
        if key == VK_ESCAPE {
            with_state(|state| {
                state.capture_hotkey = false;
                state.status = "Hotkey change cancelled".to_string();
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
            unsafe {
                ShowWindow(hwnd, SW_HIDE);
            }
            true
        }
        VK_F2 => {
            with_state(|state| {
                state.capture_hotkey = true;
                state.status = "Press your new shortcut (for example Ctrl+Alt+Space)".to_string();
            });
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            true
        }
        VK_UP => {
            with_state(|state| state.selected = state.selected.saturating_sub(1));
            unsafe {
                InvalidateRect(hwnd, null(), 0);
            }
            true
        }
        VK_DOWN => {
            with_state(|state| {
                if state.selected + 1 < state.hits.len() {
                    state.selected += 1;
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

fn handle_char(hwnd: Hwnd, character: u32) -> bool {
    if character == VK_BACK as u32 {
        with_state(|state| {
            state.query.pop();
        });
    } else if character >= 0x20 && character != 0x7f {
        if let Some(value) = char::from_u32(character) {
            with_state(|state| state.query.push(value));
        }
    } else {
        return false;
    }
    queue_search(hwnd);
    resize_for_content(hwnd);
    unsafe {
        InvalidateRect(hwnd, null(), 0);
    }
    true
}

fn queue_search(hwnd: Hwnd) {
    with_state(|state| {
        state.epoch += 1;
        state.latest_epoch.store(state.epoch, Ordering::Relaxed);
        state.selected = 0;
        if state.query.trim().is_empty() {
            state.hits.clear();
        } else {
            state.status = "Searching in memory…".to_string();
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
        while let Ok(request) = receiver.recv() {
            let started = Instant::now();
            let hits = entries
                .read()
                .map(|items| search_cancellable(&items, &request.query, 20, request.epoch, &latest))
                .unwrap_or_default();
            if latest.load(Ordering::Relaxed) != request.epoch {
                continue;
            }
            let response = Box::new(SearchResponse {
                epoch: request.epoch,
                hits,
                elapsed_us: started.elapsed().as_micros(),
            });
            unsafe {
                PostMessageW(
                    hwnd as Hwnd,
                    WM_APP_SEARCH_READY,
                    0,
                    Box::into_raw(response) as isize,
                );
            }
        }
    });
}

fn start_indexer(hwnd: isize, entries: Arc<RwLock<Vec<FileEntry>>>) {
    thread::spawn(move || {
        let roots = roots_from_environment();
        let indexed = scan_visible_roots(&roots, |count| unsafe {
            PostMessageW(hwnd as Hwnd, WM_APP_INDEX_PROGRESS, count, 0);
        });
        let count = indexed.len();
        if let Ok(mut destination) = entries.write() {
            *destination = indexed;
        }
        unsafe {
            PostMessageW(hwnd as Hwnd, WM_APP_INDEX_READY, count, 0);
        }
    });
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
        with_state(|state| state.status = "Use at least one modifier key".to_string());
        return;
    }
    let old = with_state_value(|state| state.hotkey).unwrap_or(Hotkey {
        modifiers: MOD_CONTROL | MOD_ALT,
        key: 0x20,
    });
    unsafe {
        UnregisterHotKey(hwnd, 1);
    }
    let candidate = Hotkey { modifiers, key };
    if unsafe { RegisterHotKey(hwnd, 1, modifiers | MOD_NOREPEAT, key) } != 0 {
        save_hotkey(candidate);
        with_state(|state| {
            state.hotkey = candidate;
            state.capture_hotkey = false;
            state.status = format!("Hotkey saved: {}", hotkey_label(candidate));
        });
    } else {
        unsafe {
            RegisterHotKey(hwnd, 1, old.modifiers | MOD_NOREPEAT, old.key);
        }
        with_state(|state| {
            state.capture_hotkey = false;
            state.status = "That shortcut is already in use — press F2 to retry".to_string();
        });
    }
}

fn load_hotkey() -> Hotkey {
    let fallback = Hotkey {
        modifiers: MOD_CONTROL | MOD_ALT,
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
    result
}

fn save_hotkey(hotkey: Hotkey) {
    let Some(path) = settings_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(
        path,
        format!("modifiers={}\nkey={}\n", hotkey.modifiers, hotkey.key),
    );
}

fn settings_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("ZeroFind").join("settings.ini"))
}

fn open_selected(hwnd: Hwnd, containing: bool) {
    let target = with_state_value(|state| {
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
    let file = wide(destination.as_os_str());
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
        with_state(|state| state.status = "Windows could not open that item".to_string());
        unsafe {
            InvalidateRect(hwnd, null(), 0);
        }
    }
}

fn show_island(hwnd: Hwnd) {
    with_state(|state| state.opacity = 222);
    unsafe {
        SetLayeredWindowAttributes(hwnd, 0, 222, LWA_ALPHA);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetFocus(hwnd);
        SetTimer(hwnd, 1, 12, null_mut());
        InvalidateRect(hwnd, null(), 0);
    }
}

fn configure_visuals(hwnd: Hwnd, width: i32, height: i32) {
    unsafe {
        SetLayeredWindowAttributes(hwnd, 0, 222, LWA_ALPHA);
        apply_dwm_backdrop(hwnd);
        SetWindowRgn(
            hwnd,
            CreateRoundRectRgn(0, 0, width + 1, height + 1, 34, 34),
            1,
        );
    }
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
        let dark: i32 = 1;
        unsafe {
            set(hwnd, 33, (&corners as *const u32).cast(), 4);
            set(hwnd, 38, (&backdrop as *const u32).cast(), 4);
            set(hwnd, 20, (&dark as *const i32).cast(), 4);
        }
    }
    if !extend_ptr.is_null() {
        let extend: ExtendFrame = unsafe { std::mem::transmute(extend_ptr) };
        let margins = Margins {
            left: -1,
            right: -1,
            top: -1,
            bottom: -1,
        };
        unsafe {
            extend(hwnd, &margins);
        }
    }
}

fn resize_for_content(hwnd: Hwnd) {
    let height = with_state_value(|state| {
        if state.query.trim().is_empty() {
            132
        } else if state.hits.is_empty() {
            150
        } else {
            118 + (state.hits.len().min(9) as i32 * 54) + 34
        }
    })
    .unwrap_or(132);
    let width = 760;
    let x = (unsafe { GetSystemMetrics(0) } - width) / 2;
    unsafe {
        SetWindowPos(hwnd, null_mut(), x, 110, width, height, SWP_NOACTIVATE);
        SetWindowRgn(
            hwnd,
            CreateRoundRectRgn(0, 0, width + 1, height + 1, 34, 34),
            1,
        );
    }
}

fn paint(hwnd: Hwnd) {
    let mut paint: PaintStruct = unsafe { std::mem::zeroed() };
    let target = unsafe { BeginPaint(hwnd, &mut paint) };
    let mut client = Rect::default();
    unsafe {
        GetClientRect(hwnd, &mut client);
    }
    let width = client.right;
    let height = client.bottom;
    let memory = unsafe { CreateCompatibleDC(target) };
    let bitmap = unsafe { CreateCompatibleBitmap(target, width, height) };
    let old_bitmap = unsafe { SelectObject(memory, bitmap) };

    fill_round(
        memory,
        Rect {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        },
        34,
        rgb(8, 23, 44),
        Some(rgb(118, 179, 221)),
    );
    fill_round(
        memory,
        Rect {
            left: 22,
            top: 20,
            right: width - 22,
            bottom: 82,
        },
        31,
        rgb(18, 46, 76),
        Some(rgb(92, 166, 215)),
    );
    draw_search_icon(memory, 49, 51);

    let snapshot = with_state_value(|state| {
        (
            state.query.clone(),
            state.hits.clone(),
            state.selected,
            state.status.clone(),
            state.capture_hotkey,
            state.hotkey,
        )
    });
    if let Some((query, hits, selected, status, capturing, hotkey)) = snapshot {
        let display = if query.is_empty() {
            if capturing {
                "Press a shortcut…".to_string()
            } else {
                "Search files instantly…".to_string()
            }
        } else {
            query
        };
        draw_text(
            memory,
            &display,
            Rect {
                left: 82,
                top: 22,
                right: width - 42,
                bottom: 82,
            },
            24,
            FW_SEMIBOLD,
            if display == "Search files instantly…" {
                rgb(147, 177, 203)
            } else {
                rgb(239, 249, 255)
            },
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );

        if hits.is_empty() {
            draw_text(
                memory,
                &status,
                Rect {
                    left: 34,
                    top: 91,
                    right: width - 34,
                    bottom: height - 12,
                },
                14,
                FW_NORMAL,
                rgb(142, 185, 216),
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        } else {
            for (index, hit) in hits.iter().take(9).enumerate() {
                let top = 94 + index as i32 * 54;
                if index == selected {
                    fill_round(
                        memory,
                        Rect {
                            left: 20,
                            top,
                            right: width - 20,
                            bottom: top + 49,
                        },
                        13,
                        rgb(28, 78, 119),
                        Some(rgb(92, 178, 226)),
                    );
                }
                let marker = if hit.entry.kind == EntryKind::Directory {
                    "□"
                } else {
                    "·"
                };
                draw_text(
                    memory,
                    marker,
                    Rect {
                        left: 37,
                        top: top + 2,
                        right: 64,
                        bottom: top + 48,
                    },
                    21,
                    FW_SEMIBOLD,
                    if index == selected {
                        rgb(116, 221, 245)
                    } else {
                        rgb(91, 166, 211)
                    },
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
                draw_text(
                    memory,
                    &hit.entry.name,
                    Rect {
                        left: 72,
                        top: top + 4,
                        right: width - 42,
                        bottom: top + 29,
                    },
                    17,
                    FW_SEMIBOLD,
                    rgb(235, 247, 255),
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
                draw_text(
                    memory,
                    &hit.entry.path.to_string_lossy(),
                    Rect {
                        left: 72,
                        top: top + 27,
                        right: width - 42,
                        bottom: top + 48,
                    },
                    12,
                    FW_NORMAL,
                    rgb(131, 170, 199),
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
            }
            let footer = format!(
                "↑↓ Navigate   Enter Open   Ctrl+Enter Folder   F2 Hotkey ({})",
                hotkey_label(hotkey)
            );
            draw_text(
                memory,
                &footer,
                Rect {
                    left: 26,
                    top: height - 30,
                    right: width - 26,
                    bottom: height - 7,
                },
                11,
                FW_NORMAL,
                rgb(105, 153, 188),
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
    }

    unsafe {
        BitBlt(target, 0, 0, width, height, memory, 0, 0, SRCCOPY);
        SelectObject(memory, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(memory);
        EndPaint(hwnd, &paint);
    }
}

fn fill_round(hdc: Hdc, rect: Rect, radius: i32, color: u32, border: Option<u32>) {
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
    fill_round(
        hdc,
        Rect {
            left: x - 9,
            top: y - 10,
            right: x + 9,
            bottom: y + 8,
        },
        18,
        rgb(83, 185, 224),
        None,
    );
    fill_round(
        hdc,
        Rect {
            left: x - 5,
            top: y - 6,
            right: x + 5,
            bottom: y + 4,
        },
        10,
        rgb(18, 46, 76),
        None,
    );
    fill_round(
        hdc,
        Rect {
            left: x + 6,
            top: y + 5,
            right: x + 15,
            bottom: y + 9,
        },
        4,
        rgb(83, 185, 224),
        None,
    );
}

fn draw_text(
    hdc: Hdc,
    text: &str,
    mut rect: Rect,
    size: i32,
    weight: i32,
    color: u32,
    format: u32,
) {
    let face = wide("Segoe UI Variable Text");
    let font = unsafe {
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
        )
    };
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
        DeleteObject(font);
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

