//! The window: sidebar, page header, banners and the five pages.
use super::*;
use crate::ui::{
    button::{Button, Size, Variant},
    controls::{self, AlertVariant},
    icon::{self, Icon},
    layout, text,
    tokens::{hairline, tk},
};
use eframe::egui::{Align, Align2, Layout, Margin, Pos2, Rect, Sense, UiBuilder, Vec2};

mod adapters;
mod apps;
mod connections;
mod dialogs;
mod overview;
mod settings;
mod toolbar;

pub const NAV_WIDTH: f32 = 232.0;
pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [1440.0, 820.0];
/// The smallest window, in physical pixels at the default interface size. Pages are laid out
/// for a fixed logical size and up, so nothing is hidden or rearranged in a small window.
pub const MIN_WINDOW_SIZE: [f32; 2] = [952.0, 663.0];
/// The smallest logical size every page is built for, in points at any interface size.
pub const MIN_LOGICAL_SIZE: [f32; 2] = [1120.0, 780.0];
/// Page content is centred and no wider than this.
const CONTENT_MAX_WIDTH: f32 = 1240.0;
const PAGE_PADDING: f32 = 24.0;
/// Space kept under the last card on a page.
pub(super) const PAGE_BOTTOM: f32 = 20.0;

fn page_icon(page: Page) -> Icon {
    match page {
        Page::Overview => Icon::Overview,
        Page::Connections => Icon::Connections,
        Page::Adapters => Icon::Adapters,
        Page::Settings => Icon::Settings,
    }
}

