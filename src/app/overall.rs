use super::*;
use eframe::egui::{Align, Align2, Layout, PopupCloseBehavior, Pos2, Rect, Sense};
use widgets::{Col, Icon, bold, paint_text, paint_value, pal, sans, thin};

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
    /// Fixed height of the sections inside the summary card, so the chart can take the rest.
    pub(super) const SUMMARY_CONTENT_HEIGHT: f32 = 138.0;

    /// Space above a section whose content is `content` tall so it sits centred in its cell.
    pub(super) fn centred(content: f32) -> f32 {
        ((Self::SUMMARY_CONTENT_HEIGHT - content) / 2.0)
            .max(0.0)
            .floor()
    }
    const SUMMARY_GAP: f32 = 12.0;

    pub(super) fn adapter_overview(&mut self, ui: &mut egui::Ui) {
        self.toolbar(ui, false);
        let summary = widgets::surface(ui).total_margin().sum().y + Self::SUMMARY_CONTENT_HEIGHT;
        let chart_height = (ui.available_height() - summary - Self::SUMMARY_GAP).max(160.0);
        ui.allocate_ui(Vec2::new(ui.available_width(), chart_height), |ui| {
            self.chart_card(ui)
        });
        ui.add_space(Self::SUMMARY_GAP);
        self.summary_card(ui);
    }

    /// Highest download and upload speeds among the chart's observed samples, or None
    /// when the period has none. Frozen charts report the frozen samples.
    pub(super) fn peak_rates(&self) -> (Option<f64>, Option<f64>) {
        let (start, end) = self.chart_window();
        let points = self
            .frozen
            .as_ref()
            .map_or(&self.data.history.points, |frozen| &frozen.points);
        let mut peaks = (None::<u64>, None::<u64>);
        for point in points
            .iter()
            .filter(|p| p.observed && (start..=end).contains(&p.timestamp))
        {
            peaks.0 = Some(peaks.0.map_or(point.down, |peak| peak.max(point.down)));
            peaks.1 = Some(peaks.1.map_or(point.up, |peak| peak.max(point.up)));
        }
        (peaks.0.map(|v| v as f64), peaks.1.map(|v| v as f64))
    }

    fn chart_card(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let (live_down, live_up) = self.live_rates(Source::Adapter);
        let (down_text, up_text) = (self.rate(live_down), self.rate(live_up));
        let (peak_down, peak_up) = self.peak_rates();
        let peak_text = |peak: Option<f64>| peak.map_or_else(|| "—".to_owned(), |v| self.rate(v));
        let (peak_down_text, peak_up_text) = (peak_text(peak_down), peak_text(peak_up));
        let frame = widgets::surface(ui);
        widgets::surface_fill(ui, frame, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 30.0;
                series_toggle(
                    ui,
                    "legend-down",
                    "Download",
                    (&down_text, &peak_down_text),
                    p.down,
                    &mut self.show_down,
                );
                series_toggle(
                    ui,
                    "legend-up",
                    "Upload",
                    (&up_text, &peak_up_text),
                    p.up,
                    &mut self.show_up,
                );
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

    /// Total usage, destinations and the busiest apps in one card below the chart.
    pub(super) fn summary_card(&mut self, ui: &mut egui::Ui) {
        widgets::surface(ui).show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), Self::SUMMARY_CONTENT_HEIGHT),
                Sense::hover(),
            );
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            widgets::columns(
                &mut body,
                [Col::Flex(0.27), Col::Flex(0.33), Col::Flex(0.40)],
                40.0,
                |cols| {
                    let [totals, destinations, apps] = cols;
                    self.totals_section(totals);
                    destinations.add_space(Self::centred(crate::graph::DESTINATION_HEIGHT));
                    crate::graph::destination_breakdown(destinations, &self.data.classified);
                    self.top_apps_section(apps);
                    for (left, right) in [(&*totals, &*destinations), (&*destinations, &*apps)] {
                        let x = (left.max_rect().right() + right.max_rect().left()) / 2.0;
                        totals.painter().vline(
                            x,
                            rect.top()..=rect.bottom(),
                            widgets::hairline(totals),
                        );
                    }
                },
            );
        });
    }

    fn totals_section(&self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let width = ui.available_width();
        // Narrow sections stack the two totals instead of squeezing them.
        let side_by_side = width >= 240.0;
        ui.add_space(Self::centred(
            22.0 + 8.0 + if side_by_side { 54.0 } else { 108.0 },
        ));
        let (head, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
        paint_text(
            ui.painter(),
            head,
            Align2::LEFT_CENTER,
            "Total usage",
            bold(14.5),
            p.text,
        );
        ui.add_space(8.0);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(width, if side_by_side { 54.0 } else { 108.0 }),
            Sense::hover(),
        );
        let half = ((width - 16.0) / 2.0).floor();
        for (i, label, value, color) in [
            (0.0, "Downloaded", self.data.history.down, p.down),
            (1.0, "Uploaded", self.data.history.up, p.up),
        ] {
            let cell = if side_by_side {
                Rect::from_min_size(
                    rect.min + Vec2::new(i * (half + 16.0), 0.0),
                    Vec2::new(half, 54.0),
                )
            } else {
                Rect::from_min_size(rect.min + Vec2::new(0.0, i * 54.0), Vec2::new(width, 54.0))
            };
            ui.painter()
                .circle_filled(Pos2::new(cell.left() + 4.0, cell.top() + 9.0), 3.5, color);
            paint_text(
                ui.painter(),
                Rect::from_min_size(
                    cell.min + Vec2::new(14.0, 0.0),
                    Vec2::new(cell.width() - 14.0, 18.0),
                ),
                Align2::LEFT_CENTER,
                label,
                sans(13.0),
                p.dim,
            );
            paint_value(
                ui.painter(),
                Rect::from_min_size(
                    cell.min + Vec2::new(0.0, 20.0),
                    Vec2::new(cell.width(), if side_by_side { 34.0 } else { 30.0 }),
                ),
                Align2::LEFT_CENTER,
                &compact_bytes(value),
                (thin(if side_by_side { 26.0 } else { 22.0 }), sans(14.0)),
                p.text,
                p.dim,
            );
        }
    }

    fn top_apps_section(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        let width = ui.available_width();
        let rows = self.data.top_apps.len().min(5);
        ui.add_space(Self::centred(if rows == 0 {
            22.0 + 6.0 + 4.0 + 18.0
        } else {
            22.0 + 6.0 + 22.0 * rows as f32
        }));
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
        ui.add_space(6.0);
        for app in self.data.top_apps.iter().take(5) {
            let name = self.label(&app.app);
            let (row, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
            let icon = self.icons.texture(ui.ctx(), &app.app);
            let painter = ui.painter();
            let tile = Rect::from_center_size(
                Pos2::new(row.left() + 9.0, row.center().y),
                Vec2::splat(18.0),
            );
            apps::app_tile(painter, tile, &name, icon, &p);
            let total = compact_bytes(app.down.saturating_add(app.up));
            let total_width = widgets::text_width(painter, &total, bold(13.5)) + 2.0;
            paint_text(
                painter,
                Rect::from_min_max(
                    Pos2::new(row.left() + 28.0, row.top()),
                    Pos2::new(row.right() - total_width - 8.0, row.bottom()),
                ),
                Align2::LEFT_CENTER,
                &name,
                sans(13.5),
                p.text,
            );
            paint_text(
                painter,
                Rect::from_min_max(Pos2::new(row.right() - total_width, row.top()), row.max),
                Align2::RIGHT_CENTER,
                &total,
                bold(13.5),
                p.text,
            );
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
            let loading = self.data.history_revision != self.filter_revision;
            ui.label(
                RichText::new(if loading {
                    "Loading usage…"
                } else {
                    "No app traffic recorded"
                })
                .size(13.5)
                .weak(),
            );
        }
    }
}

