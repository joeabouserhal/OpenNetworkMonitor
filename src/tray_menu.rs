//! The tray icon's right-click menu: a small popup window drawn with Win32 in the app's colours
//! and Inter type. It is not an egui window on purpose: while the main window is hidden in the
//! tray eframe runs no interface code at all, and the menu has to work exactly then.
use std::{
    cell::RefCell,
    sync::{Mutex, OnceLock},
};
use windows_sys::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
    Graphics::{
        Dwm::{DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DwmSetWindowAttribute},
        Gdi::{
            AddFontMemResourceEx, BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
            CreateFontW, CreatePen, CreateSolidBrush, DT_END_ELLIPSIS, DT_LEFT, DT_SINGLELINE,
            DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetMonitorInfoW,
            HDC, HGDIOBJ, InvalidateRect, LineTo, MONITOR_DEFAULTTONEAREST, MONITORINFO,
            MonitorFromPoint, MoveToEx, PAINTSTRUCT, PS_SOLID, Polygon, RoundRect, SRCCOPY,
            SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
        },
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::{
        HiDpi::GetDpiForWindow,
        WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, HWND_TOPMOST, IDC_ARROW,
            LoadCursorW, RegisterClassW, SW_SHOWNOACTIVATE, SWP_SHOWWINDOW, SetForegroundWindow,
            SetWindowPos, ShowWindow, WA_INACTIVE, WM_ACTIVATE, WM_DESTROY, WM_ERASEBKGND,
            WM_KEYDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT, WNDCLASSW, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_POPUP,
        },
    },
};

/// What a menu entry asks the app to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Settings,
    TogglePause,
    Exit,
}

/// The colours of the menu, as 0xRRGGBB, taken from the app's current theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: u32,
    pub border: u32,
    pub text: u32,
    pub muted: u32,
    pub hover: u32,
    pub danger: u32,
    pub dark: bool,
}
impl Palette {
    pub fn from_tokens(tokens: &crate::ui::tokens::Tokens) -> Self {
        let over = tokens.popover;
        // Translucent colours (the dark theme's borders) are laid over the menu's background.
        let solid = |c: eframe::egui::Color32| {
            let a = u32::from(c.a());
            let mix =
                |top: u8, under: u8| (u32::from(top) + u32::from(under) * (255 - a) / 255).min(255);
            mix(c.r(), over.r()) << 16 | mix(c.g(), over.g()) << 8 | mix(c.b(), over.b())
        };
        Self {
            background: solid(over),
            border: solid(tokens.border),
            text: solid(tokens.foreground),
            muted: solid(tokens.muted_foreground),
            hover: solid(tokens.accent),
            danger: solid(tokens.destructive),
            dark: tokens.is_dark(),
        }
    }
}

static PALETTE: Mutex<Option<Palette>> = Mutex::new(None);
/// Called by the window whenever its theme changes, so the menu matches it.
pub fn set_palette(palette: Palette) {
    if let Ok(mut current) = PALETTE.lock() {
        *current = Some(palette);
    }
}

/// How the app carries out a choice, set once when the tray is created.
type Handler = Box<dyn Fn(Action) + Send + Sync>;
static HANDLER: OnceLock<Handler> = OnceLock::new();
pub fn set_handler(handler: impl Fn(Action) + Send + Sync + 'static) {
    let _ = HANDLER.set(Box::new(handler));
}

// Sizes are in points at 96 dpi and scaled by the monitor's dpi.
const WIDTH: i32 = 244;
const PADDING: i32 = 6;
const ROW: i32 = 32;
const SEPARATOR: i32 = 5;
const RADIUS: i32 = 6;

#[derive(Clone, Copy)]
enum Glyph {
    Overview,
    Settings,
    Pause,
    Play,
    Close,
}

struct Entry {
    action: Action,
    glyph: Glyph,
    label: &'static str,
    danger: bool,
    /// A line above this entry.
    separated: bool,
}

