use super::*;
use eframe::egui::{Align, Layout, Margin, Pos2, Rect, Sense, UiBuilder};
use widgets::{Icon, pal, radius, strong_text};

const SETTINGS_WIDTH: f32 = 820.0;
/// How long an information message stays before it goes away on its own.
const NOTICE_LIFETIME: Duration = Duration::from_secs(15);

/// Messages that report damage are kept until the user dismisses them.
fn notice_is_sticky(text: &str) -> bool {
    text.contains("integrity check")
}

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
                    if self.page == Page::Overview && self.settings.intro_seen == Some(false) {
                        self.intro_card(ui);
                    }
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
        for (index, page) in Page::ALL.into_iter().enumerate() {
            if widgets::navigation(ui, page.label(), page.icon(), self.page == page, rail)
                .on_hover_text(format!("{} (Ctrl+{})", page.label(), index + 1))
                .clicked()
            {
                self.page = page;
            }
            ui.add_space(4.0);
        }
        let active = self.data.snapshot.etw_active;
        let paused = self.data.snapshot.recording_paused;
        let tone = if active && !paused { p.good } else { p.warn };
        let status = if paused {
            "Recording paused"
        } else if active {
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
            if let Some(cap) = self.data.cap {
                let share = cap.used as f64 / cap.limit.max(1) as f64 * 100.0;
                ui.label(
                    RichText::new(format!("Data cap {share:.0}% used"))
                        .size(12.5)
                        .color(match crate::alerts::cap_level(cap.used, cap.limit) {
                            crate::alerts::CapLevel::Below => p.dim,
                            crate::alerts::CapLevel::Warning => p.warn,
                            crate::alerts::CapLevel::Reached => p.bad,
                        }),
                );
            }
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

    /// A short welcome for a first launch, shown until it is dismissed.
    fn intro_card(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let mut dismissed = false;
        let mut open_settings = false;
        widgets::surface(ui)
            .fill(p.accent_soft)
            .inner_margin(Margin::symmetric(20, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.label(strong_text("Welcome to OpenNetworkMonitor", 16.0));
                for line in [
                    "Overall Traffic is everything each network adapter carries. App traffic shows which apps and hosts used it, and needs administrator rights.",
                    "Right-click a connection for actions, and use Ctrl+1 to Ctrl+4 to switch pages.",
                    "Settings has data-cap and new-app alerts, and an optional country database for IP lookups.",
                ] {
                    ui.label(RichText::new(line).size(13.5).color(p.dim));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    dismissed = widgets::button(ui, widgets::ButtonKind::Primary, "Got it").clicked();
                    open_settings =
                        widgets::button(ui, widgets::ButtonKind::Secondary, "Open Settings")
                            .clicked();
                });
            });
        ui.add_space(10.0);
        if dismissed || open_settings {
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

    /// Status messages stacked under the header; capture warnings stay visible on every page.
    fn banners(&mut self, ui: &mut egui::Ui) {
        if !self.view_warning.is_empty() {
            widgets::message(ui, &self.view_warning, true, false);
        }
        if !self.data.notice.is_empty()
            && (widgets::message(ui, &self.data.notice, false, true)
                | self.notice_expired(ui.ctx()))
        {
            self.clear_notice();
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

#[cfg(test)]
mod notice_tests {
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
