//! The tray icon's right-click menu: a small borderless window drawn with the app's own
//! components and placed beside the icon, instead of Windows' own menu.
use super::*;
use eframe::egui::{Key, StrokeKind, ViewportBuilder, ViewportId};
use std::time::{Duration, Instant};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

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
}
impl TrayMenu {
    pub(in crate::app) fn new(at: (f64, f64)) -> Self {
        Self {
            at,
            opened: Instant::now(),
            focused: false,
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
        // The window is sized in points of the interface scale and placed in the points
        // the operating system uses, with the click given in physical pixels.
        let native = ctx
            .input(|i| i.viewport().native_pixels_per_point)
            .unwrap_or(1.0);
        let size = Vec2::new(WIDTH, HEIGHT) * ctx.zoom_factor();
        // SAFETY: GetSystemMetrics takes a plain index and has no preconditions.
        let screen = unsafe {
            Vec2::new(
                GetSystemMetrics(SM_CXSCREEN) as f32,
                GetSystemMetrics(SM_CYSCREEN) as f32,
            )
        } / native;
        let click = Pos2::new(menu.at.0 as f32, menu.at.1 as f32) / native;
        let x = (click.x - size.x + 12.0).clamp(0.0, (screen.x - size.x).max(0.0));
        let y = if click.y >= size.y + 12.0 {
            click.y - size.y - 6.0
        } else {
            click.y + 6.0
        };
        let builder = ViewportBuilder::default()
            .with_title("OpenNetworkMonitor menu")
            .with_decorations(false)
            .with_resizable(false)
            .with_transparent(true)
            .with_taskbar(false)
            .with_always_on_top()
            .with_inner_size(size)
            .with_position(Pos2::new(x, y));
        let paused = tray.paused.load(Ordering::Acquire);
        let mut chosen = None;
        let mut close = false;
        ctx.show_viewport_immediate(ViewportId::from_hash_of("tray-menu"), builder, |ui, _| {
            let focused = ui.input(|i| i.viewport().focused).unwrap_or(false);
            menu.focused |= focused;
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