fn entries(paused: bool) -> [Entry; 4] {
    [
        Entry {
            action: Action::Open,
            glyph: Glyph::Overview,
            label: "Open OpenNetworkMonitor",
            danger: false,
            separated: false,
        },
        Entry {
            action: Action::Settings,
            glyph: Glyph::Settings,
            label: "Open Settings",
            danger: false,
            separated: false,
        },
        Entry {
            action: Action::TogglePause,
            glyph: if paused { Glyph::Play } else { Glyph::Pause },
            label: if paused {
                "Resume recording"
            } else {
                "Pause recording"
            },
            danger: false,
            separated: true,
        },
        Entry {
            action: Action::Exit,
            glyph: Glyph::Close,
            label: "Exit",
            danger: true,
            separated: true,
        },
    ]
}

/// The height of the menu in points.
fn height() -> i32 {
    2 * PADDING + 4 * ROW + 2 * SEPARATOR
}

/// The top of entry `index` in points, from the top of the menu.
fn top_of(index: usize) -> i32 {
    let separators = entries(false)[..=index]
        .iter()
        .filter(|entry| entry.separated)
        .count() as i32;
    PADDING + index as i32 * ROW + separators * SEPARATOR
}

/// Which entry a vertical position in points falls on.
fn entry_at(y: i32) -> Option<usize> {
    (0..4).find(|&index| (top_of(index)..top_of(index) + ROW).contains(&y))
}

/// Where the top left of a menu of `size` goes when the icon was clicked at `click`: above the
/// click when there is room, otherwise below it, and always inside `screen`.
fn place(click: (i32, i32), size: (i32, i32), screen: RECT) -> (i32, i32) {
    let x = (click.0 - size.0 + 12).clamp(screen.left, (screen.right - size.0).max(screen.left));
    let above = click.1 - size.1 - 6;
    let y = if above >= screen.top {
        above
    } else {
        (click.1 + 6).clamp(screen.top, (screen.bottom - size.1).max(screen.top))
    };
    (x, y)
}

struct Menu {
    entries: [Entry; 4],
    hover: Option<usize>,
    scale: f32,
    palette: Palette,
}
thread_local! {
    /// The open menu. The window and everything that touches this live on the interface thread.
    static OPEN: RefCell<Option<(HWND, Menu)>> = const { RefCell::new(None) };
}

const FONT: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
fn load_font() {
    static LOADED: OnceLock<()> = OnceLock::new();
    LOADED.get_or_init(|| {
        let count = 0u32;
        // SAFETY: the pointer and length describe a static font file; the font stays loaded
        // for the life of the process.
        unsafe {
            AddFontMemResourceEx(
                FONT.as_ptr().cast(),
                FONT.len() as u32,
                std::ptr::null(),
                &count,
            );
        }
    });
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
fn colorref(rgb: u32) -> COLORREF {
    (rgb >> 16 & 0xff) | (rgb & 0xff00) | (rgb & 0xff) << 16
}

/// Opens the menu beside the tray icon that was clicked at `at`, in physical pixels.
pub fn open(at: (f64, f64), paused: bool) {
    close();
    load_font();
    let palette = PALETTE
        .lock()
        .ok()
        .and_then(|palette| *palette)
        .unwrap_or(Palette {
            background: 0x18181b,
            border: 0x3f3f46,
            text: 0xfafafa,
            muted: 0xa1a1aa,
            hover: 0x27272a,
            danger: 0xf87171,
            dark: true,
        });
    let class = wide("OpenNetworkMonitorTrayMenu");
    let click = (at.0 as i32, at.1 as i32);
    // SAFETY: plain Win32 window creation on this thread; every pointer passed is valid for
    // the call, and the window procedure below only touches this thread's own state.
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(procedure),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: module,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class.as_ptr(),
        };
        // Registering again is refused harmlessly once the class exists.
        RegisterClassW(&window_class);
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            std::ptr::null(),
            WS_POPUP,
            click.0,
            click.1,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            module,
            std::ptr::null(),
        );
        if window.is_null() {
            return;
        }
        let dpi = GetDpiForWindow(window).max(96);
        let scale = dpi as f32 / 96.0;
        let size = (
            (WIDTH as f32 * scale).round() as i32,
            (height() as f32 * scale).round() as i32,
        );
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let monitor = MonitorFromPoint(
            POINT {
                x: click.0,
                y: click.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        let screen = if GetMonitorInfoW(monitor, &mut info) != 0 {
            info.rcMonitor
        } else {
            RECT {
                left: 0,
                top: 0,
                right: click.0 + size.0,
                bottom: click.1 + size.1,
            }
        };
        let (x, y) = place(click, size, screen);
        // Windows 11 rounds the corners and draws the border; older Windows keeps them square.
        let round: i32 = 2;
        DwmSetWindowAttribute(
            window,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            (&raw const round).cast(),
            4,
        );
        let border = colorref(palette.border);
        DwmSetWindowAttribute(
            window,
            DWMWA_BORDER_COLOR as u32,
            (&raw const border).cast(),
            4,
        );
        OPEN.with(|open| {
            *open.borrow_mut() = Some((
                window,
                Menu {
                    entries: entries(paused),
                    hover: None,
                    scale,
                    palette,
                },
            ));
        });
        SetWindowPos(window, HWND_TOPMOST, x, y, size.0, size.1, SWP_SHOWWINDOW);
        ShowWindow(window, SW_SHOWNOACTIVATE);
        // A click on the tray icon lets this process take the foreground; the menu needs it to
        // notice when the user clicks elsewhere.
        SetForegroundWindow(window);
    }
}

