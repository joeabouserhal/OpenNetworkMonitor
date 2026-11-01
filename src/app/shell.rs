use super::*;
use eframe::egui::{Align, Layout, Margin, Pos2, Rect, Sense, UiBuilder};
use widgets::{Icon, pal, radius, strong_text};

const SETTINGS_WIDTH: f32 = 820.0;

impl MonitorApp {
    pub(super) fn draw(&mut self, root: &mut egui::Ui) -> egui::Rect {
        widgets::Palette::sync_scale(root.ctx());
        let p = pal(root);
        let rail = root.available_width() < widgets::NAV_RAIL_BELOW;
        egui::Panel::left("navigation")
            .resizable(false)
            .show_separator_line(false)
            .exact_size(if rail {
                widgets::NAV_RAIL_WIDTH
            } else {
                widgets::NAV_WIDTH
            })
            .frame(
                egui::Frame::new()
                    .fill(p.canvas)
                    .inner_margin(Margin::symmetric(12, 22)),
            )
            .show(root, |ui| self.sidebar(ui, rail));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(p.canvas).inner_margin(Margin {
                left: 12,
                right: widgets::PAGE_MARGIN,
                top: 24,
                bottom: 24,
            }))
            .show(root, |ui| {
                let area = ui.available_rect_before_wrap();
                let width = if self.page == Page::Settings {
                    area.width().min(SETTINGS_WIDTH)
                } else {
                    area.width()
                };
                let left = (area.left() + (area.width() - width) / 2.0).round();
                let page = Rect::from_min_size(
                    Pos2::new(left, area.top()),
                    Vec2::new(width, area.height()),
                );
                ui.scope_builder(UiBuilder::new().max_rect(page), |ui| {
                    self.page_header(ui);
                    self.banners(ui);
                    match self.page {
                        Page::Overview => self.overview(ui),
                        Page::Connections => self.connections(ui),
                        Page::Adapters | Page::Settings => {
                            egui::ScrollArea::vertical()
                                .id_salt(self.page.label())
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if self.page == Page::Adapters {
                                        self.adapters(ui);
                                    } else {
                                        self.settings(ui);
                                    }
                                });
                        }
                    }
                });
            })
            .response
            .rect
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, rail: bool) {
        let p = pal(ui);
        let brand = |ui: &mut egui::Ui| {
            let (logo, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
            ui.painter().rect_filled(logo, radius(10.0), p.accent_soft);
            for (x, h) in [(10.0, 8.0), (17.0, 17.0), (24.0, 12.0)] {
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        logo.min + Vec2::new(x - 2.0, 26.0 - h),
                        logo.min + Vec2::new(x + 2.0, 26.0),
                    ),
                    radius(1.5),
                    p.accent,
                );
            }
        };
        if rail {
            ui.vertical_centered(brand);
        } else {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                brand(ui);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(strong_text("OpenNetwork", 14.5));
                    ui.label(RichText::new("Monitor").size(13.0).weak());
                });
            });
        }
        ui.add_space(28.0);
        for page in [
            Page::Overview,
            Page::Connections,
            Page::Adapters,
            Page::Settings,
        ] {
            if widgets::navigation(ui, page.label(), page.icon(), self.page == page, rail).clicked()
            {
                self.page = page;
                self.selected_app = None;
            }
            ui.add_space(4.0);
        }
        let active = self.data.snapshot.etw_active;
        let tone = if active { p.good } else { p.warn };
        let status = if active {
            "Monitoring active"
        } else {
            "Capture unavailable"
        };
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if rail {
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.5, tone);
                response.on_hover_text(status);
                return;
            }
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!(
                    "{} adapters · {} connections",
                    self.data.snapshot.adapters.len(),
                    self.data.snapshot.connections.len()
                ))
                .size(12.5)
                .weak(),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                widgets::status_dot(ui, tone);
                ui.label(RichText::new(status).size(13.5).color(p.text));
            });
        });
    }

    pub(super) fn page_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 20.0;
            ui.set_min_height(38.0);
            ui.label(strong_text(self.page.label(), 28.0));
            if self.page == Page::Overview {
                self.source_toggle(ui);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button_with(
                        ui,
                        widgets::ButtonKind::Secondary,
                        Some(Icon::Export),
                        "Export CSV",
                        None,
                    )
                    .clicked()
                    {
                        self.export(ui.ctx());
                    }
                });
            }
        });
        let description = match self.page {
            Page::Overview => None,
            Page::Connections => Some("Live TCP connections and UDP sockets across your PC."),
            Page::Adapters => Some("Your network interfaces, connection status and live speeds."),
            Page::Settings => Some("Appearance, background monitoring and saved history."),
        };
        if let Some(description) = description {
            ui.add_space(2.0);
            ui.label(RichText::new(description).size(14.0).weak());
        }
        ui.add_space(16.0);
    }

    fn source_toggle(&mut self, ui: &mut egui::Ui) {
        let mut source = self.settings.filter.source;
        if widgets::segmented(
            ui,
            "source-toggle",
            &mut source,
            &[
                (Source::Adapter, "Overall Traffic"),
                (Source::Transport, "App traffic"),
            ],
        ) {
            self.switch_source(source);
        }
    }

    /// Status messages stacked under the header; capture warnings stay visible on every page.
    fn banners(&mut self, ui: &mut egui::Ui) {
        if !self.view_warning.is_empty() {
            widgets::message(ui, &self.view_warning, true, false);
        }
        if !self.data.notice.is_empty() && widgets::message(ui, &self.data.notice, false, true) {
            self.data.notice.clear();
            if let Ok(mut state) = self.worker.shared.lock() {
                state.notice.clear();
            }
        }
        for warning in &self.data.snapshot.warnings {
            widgets::message(ui, warning, true, false);
        }
        if !self.data.snapshot.etw_active && !self.data.snapshot.capture_status.is_empty() {
            widgets::message(ui, &self.data.snapshot.capture_status, true, false);
        }
        if self.data.snapshot.parse_errors
            + self.data.snapshot.queue_drops
            + self.data.snapshot.kernel_losses
            > 0
        {
            widgets::message(
                ui,
                "Some traffic events were lost. App totals may be incomplete; see Settings.",
                true,
                false,
            );
        }
    }
}
