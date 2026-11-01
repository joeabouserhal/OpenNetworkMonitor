use super::*;
use eframe::egui::{Align, Align2, Layout, PopupCloseBehavior, Pos2, Rect, Sense};
use widgets::{Col, Icon, bold, paint_text, paint_value, pal, sans, thin};

const SIDE_WIDTH: f32 = 316.0;

impl MonitorApp {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        // These pages budget their height explicitly, so widgets must not add their own gaps.
        ui.spacing_mut().item_spacing.y = 0.0;
        if self.settings.filter.source == Source::Transport {
            self.applications(ui);
        } else {
            self.adapter_overview(ui);
        }
    }
    pub(super) fn adapter_overview(&mut self, ui: &mut egui::Ui) {
        self.toolbar(ui, false);
        widgets::columns(ui, [Col::Flex(1.0), Col::Fixed(SIDE_WIDTH)], 16.0, |cols| {
            let [chart, side] = cols;
            self.chart_card(chart);
            self.summary_panel(side);
        });
    }

    fn chart_card(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let (live_down, live_up) = self.live_rates(Source::Adapter);
        let (down_text, up_text) = (self.rate(live_down), self.rate(live_up));
        let frame = widgets::surface(ui);
        widgets::surface_fill(ui, frame, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 30.0;
                series_toggle(
                    ui,
                    "legend-down",
                    "Download",
                    &down_text,
                    p.down,
                    &mut self.show_down,
                );
                series_toggle(ui, "legend-up", "Upload", &up_text, p.up, &mut self.show_up);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let more = widgets::icon_button(ui, Icon::More, "Chart options");
                    egui::Popup::menu(&more)
                        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                        .gap(6.0)
                        .show(|ui| {
                            ui.set_width(236.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            widgets::check_item(ui, &mut self.log, "Logarithmic scale");
                            let mut frozen = self.paused;
                            if widgets::check_item(ui, &mut frozen, "Freeze chart") {
                                self.paused = frozen;
                                self.frozen = frozen.then(|| FrozenChart {
                                    points: self.chart_points(),
                                    window: self.chart_window(),
                                });
                            }
                            widgets::menu_note(ui, "Freezing the chart keeps monitoring active.");
                        });
                    if self.paused {
                        widgets::badge(ui, "Paused", p.warn);
                    }
                });
            });
            ui.add_space(6.0);
            let points = self.chart_points();
            let window = self.chart_window();
            let height = ui.available_height();
            self.graph.show(
                ui,
                &points,
                window,
                height,
                self.show_down,
                self.show_up,
                self.settings.bits,
                self.log,
            );
        });
    }

    /// Period totals and the busiest apps, as cards that keep their natural height.
    pub(super) fn summary_panel(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = 0.0;
        self.totals_card(ui);
        ui.add_space(12.0);
        self.top_apps_card(ui);
    }

    fn totals_card(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        widgets::surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 60.0), Sense::hover());
            let half = ((width - 16.0) / 2.0).floor();
            for (i, label, value, color) in [
                (0.0, "Downloaded", self.data.history.down, p.down),
                (1.0, "Uploaded", self.data.history.up, p.up),
            ] {
                let cell = Rect::from_min_size(
                    rect.min + Vec2::new(i * (half + 16.0), 0.0),
                    Vec2::new(half, rect.height()),
                );
                ui.painter().circle_filled(
                    Pos2::new(cell.left() + 4.0, cell.top() + 9.0),
                    3.5,
                    color,
                );
                paint_text(
                    ui.painter(),
                    Rect::from_min_size(
                        cell.min + Vec2::new(14.0, 0.0),
                        Vec2::new(half - 14.0, 18.0),
                    ),
                    Align2::LEFT_CENTER,
                    label,
                    sans(13.0),
                    p.dim,
                );
                paint_value(
                    ui.painter(),
                    Rect::from_min_size(cell.min + Vec2::new(0.0, 22.0), Vec2::new(half, 38.0)),
                    Align2::LEFT_CENTER,
                    &compact_bytes(value),
                    (thin(26.0), sans(14.0)),
                    p.text,
                    p.dim,
                );
            }
            ui.add_space(16.0);
            widgets::divider(ui);
            ui.add_space(16.0);
            crate::graph::destination_breakdown(ui, &self.data.classified);
        });
    }

    fn top_apps_card(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        widgets::surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = ui.available_width();
            let (head, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
            paint_text(
                ui.painter(),
                head,
                Align2::LEFT_CENTER,
                "Top 5 apps",
                bold(14.5),
                p.text,
            );
            paint_text(
                ui.painter(),
                head,
                Align2::RIGHT_CENTER,
                "Total",
                sans(12.5),
                p.dim,
            );
            ui.add_space(8.0);
            let busiest = self
                .data
                .top_apps
                .first()
                .map_or(0, |a| a.down.saturating_add(a.up));
            for app in self.data.top_apps.iter().take(5) {
                let name = self.label(&app.app);
                let (row, _) = ui.allocate_exact_size(Vec2::new(width, 50.0), Sense::hover());
                let icon = self.icons.texture(ui.ctx(), &app.app);
                let painter = ui.painter();
                let tile =
                    Rect::from_min_size(Pos2::new(row.left(), row.top() + 7.0), Vec2::splat(30.0));
                apps::app_tile(painter, tile, &name, icon, &p);
                let total_bytes = app.down.saturating_add(app.up);
                let total = compact_bytes(total_bytes);
                let total_width = widgets::text_width(painter, &total, bold(14.0)) + 2.0;
                let text_left = row.left() + 42.0;
                paint_text(
                    painter,
                    Rect::from_min_max(
                        Pos2::new(text_left, row.top() + 6.0),
                        Pos2::new(row.right() - total_width - 8.0, row.top() + 26.0),
                    ),
                    Align2::LEFT_CENTER,
                    &name,
                    sans(14.0),
                    p.text,
                );
                paint_text(
                    painter,
                    Rect::from_min_max(
                        Pos2::new(row.right() - total_width, row.top() + 6.0),
                        Pos2::new(row.right(), row.top() + 26.0),
                    ),
                    Align2::RIGHT_CENTER,
                    &total,
                    bold(14.0),
                    p.text,
                );
                let track = Rect::from_min_max(
                    Pos2::new(text_left, row.top() + 33.0),
                    Pos2::new(row.right(), row.top() + 37.0),
                );
                painter.rect_filled(track, widgets::radius(2.0), p.text.gamma_multiply(0.07));
                if busiest > 0 && total_bytes > 0 {
                    let share = (total_bytes as f64 / busiest as f64) as f32;
                    let down_part = (app.down as f64 / total_bytes as f64) as f32;
                    widgets::split_bar(
                        painter,
                        Rect::from_min_size(
                            track.min,
                            Vec2::new((track.width() * share).max(4.0), track.height()),
                        ),
                        &[(down_part, p.down), (1.0 - down_part, p.up)],
                        Color32::TRANSPARENT,
                    );
                }
                ui.interact(
                    row,
                    ui.make_persistent_id(("summary-app", &app.app)),
                    Sense::hover(),
                )
                .on_hover_text(format!(
                    "{name}
{}
Downloaded: {}
Uploaded: {}
Captured TCP/UDP app traffic in the selected period and adapters.",
                    app.app,
                    format_bytes(app.down as f64),
                    format_bytes(app.up as f64)
                ));
            }
            if self.data.top_apps.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("No app traffic recorded").size(13.5).weak());
            }
        });
    }
}