/// Closes the menu if it is open.
pub fn close() {
    let window = OPEN.with(|open| open.borrow().as_ref().map(|(window, _)| *window));
    if let Some(window) = window {
        // SAFETY: the handle is this thread's own menu window.
        unsafe { DestroyWindow(window) };
    }
}

fn choose(index: usize) {
    let action = OPEN.with(|open| {
        open.borrow()
            .as_ref()
            .map(|(_, menu)| menu.entries[index].action)
    });
    close();
    if let (Some(action), Some(handler)) = (action, HANDLER.get()) {
        handler(action);
    }
}

/// # Safety
/// Called only by Windows as the menu window's procedure.
unsafe extern "system" fn procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(window);
            0
        }
        WM_MOUSEMOVE => {
            let y = (lparam >> 16) as i16 as i32;
            let changed = OPEN.with(|open| {
                let mut open = open.borrow_mut();
                let Some((_, menu)) = open.as_mut() else {
                    return false;
                };
                let hover = entry_at((y as f32 / menu.scale) as i32);
                std::mem::replace(&mut menu.hover, hover) != hover
            });
            if changed {
                // SAFETY: repaints this window.
                unsafe { InvalidateRect(window, std::ptr::null(), 0) };
            }
            0
        }
        WM_LBUTTONUP => {
            let y = (lparam >> 16) as i16 as i32;
            let scale = OPEN.with(|open| open.borrow().as_ref().map(|(_, menu)| menu.scale));
            if let Some(index) = scale.and_then(|scale| entry_at((y as f32 / scale) as i32)) {
                choose(index);
            }
            0
        }
        WM_KEYDOWN => {
            match wparam as u32 {
                0x1B => close(),
                0x26 | 0x28 => {
                    let down = wparam as u32 == 0x28;
                    OPEN.with(|open| {
                        if let Some((_, menu)) = open.borrow_mut().as_mut() {
                            menu.hover = Some(match (menu.hover, down) {
                                (None, true) => 0,
                                (None, false) => 3,
                                (Some(i), true) => (i + 1) % 4,
                                (Some(i), false) => (i + 3) % 4,
                            });
                        }
                    });
                    // SAFETY: repaints this window.
                    unsafe { InvalidateRect(window, std::ptr::null(), 0) };
                }
                0x0D => {
                    if let Some(index) =
                        OPEN.with(|open| open.borrow().as_ref().and_then(|(_, menu)| menu.hover))
                    {
                        choose(index);
                    }
                }
                _ => {}
            }
            0
        }
        // Clicking anywhere else, or switching windows, dismisses the menu.
        WM_ACTIVATE if (wparam & 0xffff) as u32 == WA_INACTIVE => {
            close();
            0
        }
        WM_DESTROY => {
            OPEN.with(|open| *open.borrow_mut() = None);
            0
        }
        // SAFETY: forwards Windows' own arguments untouched.
        _ => unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
}