impl MonitorApp {
    pub(super) fn draw(&mut self, root: &mut egui::Ui) -> Rect {
        let t = tk(root.ctx());
        egui::Panel::left("navigation")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(NAV_WIDTH)
            .frame(
                egui::Frame::new()
                    .fill(t.background)
                    .inner_margin(Margin::symmetric(12, 16)),
            )
            .show(root, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(t.background))
            .show(root, |ui| {
                let area = ui.available_rect_before_wrap();
                // The sidebar's right border runs the full height of the window.
                ui.painter()
                    .vline(area.left(), area.y_range(), hairline(ui.ctx(), t.border));
                let width = (area.width() - PAGE_PADDING * 2.0).clamp(300.0, CONTENT_MAX_WIDTH);
                let left = area.left() + (area.width() - width) / 2.0;
                let page = Rect::from_min_size(
                    Pos2::new(left, area.top() + 20.0),
                    Vec2::new(width, area.height() - 20.0),
                );
                ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
                    self.page_header(ui);
                    self.banners(ui);
                    // Pages fill the window; only lists and the long settings form scroll.
                    match self.page {
                        Page::Overview => self.overview(ui),
                        Page::Connections => self.connections(ui),
                        Page::Adapters => self.adapters(ui),
                        Page::Settings => self.settings(ui),
                    }
                });
            })
            .response
            .rect
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        ui.spacing_mut().item_spacing.y = 2.0;
        for (index, page) in Page::ALL.into_iter().enumerate() {
            let shortcut = format!("Ctrl+{}", index + 1);
            if nav_item(
                ui,
                page_icon(page),
                page.label(),
                &shortcut,
                self.page == page,
            )
            .clicked()
            {
                self.page = page;
            }
        }
        // Status at the bottom.
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let active = self.data.snapshot.etw_active;
            let paused = self.data.snapshot.recording_paused;
            let (tone, status) = if paused {
                (t.warning, "Recording paused")
            } else if active {
                (t.success, "Monitoring active")
            } else {
                (t.warning, "Capture unavailable")
            };
            layout::card(ui, 12, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, status)
                });
                ui.painter()
                    .circle_filled(Pos2::new(row.left() + 4.0, row.center().y), 3.5, tone);
                text::paint(
                    ui.painter(),
                    Rect::from_min_max(Pos2::new(row.left() + 16.0, row.top()), row.max),
                    Align2::LEFT_CENTER,
                    status,
                    text::medium(13.0),
                    t.foreground,
                );
                let counts = format!(
                    "{} adapters · {} connections",
                    self.data.snapshot.adapters.len(),
                    self.data.snapshot.connections.len()
                );
                let (row, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
                text::paint(
                    ui.painter(),
                    row,
                    Align2::LEFT_CENTER,
                    &counts,
                    text::regular(12.0),
                    t.muted_foreground,
                );
                if let Some(cap) = self.data.cap {
                    let share = cap.used as f64 / cap.limit.max(1) as f64;
                    let level = crate::alerts::cap_level(cap.used, cap.limit);
                    let tone = match level {
                        crate::alerts::CapLevel::Below => None,
                        crate::alerts::CapLevel::Warning => Some(t.warning),
                        crate::alerts::CapLevel::Reached => Some(t.destructive),
                    };
                    let (row, _) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
                    text::paint(
                        ui.painter(),
                        row,
                        Align2::LEFT_CENTER,
                        &format!("Data cap {:.0}% used", share * 100.0),
                        text::regular(12.0),
                        tone.unwrap_or(t.muted_foreground),
                    );
                    controls::progress(ui, share as f32, tone);
                }
            });
        });
    }

    /// The page title and description on the left, page actions on the right.
    fn page_header(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        let (title, description) = match self.page {
            Page::Overview => ("Overview", "Network usage across your adapters and apps."),
            Page::Connections => (
                "Connections",
                "Live TCP connections and UDP sockets on this PC.",
            ),
            Page::Adapters => ("Adapters", "Network adapters and their current speeds."),
            Page::Settings => (
                "Settings",
                "Appearance, background monitoring, alerts and saved history.",
            ),
        };
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width() - 180.0, 32.0),
                    Sense::hover(),
                );
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title)
                });
                text::paint(
                    ui.painter(),
                    rect,
                    Align2::LEFT_CENTER,
                    title,
                    text::semibold(24.0),
                    t.foreground,
                );
                let (rect, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width() - 180.0, 20.0),
                    Sense::hover(),
                );
                text::paint(
                    ui.painter(),
                    rect,
                    Align2::LEFT_CENTER,
                    description,
                    text::regular(14.0),
                    t.muted_foreground,
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.page == Page::Overview
                    && Button::new("Export CSV")
                        .icon(Icon::Download)
                        .variant(Variant::Outline)
                        .show(ui)
                        .on_hover_text("Export the usage shown, as CSV (Ctrl+E)")
                        .clicked()
                {
                    let ctx = ui.ctx().clone();
                    self.export(&ctx);
                }
            });
        });
    }

    /// Messages under the header: your own notices first, then capture warnings, which stay
    /// visible on every page.
    fn banners(&mut self, ui: &mut egui::Ui) {
        if !self.view_warning.is_empty() {
            let message = self.view_warning.clone();
            controls::alert(
                ui,
                AlertVariant::Destructive,
                Icon::Alert,
                "Tray unavailable",
                &message,
                false,
            );
        }
        if !self.data.notice.is_empty() {
            let message = self.data.notice.clone();
            let expired = self.notice_expired(ui.ctx());
            let dismissed =
                controls::alert(ui, AlertVariant::Default, Icon::Info, &message, "", true);
            if dismissed || expired {
                self.clear_notice();
            }
        }
        for warning in self.data.snapshot.warnings.clone() {
            controls::alert(
                ui,
                AlertVariant::Destructive,
                Icon::Alert,
                &warning,
                "",
                false,
            );
        }
        if !self.data.snapshot.etw_active && !self.data.snapshot.capture_status.is_empty() {
            let message = self.data.snapshot.capture_status.clone();
            controls::alert(
                ui,
                AlertVariant::Destructive,
                Icon::Alert,
                "Detailed app capture is unavailable",
                &message,
                false,
            );
        }
        let snapshot = &self.data.snapshot;
        let losses = snapshot.parse_errors + snapshot.queue_drops + snapshot.kernel_losses;
        if losses > 0 {
            let message = format!(
                "{} decode errors, {} queue drops and {} kernel losses. Some app traffic may be missing.",
                snapshot.parse_errors, snapshot.queue_drops, snapshot.kernel_losses
            );
            controls::alert(
                ui,
                AlertVariant::Destructive,
                Icon::Alert,
                "Capture reported losses",
                &message,
                false,
            );
        }
        if self.page == Page::Overview && self.settings.intro_seen == Some(false) {
            self.intro_card(ui);
        }
    }

    /// A short welcome for a first launch, shown until it is dismissed.
    fn intro_card(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        let mut done = false;
        let mut open_settings = false;
        layout::card(ui, 20, |ui| {
            layout::card_header(
                ui,
                "Welcome to OpenNetworkMonitor",
                "A quick orientation before you start.",
                |_| {},
            );
            ui.add_space(8.0);
            for line in [
                "Overall traffic is everything each network adapter carries. App traffic shows which apps and hosts used it, and needs administrator rights.",
                "Right-click a connection for actions, and use Ctrl+1 to Ctrl+4 to switch pages.",
                "Settings has data-cap and new-app alerts, and an optional country database for IP lookups.",
            ] {
                let galley = ui.painter().layout(
                    line.to_owned(),
                    text::regular(14.0),
                    t.muted_foreground,
                    ui.available_width(),
                );
                let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
                ui.painter().galley(rect.min, galley, t.muted_foreground);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                done = Button::new("Got it").show(ui).clicked();
                open_settings = Button::new("Open settings")
                    .variant(Variant::Outline)
                    .show(ui)
                    .clicked();
            });
        });
        if done || open_settings {
            self.settings.intro_seen = Some(true);
        }
        if open_settings {
            self.page = Page::Settings;
        }
    }

    /// Whether the message on screen has been there long enough to go away by itself.
    /// A new message starts its own time; one about damaged history stays until dismissed.
    fn notice_expired(&mut self, ctx: &egui::Context) -> bool {
        let text = &self.data.notice;
        let shown = match &self.notice_seen {
            Some((seen, at)) if seen == text => *at,
            _ => {
                let now = Instant::now();
                self.notice_seen = Some((text.clone(), now));
                now
            }
        };
        if notice_is_sticky(text) {
            return false;
        }
        let age = shown.elapsed();
        if age >= NOTICE_LIFETIME {
            return true;
        }
        ctx.request_repaint_after(NOTICE_LIFETIME - age);
        false
    }

    /// Removes the message from the window and from the worker, but only if the worker
    /// still holds this same message; a newer one may have arrived since it was drawn.
    fn clear_notice(&mut self) {
        let dismissed = std::mem::take(&mut self.data.notice);
        if let Ok(mut state) = self.worker.shared.lock()
            && state.notice == dismissed
        {
            state.notice.clear();
        }
        self.notice_seen = None;
    }
}