/// Legend entry that also shows the live speed and toggles its series.
fn series_toggle(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    value: &str,
    color: Color32,
    on: &mut bool,
) {
    let p = pal(ui);
    let fonts = (thin(28.0), sans(14.0));
    let galley = widgets::value_galley(
        ui.painter(),
        value,
        fonts.0.clone(),
        fonts.1.clone(),
        p.text,
        p.dim,
    );
    let label_width = widgets::text_width(ui.painter(), label, sans(13.5)) + 16.0;
    let size = Vec2::new(galley.size().x.max(label_width) + 4.0, 58.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let response = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *on, label)
    });
    if response.clicked() {
        *on = !*on;
    }
    let fade = if *on { 1.0 } else { 0.4 };
    let painter = ui.painter();
    let dot = Pos2::new(rect.left() + 4.0, rect.top() + 9.0);
    if *on {
        painter.circle_filled(dot, 4.0, color);
    } else {
        painter.circle_stroke(dot, 3.5, egui::Stroke::new(1.5, color));
    }
    paint_text(
        painter,
        Rect::from_min_size(
            rect.min + Vec2::new(16.0, 0.0),
            Vec2::new(rect.width() - 16.0, 18.0),
        ),
        Align2::LEFT_CENTER,
        label,
        sans(13.5),
        p.dim,
    );
    paint_value(
        painter,
        Rect::from_min_size(
            rect.min + Vec2::new(0.0, 20.0),
            Vec2::new(rect.width(), 38.0),
        ),
        Align2::LEFT_CENTER,
        value,
        fonts,
        p.text.gamma_multiply(fade),
        p.dim.gamma_multiply(fade),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}