fn paint(window: HWND) {
    let Some((scale, hover, palette, items)) = OPEN.with(|open| {
        open.borrow().as_ref().map(|(_, menu)| {
            (
                menu.scale,
                menu.hover,
                menu.palette,
                menu.entries
                    .iter()
                    .map(|e| (e.glyph, e.label, e.danger, e.separated))
                    .collect::<Vec<_>>(),
            )
        })
    }) else {
        return;
    };
    let px = |points: i32| (points as f32 * scale).round() as i32;
    // SAFETY: GDI calls on a device context obtained from BeginPaint for this window; every
    // object created here is deselected and deleted before returning.
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let screen = BeginPaint(window, &mut ps);
        let mut area: RECT = std::mem::zeroed();
        GetClientRect(window, &mut area);
        // Draw off screen and copy in one go, so hovering never flickers.
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, area.right, area.bottom);
        let old_bitmap = SelectObject(dc, bitmap as HGDIOBJ);
        let background = CreateSolidBrush(colorref(palette.background));
        FillRect(dc, &area, background);
        let font = CreateFontW(
            -px(14),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            wide("Inter").as_ptr(),
        );
        let old_font = SelectObject(dc, font as HGDIOBJ);
        SetBkMode(dc, TRANSPARENT as i32);
        for (index, (glyph, label, danger, separated)) in items.iter().enumerate() {
            let top = px(top_of(index));
            if *separated {
                let line = CreateSolidBrush(colorref(palette.border));
                let rect = RECT {
                    left: px(PADDING - 2),
                    top: top - px(SEPARATOR) / 2 - 1,
                    right: area.right - px(PADDING - 2),
                    bottom: top - px(SEPARATOR) / 2,
                };
                FillRect(dc, &rect, line);
                DeleteObject(line as HGDIOBJ);
            }
            let row = RECT {
                left: px(PADDING),
                top,
                right: area.right - px(PADDING),
                bottom: top + px(ROW),
            };
            if hover == Some(index) {
                let brush = CreateSolidBrush(colorref(palette.hover));
                let pen = CreatePen(PS_SOLID, 0, colorref(palette.hover));
                let (old_brush, old_pen) = (
                    SelectObject(dc, brush as HGDIOBJ),
                    SelectObject(dc, pen as HGDIOBJ),
                );
                RoundRect(
                    dc,
                    row.left,
                    row.top,
                    row.right,
                    row.bottom,
                    px(RADIUS * 2),
                    px(RADIUS * 2),
                );
                SelectObject(dc, old_brush);
                SelectObject(dc, old_pen);
                DeleteObject(brush as HGDIOBJ);
                DeleteObject(pen as HGDIOBJ);
            }
            let ink = if *danger {
                palette.danger
            } else {
                palette.text
            };
            glyph_at(
                dc,
                *glyph,
                (row.left + px(16), (row.top + row.bottom) / 2),
                scale,
                if *danger {
                    palette.danger
                } else {
                    palette.muted
                },
            );
            SetTextColor(dc, colorref(ink));
            let mut text_area = RECT {
                left: row.left + px(34),
                top: row.top,
                right: row.right - px(8),
                bottom: row.bottom,
            };
            let text = wide(label);
            DrawTextW(
                dc,
                text.as_ptr(),
                -1,
                &mut text_area,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            );
        }
        SelectObject(dc, old_font);
        DeleteObject(font as HGDIOBJ);
        BitBlt(screen, 0, 0, area.right, area.bottom, dc, 0, 0, SRCCOPY);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap as HGDIOBJ);
        DeleteObject(background as HGDIOBJ);
        DeleteDC(dc);
        EndPaint(window, &ps);
    }
}

