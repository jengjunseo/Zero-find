//! Alpha-correct native rendering. GDI remains a readable device-loss fallback.
use super::{Hwnd, Rect, wide};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::{
    Win32::{
        Foundation::HWND,
        Graphics::{
            Direct2D::Common::*, Direct2D::*, DirectWrite::*,
            Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM,
        },
    },
    core::{PCWSTR, Result},
};
static FAILED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static STATE:RefCell<Option<Renderer>>=const {RefCell::new(None)};
    static ACTIVE:Cell<bool>=const {Cell::new(false)};
    static DRAW_FAILED:Cell<bool>=const {Cell::new(false)};
}
pub fn enabled() -> bool {
    !std::env::var("ZEROFIND_RENDERER").is_ok_and(|s| s == "gdi") && !FAILED.load(Ordering::Relaxed)
}
pub fn shutdown() {
    ACTIVE.set(false);
    STATE.with(|state| {
        state.borrow_mut().take();
    });
}
struct Renderer {
    target: ID2D1HwndRenderTarget,
    write: IDWriteFactory,
    brush: ID2D1SolidColorBrush,
    formats: HashMap<(i32, i32, &'static str), IDWriteTextFormat>,
}
impl Renderer {
    unsafe fn new(hwnd: Hwnd, width: u32, height: u32, dpi: u32) -> Result<Self> {
        let factory: ID2D1Factory =
            unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None) }?;
        let write: IDWriteFactory = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED) }?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: dpi as f32,
            dpiY: dpi as f32,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let target = unsafe {
            factory.CreateHwndRenderTarget(
                &props,
                &D2D1_HWND_RENDER_TARGET_PROPERTIES {
                    hwnd: HWND(hwnd),
                    pixelSize: D2D_SIZE_U { width, height },
                    presentOptions: D2D1_PRESENT_OPTIONS_IMMEDIATELY,
                },
            )
        }?;
        let brush = unsafe {
            target.CreateSolidColorBrush(
                &D2D1_COLOR_F {
                    r: 0.,
                    g: 0.,
                    b: 0.,
                    a: 1.,
                },
                None,
            )
        }?;
        unsafe {
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
        }
        Ok(Self {
            target,
            write,
            brush,
            formats: HashMap::new(),
        })
    }
    unsafe fn color(&self, color: u32, alpha: f32) {
        unsafe {
            self.brush.SetColor(&D2D1_COLOR_F {
                r: (color & 255) as f32 / 255.,
                g: ((color >> 8) & 255) as f32 / 255.,
                b: ((color >> 16) & 255) as f32 / 255.,
                a: alpha,
            });
        }
    }
    #[allow(clippy::too_many_arguments)]
    unsafe fn text(
        &mut self,
        text: &str,
        rect: Rect,
        size: i32,
        weight: i32,
        color: u32,
        flags: u32,
        face: &'static str,
    ) -> Result<()> {
        let key = (size, weight, face);
        if !self.formats.contains_key(&key) {
            let family = wide(face);
            let locale = wide("ko-KR");
            let format = unsafe {
                self.write.CreateTextFormat(
                    PCWSTR(family.as_ptr()),
                    None,
                    DWRITE_FONT_WEIGHT(weight),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size as f32,
                    PCWSTR(locale.as_ptr()),
                )
            }?;
            unsafe {
                format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
                format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
            }
            self.formats.insert(key, format);
        }
        let format = &self.formats[&key];
        let units = text.encode_utf16().collect::<Vec<_>>();
        let layout = unsafe {
            self.write.CreateTextLayout(
                &units,
                format,
                (rect.right - rect.left).max(1) as f32,
                (rect.bottom - rect.top).max(1) as f32,
            )
        }?;
        let alignment = if flags & 1 != 0 {
            DWRITE_TEXT_ALIGNMENT_CENTER
        } else if flags & 2 != 0 {
            DWRITE_TEXT_ALIGNMENT_TRAILING
        } else {
            DWRITE_TEXT_ALIGNMENT_LEADING
        };
        unsafe {
            layout.SetTextAlignment(alignment)?;
            let sign = self.write.CreateEllipsisTrimmingSign(format)?;
            layout.SetTrimming(
                &DWRITE_TRIMMING {
                    granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER,
                    delimiter: 0,
                    delimiterCount: 0,
                },
                &sign,
            )?;
            self.color(color, 1.);
            self.target.DrawTextLayout(
                D2D_POINT_2F {
                    x: rect.left as f32,
                    y: rect.top as f32,
                },
                &layout,
                &self.brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
            );
        }
        Ok(())
    }
}
pub fn begin(hwnd: Hwnd, width: i32, height: i32, dpi: u32) -> bool {
    if !enabled() {
        return false;
    }
    let result = STATE.with(|cell| -> Result<()> {
        let mut state = cell.borrow_mut();
        if state.is_none() {
            *state = Some(unsafe {
                Renderer::new(hwnd, width.max(1) as u32, height.max(1) as u32, dpi)
            }?);
        }
        let r = state.as_ref().unwrap();
        unsafe {
            let size = r.target.GetPixelSize();
            if size.width != width as u32 || size.height != height as u32 {
                r.target.Resize(&D2D_SIZE_U {
                    width: width.max(1) as u32,
                    height: height.max(1) as u32,
                })?;
            }
            r.target.SetDpi(dpi as f32, dpi as f32);
            r.target.BeginDraw();
            r.target.Clear(Some(&D2D1_COLOR_F {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            }));
        }
        Ok(())
    });
    if let Err(error) = result {
        if let Some(path) = std::env::var_os("ZEROFIND_RENDER_LOG") {
            let _ = std::fs::write(path, format!("Direct2D initialization failed: {error:?}"));
        }
        FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    DRAW_FAILED.set(false);
    ACTIVE.set(true);
    true
}
pub fn round(rect: Rect, diameter: i32, color: u32, border: Option<u32>) -> bool {
    if !ACTIVE.get() {
        return false;
    }
    STATE.with(|cell| {
        let state = cell.borrow();
        let r = state.as_ref().unwrap();
        let rounded = D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F {
                left: rect.left as f32 + 0.5,
                top: rect.top as f32 + 0.5,
                right: rect.right as f32 - 0.5,
                bottom: rect.bottom as f32 - 0.5,
            },
            radiusX: diameter as f32 / 2.,
            radiusY: diameter as f32 / 2.,
        };
        unsafe {
            r.color(color, if rect.left == 0 { 0.88 } else { 0.97 });
            r.target.FillRoundedRectangle(&rounded, &r.brush);
            if let Some(color) = border {
                r.color(color, 0.8);
                r.target.DrawRoundedRectangle(&rounded, &r.brush, 1., None);
            }
        }
    });
    true
}
pub fn text(
    text: &str,
    rect: Rect,
    size: i32,
    weight: i32,
    color: u32,
    flags: u32,
    face: &'static str,
) -> bool {
    if !ACTIVE.get() {
        return false;
    }
    STATE.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_mut()
            && unsafe { r.text(text, rect, size, weight, color, flags, face) }.is_err()
        {
            DRAW_FAILED.set(true);
        }
    });
    true
}
pub fn finish() -> bool {
    if !ACTIVE.replace(false) {
        return false;
    }
    let result =
        STATE.with(|cell| unsafe { cell.borrow().as_ref().unwrap().target.EndDraw(None, None) });
    if result.is_err() || DRAW_FAILED.get() {
        STATE.with(|c| *c.borrow_mut() = None);
        FAILED.store(true, Ordering::Relaxed);
        return false;
    }
    true
}
