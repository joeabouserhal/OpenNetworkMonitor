//! The tray icon's right-click menu: a small borderless window drawn with the app's own
//! components and placed beside the icon, instead of Windows' own menu.
use super::*;
use eframe::egui::{Key, StrokeKind, ViewportBuilder, ViewportId};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::WindowsAndMessaging::{
        FindWindowW, GetSystemMetrics, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN, SWP_NOACTIVATE,
        SetWindowPos,
    },
};

const TITLE: &str = "OpenNetworkMonitor menu";
const WIDTH: f32 = 244.0;
const PADDING: f32 = 6.0;
const HEIGHT: f32 = 2.0 * PADDING + 2.0 + 4.0 * 32.0 + 2.0 * 5.0;
/// How long the menu waits for Windows to give it focus before it gives up.
const FOCUS_GRACE: Duration = Duration::from_millis(1500);

/// A menu that is open: where it was asked for and whether it has had focus yet.
pub(in crate::app) struct TrayMenu {
    at: (f64, f64),
    opened: Instant,
    focused: bool,
    /// Whether the window has been moved beside the icon yet.
    placed: bool,
}
impl TrayMenu {
    pub(in crate::app) fn new(at: (f64, f64)) -> Self {
        Self {
            at,
            opened: Instant::now(),
            focused: false,
            placed: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Open,
    Settings,
    TogglePause,
    Exit,
}

fn contents(ui: &mut egui::Ui, paused: bool) -> Option<Action> {
    ui.spacing_mut().item_spacing.y = 0.0;
    let mut action = None;
    let (pause_icon, pause) = if paused {
        (Icon::Play, "Resume recording")
    } else {
        (Icon::Pause, "Pause recording")
    };
    for (kind, glyph, label, destructive) in [
        (
            Action::Open,
            Icon::Overview,
            "Open OpenNetworkMonitor",
            false,
        ),
        (Action::Settings, Icon::Settings, "Open Settings", false),
        (Action::TogglePause, pause_icon, pause, false),
        (Action::Exit, Icon::Close, "Exit", true),
    ] {
        if matches!(kind, Action::TogglePause | Action::Exit) {
            layout::menu_separator(ui);
        }
        if layout::menu_action(ui, Some(glyph), label, "", destructive).clicked() {
            action = Some(kind);
        }
    }
    action
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

/// Moves the menu window beside the clicked icon, in physical pixels. False until the window
/// exists.
fn move_window(at: (f64, f64), size: (f32, f32)) -> bool {
    let click = (at.0 as i32, at.1 as i32);
    let size = (size.0.round() as i32, size.1.round() as i32);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        rcMonitor: RECT {
            left: 0,
            top: 0,
            // SAFETY: GetSystemMetrics takes a plain index and has no preconditions.
            right: unsafe { GetSystemMetrics(SM_CXSCREEN) },
            // SAFETY: as above.
            bottom: unsafe { GetSystemMetrics(SM_CYSCREEN) },
        },
        rcWork: RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
        dwFlags: 0,
    };
    let title: Vec<u16> = TITLE.encode_utf16().chain(Some(0)).collect();
    // SAFETY: the point is plain data, `info` is a valid MONITORINFO with its size set, and
    // `title` is a nul-terminated string that outlives the call. On failure `info` keeps the
    // primary screen's size set above.
    unsafe {
        let monitor = MonitorFromPoint(
            POINT {
                x: click.0,
                y: click.1,
            },
            MONITOR_DEFAULTTONEAREST,
        );
        GetMonitorInfoW(monitor, &mut info);
        let window = FindWindowW(std::ptr::null(), title.as_ptr());
        if window.is_null() {
            return false;
        }
        let (x, y) = place(click, size, info.rcMonitor);
        SetWindowPos(window, HWND_TOPMOST, x, y, size.0, size.1, SWP_NOACTIVATE) != 0
    }
}

impl MonitorApp {
    /// Shows the open tray menu, if any, and carries out what is chosen in it.
    pub(in crate::app) fn tray_menu_window(&mut self, ctx: &egui::Context) {
        let Some(menu) = &mut self.tray_menu else {
            return;
        };
        let Some(tray) = &self.tray else {
            self.tray_menu = None;
            return;
        };
        let t = tk(ctx);
        // Only a first guess: egui's window positions are in points of a scale it does not
        // share with this one, so the window is moved in physical pixels once it exists.
        let native = ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(1.0);
        let size = Vec2::new(WIDTH, HEIGHT);
        let guess = Pos2::new(menu.at.0 as f32, menu.at.1 as f32) / native - size;
        let builder = ViewportBuilder::default()
            .with_title(TITLE)
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_taskbar(false)
            .with_always_on_top()
            .with_inner_size(size)
            .with_position(guess);
        let paused = tray.paused.load(Ordering::Acquire);
        let mut chosen = None;
        let mut close = false;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("tray-menu"), builder, |ui, _| {
            let focused = ui.input(|i| i.viewport().focused).unwrap_or(false);
            menu.focused |= focused;
            if !menu.placed {
                // The window is sized in the points it draws in, so measure it the same way.
                let scale = ui.ctx().pixels_per_point();
                menu.placed = move_window(menu.at, (WIDTH * scale, HEIGHT * scale));
            }
            close = (menu.focused && !focused)
                || (!menu.focused && menu.opened.elapsed() > FOCUS_GRACE)
                || ui.input(|i| i.key_pressed(Key::Escape) || i.viewport().close_requested());
            let rect = ui.max_rect();
            let painter = ui.painter();
            painter.rect_filled(rect, egui::CornerRadius::same(8), t.popover);
            painter.rect_stroke(
                rect,
                egui::CornerRadius::same(8),
                hairline(ui.ctx(), t.border),
                StrokeKind::Inside,
            );
            ui.scope_builder(
                UiBuilder::new().max_rect(rect.shrink(PADDING + 1.0)),
                |ui| {
                    chosen = contents(ui, paused);
                },
            );
        });
        if chosen.is_none() && !close {
            return;
        }
        self.tray_menu = None;
        match chosen {
            Some(Action::Open) => tray.open(ctx),
            Some(Action::Settings) => {
                tray.open_settings.store(true, Ordering::Release);
                tray.open(ctx);
            }
            Some(Action::TogglePause) => {
                tray.paused.fetch_xor(true, Ordering::AcqRel);
            }
            Some(Action::Exit) => tray.quit(ctx),
            None => {}
        }
        ctx.request_repaint();
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
        assert_eq!(place((2500, 1410), (244, 152), screen()), (2268, 1252));
        assert_eq!(place((2555, 1410), (244, 152), screen()).0, 2316);
        // At the left edge it cannot go off the screen.
        assert_eq!(place((5, 1410), (244, 152), screen()).0, 0);
        // A taskbar at the top: the menu opens below the icon.
        assert_eq!(place((2500, 20), (244, 152), screen()), (2268, 26));
    }

    #[test]
    fn the_menu_fits_the_window_it_is_shown_in() {
        for paused in [false, true] {
            let ctx = egui::Context::default();
            let _app = MonitorApp::preview(&ctx, "overview", false);
            let mut height = 0.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(WIDTH, HEIGHT))),
                    ..Default::default()
                },
                |ui| {
                    let rect = ui.max_rect().shrink(PADDING + 1.0);
                    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                        contents(ui, paused);
                        height = ui.min_rect().height();
                    });
                },
            );
            output.textures_delta.clear();
            let room = HEIGHT - 2.0 * (PADDING + 1.0);
            assert!(
                (room - 2.0..=room + 0.5).contains(&height),
                "{paused}: the items take {height} of {room}"
            );
        }
    }

    #[test]
    #[ignore = "Exports the menu's geometry for offline visual review"]
    fn export_tray_menu() {
        use crate::ui::{
            scene,
            tokens::{BaseColor, Scheme, Tokens},
        };
        for (name, scheme) in [
            ("tray-menu-dark", Scheme::Dark),
            ("tray-menu-light", Scheme::Light),
        ] {
            scene::export(
                name,
                [WIDTH + 40.0, HEIGHT + 40.0],
                1.0,
                Tokens::new(scheme, BaseColor::Zinc),
                &[],
                |ui| {
                    let t = tk(ui.ctx());
                    let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(WIDTH, HEIGHT));
                    ui.painter()
                        .rect_filled(rect, egui::CornerRadius::same(8), t.popover);
                    ui.painter().rect_stroke(
                        rect,
                        egui::CornerRadius::same(8),
                        hairline(ui.ctx(), t.border),
                        StrokeKind::Inside,
                    );
                    ui.scope_builder(
                        UiBuilder::new().max_rect(rect.shrink(PADDING + 1.0)),
                        |ui| {
                            contents(ui, false);
                        },
                    );
                },
            );
        }
    }
}