/// How long an information message stays before it goes away on its own.
const NOTICE_LIFETIME: Duration = Duration::from_secs(15);

/// Messages that report damage are kept until the user dismisses them.
fn notice_is_sticky(text: &str) -> bool {
    text.contains("integrity check")
}

/// A column inside a row of cards: a fixed width, with its content stacked downwards.
fn cell<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(Vec2::new(width, 4000.0), Layout::top_down(Align::Min), add)
        .inner
}

/// One sidebar destination.
fn nav_item(
    ui: &mut egui::Ui,
    glyph: Icon,
    label: &str,
    shortcut: &str,
    active: bool,
) -> egui::Response {
    let t = tk(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, active, label)
    });
    let painter = ui.painter();
    if active || response.hovered() {
        painter.rect_filled(
            rect,
            egui::CornerRadius::same(8),
            if active {
                t.accent
            } else {
                t.accent.gamma_multiply(0.6)
            },
        );
    }
    if response.has_focus() {
        crate::ui::focus_ring(painter, ui.ctx(), rect, egui::CornerRadius::same(8), &t);
    }
    let ink = if active {
        t.foreground
    } else {
        t.muted_foreground
    };
    icon::paint(
        painter,
        glyph,
        Pos2::new(rect.left() + 20.0, rect.center().y),
        17.0,
        ink,
    );
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(rect.left() + 38.0, rect.top()),
            Pos2::new(rect.right() - 60.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        if active {
            text::medium(14.0)
        } else {
            text::regular(14.0)
        },
        if active {
            t.foreground
        } else {
            t.foreground.gamma_multiply(0.85)
        },
    );
    text::paint(
        painter,
        Rect::from_min_max(rect.min, Pos2::new(rect.right() - 10.0, rect.bottom())),
        Align2::RIGHT_CENTER,
        shortcut,
        text::regular(11.5),
        t.muted_foreground,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_damage_reports_outlive_the_timeout() {
        assert!(notice_is_sticky(
            "Saved history failed its integrity check. Preserve usage.db"
        ));
        assert!(!notice_is_sticky("Exported 12 rows to C:/a.csv"));
        assert!(!notice_is_sticky("Data cap reached: you have used 100 GiB"));
    }
}