/// Legend entry that shows the live speed and the period's peak, and toggles its series.
fn series_toggle(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    (value, peak): (&str, &str),
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
    let peak_width = widgets::text_width(ui.painter(), peak, sans(12.5)).max(widgets::text_width(
        ui.painter(),
        "Peak",
        sans(11.5),
    )) + 2.0;
    let gap = 26.0;
    let content = galley.size().x + gap + peak_width;
    let size = Vec2::new(content.max(label_width) + 4.0, 58.0);
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
        painter.circle_stroke(dot, 3.5, widgets::snapped(ui.ctx(), 1.5, color));
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
            Vec2::new(galley.size().x + 2.0, 38.0),
        ),
        Align2::LEFT_CENTER,
        value,
        fonts,
        p.text.gamma_multiply(fade),
        p.dim.gamma_multiply(fade),
    );
    // The peak sits beside the live number, centred on it. It is deliberately quiet: small,
    // regular weight and the secondary grey, so the live speed stays the focus.
    let left = rect.left() + galley.size().x + gap;
    painter.vline(
        left - gap / 2.0,
        rect.top() + 29.0..=rect.top() + 51.0,
        widgets::px_stroke(ui.ctx(), 1.0, p.hairline),
    );
    paint_text(
        painter,
        Rect::from_min_size(
            Pos2::new(left, rect.top() + 26.0),
            Vec2::new(peak_width, 13.0),
        ),
        Align2::LEFT_CENTER,
        "Peak",
        sans(11.5),
        p.dim.gamma_multiply(fade),
    );
    paint_text(
        painter,
        Rect::from_min_size(
            Pos2::new(left, rect.top() + 40.0),
            Vec2::new(peak_width, 15.0),
        ),
        Align2::LEFT_CENTER,
        peak,
        sans(12.5),
        p.dim.gamma_multiply(fade),
    );
    response
        .on_hover_text("Peak is the highest speed in the selected period. Click to show or hide this direction.")
        .on_hover_cursor(egui::CursorIcon::PointingHand);
}