/// A small line icon centred on `center`, 16 points wide.
///
/// # Safety
/// `dc` must be a valid device context.
unsafe fn glyph_at(dc: HDC, glyph: Glyph, center: (i32, i32), scale: f32, color: u32) {
    let px = |points: f32| (points * scale).round() as i32;
    let width = px(1.5).max(1);
    // SAFETY: the pen and brush are deselected and deleted before returning.
    unsafe {
        let pen = CreatePen(PS_SOLID, width, colorref(color));
        let brush = CreateSolidBrush(colorref(color));
        let old_pen = SelectObject(dc, pen as HGDIOBJ);
        let old_brush = SelectObject(dc, windows_sys::Win32::Graphics::Gdi::GetStockObject(5)); // NULL_BRUSH
        let (cx, cy) = center;
        let line = |x1: f32, y1: f32, x2: f32, y2: f32| {
            MoveToEx(dc, cx + px(x1), cy + px(y1), std::ptr::null_mut());
            LineTo(dc, cx + px(x2), cy + px(y2));
        };
        let square = |x1: f32, y1: f32, x2: f32, y2: f32| {
            RoundRect(
                dc,
                cx + px(x1),
                cy + px(y1),
                cx + px(x2),
                cy + px(y2),
                px(2.0),
                px(2.0),
            );
        };
        match glyph {
            Glyph::Overview => {
                square(-6.5, -6.5, -1.0, -1.0);
                square(1.0, -6.5, 6.5, -1.0);
                square(-6.5, 1.0, -1.0, 6.5);
                square(1.0, 1.0, 6.5, 6.5);
            }
            Glyph::Settings => {
                line(-6.5, -4.0, 6.5, -4.0);
                line(-6.5, 0.0, 6.5, 0.0);
                line(-6.5, 4.0, 6.5, 4.0);
                square(-3.5, -5.5, -1.5, -2.5);
                square(2.0, -1.5, 4.0, 1.5);
                square(-2.0, 2.5, 0.0, 5.5);
            }
            Glyph::Pause => {
                line(-2.5, -5.0, -2.5, 5.0);
                line(2.5, -5.0, 2.5, 5.0);
            }
            Glyph::Play => {
                SelectObject(dc, brush as HGDIOBJ);
                let points = [
                    POINT {
                        x: cx + px(-3.0),
                        y: cy + px(-5.0),
                    },
                    POINT {
                        x: cx + px(5.0),
                        y: cy,
                    },
                    POINT {
                        x: cx + px(-3.0),
                        y: cy + px(5.0),
                    },
                ];
                Polygon(dc, points.as_ptr(), 3);
            }
            Glyph::Close => {
                line(-4.5, -4.5, 4.5, 4.5);
                line(4.5, -4.5, -4.5, 4.5);
            }
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(pen as HGDIOBJ);
        DeleteObject(brush as HGDIOBJ);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1440,
        }
    }

    #[test]
    fn the_menu_opens_above_the_icon_and_stays_on_screen() {
        // A taskbar icon at the bottom right: the menu sits just above it, inside the screen.
        assert_eq!(place((2500, 1410), (244, 150), screen()), (2268, 1254));
        assert_eq!(place((2555, 1410), (244, 150), screen()).0, 2316);
        // At the left edge it cannot go off the screen.
        assert_eq!(place((5, 1410), (244, 150), screen()).0, 0);
        // A taskbar at the top: the menu opens below the icon.
        assert_eq!(place((2500, 20), (244, 150), screen()), (2268, 26));
    }

    #[test]
    fn rows_are_found_by_their_position() {
        assert_eq!(entry_at(0), None);
        assert_eq!(entry_at(top_of(0) + 1), Some(0));
        assert_eq!(entry_at(top_of(2) + ROW - 1), Some(2));
        assert_eq!(entry_at(top_of(2) - 1), None, "the line between rows");
        assert_eq!(entry_at(top_of(3) + 1), Some(3));
        assert_eq!(entry_at(height()), None);
        assert_eq!(top_of(3) + ROW + PADDING, height());
    }

    #[test]
    fn colours_become_windows_colorrefs() {
        assert_eq!(colorref(0x112233), 0x332211);
    }
}
