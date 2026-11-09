//! Overview, Overall traffic tab: stat cards, the speed chart, top apps and destinations.
use super::*;
use crate::ui::{
    chart::AreaChart,
    controls::{self, BadgeVariant},
    layout::{card, card_fixed, card_header, menu_check, menu_label, tabs},
    tokens::Tokens,
};
use eframe::egui::{Color32, CornerRadius, PopupCloseBehavior};
use std::collections::HashMap;

const STAT_HEIGHT: f32 = 92.0;
/// Height of the row holding top apps and destinations. The speed chart takes whatever
/// the window leaves above it.
/// A row of the app list: path, name, text for the right edge and, for the top list, the
/// download and upload shares of the busiest app.
type AppRow = (String, String, String, Option<(f32, f32)>);
const BOTTOM_HEIGHT: f32 = 228.0;
const GAP: f32 = 12.0;

impl MonitorApp {
    pub(super) fn overview(&mut self, ui: &mut egui::Ui) {
        let apps = self.settings.filter.source == Source::Transport;
        self.filter_bar(ui, apps);
        // The tabs may have just changed the source.
        if self.settings.filter.source == Source::Transport {
            self.applications(ui);
        } else {
            self.overall_traffic(ui);
        }
    }

    fn overall_traffic(&mut self, ui: &mut egui::Ui) {
        self.stat_cards(ui);
        // The chart absorbs the window's height; the bottom row is always the same.
        let chart = (ui.available_height() - BOTTOM_HEIGHT - GAP - PAGE_BOTTOM).max(200.0);
        self.speed_chart_card(ui, chart);
        let width = ui.available_width();
        let left = ((width - GAP) * 2.0 / 3.0).floor();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            cell(ui, left, |ui| self.top_apps_card(ui));
            cell(ui, width - left - GAP, |ui| self.destinations_card(ui));
        });
    }

    fn stat_cards(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        let (live_down, live_up) = self.live_rates(Source::Adapter);
        let (peak_down, peak_up) = self.peak_rates();
        let peak = |value: Option<f64>| value.map_or_else(|| "—".to_owned(), |v| self.rate(v));
        let history = self.data.history.clone();
        let total = history.down.saturating_add(history.up);
        let mut stats = vec![
            Stat {
                icon: Icon::ArrowDown,
                label: "Download".into(),
                value: self.rate(live_down),
                note: format!("Peak {}", peak(peak_down)),
                tone: t.chart_down,
                meter: None,
            },
            Stat {
                icon: Icon::ArrowUp,
                label: "Upload".into(),
                value: self.rate(live_up),
                note: format!("Peak {}", peak(peak_up)),
                tone: t.chart_up,
                meter: None,
            },
            Stat {
                icon: Icon::Overview,
                label: "Total usage".into(),
                value: format_bytes(total as f64),
                note: format!("Over {}", self.period_phrase()),
                tone: t.muted_foreground,
                meter: None,
            },
        ];
        if let Some(cap) = self.data.cap {
            let share = cap.used as f64 / cap.limit.max(1) as f64;
            let tone = match crate::alerts::cap_level(cap.used, cap.limit) {
                crate::alerts::CapLevel::Below => t.muted_foreground,
                crate::alerts::CapLevel::Warning => t.warning,
                crate::alerts::CapLevel::Reached => t.destructive,
            };
            stats.push(Stat {
                icon: Icon::Alert,
                label: "Data cap".into(),
                value: format!("{:.0} %", share * 100.0),
                note: format!(
                    "{} of {} this month",
                    format_bytes(cap.used as f64),
                    format_bytes(cap.limit as f64)
                ),
                tone,
                meter: Some((share as f32, tone)),
            });
        } else {
            stats.push(Stat {
                icon: Icon::Connections,
                label: "Connections".into(),
                value: format!("{} open", self.data.snapshot.connections.len()),
                note: format!("{} adapters", self.data.snapshot.adapters.len()),
                tone: t.muted_foreground,
                meter: None,
            });
        }
        let width = ui.available_width();
        // Always four tiles in a row.
        let columns = 4;
        let gap = 12.0;
        let column = ((width - gap * (columns as f32 - 1.0)) / columns as f32).floor();
        for row in stats.chunks(columns) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for stat in row {
                    cell(ui, column, |ui| stat_card(ui, &t, stat));
                }
            });
        }
    }

    /// "the last hour", "the last 7 days", "all saved history"…
    fn period_phrase(&self) -> String {
        let filter = &self.settings.filter;
        if filter.time_range.is_some() {
            "your custom range".into()
        } else if filter.month_to_date() {
            "this month so far".into()
        } else if filter.all_time() {
            "all saved history".into()
        } else {
            let long = PERIODS
                .iter()
                .find(|p| p.0 == filter.seconds)
                .map_or("the period", |p| p.2);
            format!("the last {}", long.to_lowercase())
        }
    }

    fn speed_chart_card(&mut self, ui: &mut egui::Ui, height: f32) {
        let t = tk(ui.ctx());
        card_fixed(ui, 20, height, |ui| {
            let description = format!("Download and upload speed over {}", self.period_phrase());
            card_header(ui, "Network speed", &description, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let more = Button::icon_only(Icon::More, "Chart options")
                    .variant(Variant::Outline)
                    .show(ui);
                egui::Popup::menu(&more)
                    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                    .gap(4.0)
                    .show(|ui| {
                        ui.set_width(240.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        menu_label(ui, "Chart");
                        menu_check(ui, &mut self.log, "Logarithmic scale");
                        let mut frozen = self.paused;
                        if menu_check(ui, &mut frozen, "Freeze chart") {
                            self.paused = frozen;
                            self.frozen = frozen.then(|| FrozenChart {
                                points: self.chart_points(),
                                window: self.chart_window(),
                            });
                        }
                    });
                if self.paused {
                    controls::badge(ui, BadgeVariant::Secondary, "Frozen");
                }
                legend_toggle(ui, "Upload", t.chart_up, &mut self.show_up);
                legend_toggle(ui, "Download", t.chart_down, &mut self.show_down);
            });
            ui.add_space(8.0);
            let points = self.chart_points();
            let window = self.chart_window();
            AreaChart {
                points: &points,
                window,
                height: ui.available_height().max(120.0),
                show_down: self.show_down,
                show_up: self.show_up,
                bits: self.settings.bits,
                log_scale: self.log,
            }
            .show(ui);
        });
    }

    fn top_apps_card(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        card_fixed(ui, 16, BOTTOM_HEIGHT, |ui| {
            let mut view = self.top_apps_view;
            if tabs(
                ui,
                "top-apps-view",
                &mut view,
                &[
                    (TopAppsView::Top, "Top apps"),
                    (TopAppsView::Recent, "Recently used"),
                ],
            ) {
                self.top_apps_view = view;
            }
            ui.add_space(6.0);
            // Rows: name, what to show at the right, and an optional (download, upload) share of the busiest.
            let rows: Vec<AppRow> = match view {
                TopAppsView::Top => {
                    let peak = self
                        .data
                        .top_apps
                        .iter()
                        .map(|a| a.down.saturating_add(a.up))
                        .max()
                        .unwrap_or(0);
                    self.data
                        .top_apps
                        .iter()
                        .map(|app| {
                            (
                                app.app.clone(),
                                self.label(&app.app),
                                // Down and up usage, split at the tab when painted.
                                format!(
                                    "{}\t{}",
                                    format_bytes(app.down as f64),
                                    format_bytes(app.up as f64)
                                ),
                                Some(if peak == 0 {
                                    (0.0, 0.0)
                                } else {
                                    (
                                        (app.down as f64 / peak as f64) as f32,
                                        (app.up as f64 / peak as f64) as f32,
                                    )
                                }),
                            )
                        })
                        .collect()
                }
                TopAppsView::Recent => {
                    let now = chrono::Utc::now().timestamp();
                    self.recently_used(now)
                        .into_iter()
                        .map(|(path, last)| (path.clone(), self.label(&path), ago(now, last), None))
                        .collect()
                }
            };
            egui::ScrollArea::vertical()
                .id_salt(("top-apps", view == TopAppsView::Recent))
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    for (path, name, value, share) in &rows {
                        let texture = self.icons.texture(ui.ctx(), path);
                        let height = 30.0;
                        let (row, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), height),
                            Sense::hover(),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Label,
                                true,
                                match value.split_once('\t') {
                                    Some((down, up)) => format!("{name}: {down} down, {up} up"),
                                    None => format!("{name}: {value}"),
                                },
                            )
                        });
                        let painter = ui.painter();
                        let avatar_y = if share.is_some() {
                            row.top() + 10.0
                        } else {
                            row.center().y
                        };
                        controls::avatar(
                            painter,
                            &t,
                            Rect::from_center_size(
                                Pos2::new(row.left() + 11.0, avatar_y),
                                Vec2::splat(20.0),
                            ),
                            name,
                            texture,
                        );
                        let value_width = value
                            .split('\t')
                            .map(|part| text::text_width(painter, part, text::medium(13.5)) + 26.0)
                            .sum::<f32>();
                        let line = if share.is_some() { 20.0 } else { height };
                        text::paint(
                            painter,
                            Rect::from_min_max(
                                Pos2::new(row.left() + 30.0, row.top()),
                                Pos2::new(row.right() - value_width - 12.0, row.top() + line),
                            ),
                            Align2::LEFT_CENTER,
                            name,
                            text::medium(13.5),
                            t.foreground,
                        );
                        let value_rect =
                            Rect::from_min_max(row.min, Pos2::new(row.right(), row.top() + line));
                        if let Some((down, up)) = value.split_once('\t') {
                            let font = text::medium(13.5);
                            let mut right = row.right();
                            for (icon, tone, amount) in [
                                (Icon::ArrowUp, t.chart_up, up),
                                (Icon::ArrowDown, t.chart_down, down),
                            ] {
                                let width = text::text_width(painter, amount, font.clone());
                                text::paint(
                                    painter,
                                    Rect::from_min_max(
                                        Pos2::new(right - width - 2.0, value_rect.top()),
                                        Pos2::new(right, value_rect.bottom()),
                                    ),
                                    Align2::RIGHT_CENTER,
                                    amount,
                                    font.clone(),
                                    t.foreground,
                                );
                                icon::paint(
                                    painter,
                                    icon,
                                    Pos2::new(right - width - 10.0, value_rect.center().y),
                                    12.0,
                                    tone,
                                );
                                right -= width + 26.0;
                            }
                        } else {
                            text::paint(
                                painter,
                                value_rect,
                                Align2::RIGHT_CENTER,
                                value,
                                text::regular(13.0),
                                t.muted_foreground,
                            );
                        }
                        if let Some(share) = share {
                            let bar = Rect::from_min_max(
                                Pos2::new(row.left() + 30.0, row.bottom() - 6.0),
                                Pos2::new(row.right(), row.bottom() - 3.0),
                            );
                            painter.rect_filled(bar, CornerRadius::same(2), t.muted);
                            let down = bar.width() * share.0;
                            // A sliver of upload stays visible however small it is.
                            let up = if share.1 > 0.0 {
                                (bar.width() * share.1).max(2.0)
                            } else {
                                0.0
                            };
                            for (from, width, tone) in
                                [(0.0, down, t.chart_down), (down, up, t.chart_up)]
                            {
                                painter.rect_filled(
                                    Rect::from_min_size(
                                        Pos2::new(bar.left() + from, bar.top()),
                                        Vec2::new(width.min(bar.width() - from), bar.height()),
                                    ),
                                    CornerRadius::same(2),
                                    tone,
                                );
                            }
                        }
                    }
                    if rows.is_empty() {
                        let loading = view == TopAppsView::Top
                            && self.data.history_revision != self.filter_revision;
                        layout::empty_state(
                            ui,
                            if loading {
                                "Loading usage…"
                            } else if view == TopAppsView::Recent {
                                "No app has used the network yet."
                            } else {
                                "No app traffic recorded in this period."
                            },
                        );
                    }
                });
        });
    }

    /// Apps by when they last used the network, newest first: the saved history, plus any
    /// app moving bytes right now.
    fn recently_used(&self, now: i64) -> Vec<(String, i64)> {
        let mut last: HashMap<String, i64> = self
            .data
            .recent_apps
            .iter()
            .map(|r| (r.app.clone(), r.last_seen))
            .collect();
        for row in &self.data.snapshot.live_rows {
            if row.source == "transport"
                && !row.app.is_empty()
                && row.down.saturating_add(row.up) > 0
            {
                last.insert(row.app.clone(), now);
            }
        }
        let mut apps: Vec<_> = last.into_iter().collect();
        apps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        apps.truncate(30);
        apps
    }

    fn destinations_card(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        let totals = self.data.classified.clone();
        let sum = totals
            .lan
            .saturating_add(totals.wan)
            .saturating_add(totals.unknown);
        card_fixed(ui, 20, BOTTOM_HEIGHT, |ui| {
            card_header(
                ui,
                "Destinations",
                &format!("{} of captured app traffic", format_bytes(sum as f64)),
                |_| {},
            );
            ui.add_space(12.0);
            let parts = [
                ("WAN", "Public internet", totals.wan, t.primary),
                (
                    "LAN",
                    "Local network",
                    totals.lan,
                    t.muted_foreground.gamma_multiply(0.7),
                ),
                (
                    "Other",
                    "Shared or reserved ranges",
                    totals.unknown,
                    t.border,
                ),
            ];
            let (bar, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    "Share of captured traffic by destination",
                )
            });
            ui.painter()
                .rect_filled(bar, CornerRadius::same(5), t.muted);
            let mut x = bar.left();
            for (_, _, bytes, color) in &parts {
                let width = if sum == 0 {
                    0.0
                } else {
                    bar.width() * (*bytes as f64 / sum as f64) as f32
                };
                if width > 0.5 {
                    ui.painter().rect_filled(
                        Rect::from_min_size(
                            Pos2::new(x, bar.top()),
                            Vec2::new(width, bar.height()),
                        ),
                        CornerRadius::same(if width > 10.0 { 5 } else { 0 }),
                        *color,
                    );
                }
                x += width;
            }
            ui.add_space(8.0);
            for (name, hint, bytes, color) in parts {
                let (row, response) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
                let percent = if sum == 0 {
                    0.0
                } else {
                    bytes as f64 / sum as f64 * 100.0
                };
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        true,
                        format!("{name}: {} ({percent:.1}%)", format_bytes(bytes as f64)),
                    )
                });
                let painter = ui.painter();
                painter.rect_filled(
                    Rect::from_center_size(
                        Pos2::new(row.left() + 5.0, row.center().y),
                        Vec2::splat(10.0),
                    ),
                    CornerRadius::same(3),
                    color,
                );
                text::paint(
                    painter,
                    Rect::from_min_max(Pos2::new(row.left() + 20.0, row.top() + 5.0), row.max),
                    Align2::LEFT_TOP,
                    name,
                    text::medium(14.0),
                    t.foreground,
                );
                text::paint(
                    painter,
                    Rect::from_min_max(Pos2::new(row.left() + 20.0, row.top() + 21.0), row.max),
                    Align2::LEFT_TOP,
                    hint,
                    text::regular(12.0),
                    t.muted_foreground,
                );
                text::paint(
                    painter,
                    Rect::from_min_max(row.min, Pos2::new(row.right(), row.top() + 27.0)),
                    Align2::RIGHT_CENTER,
                    &format_bytes(bytes as f64),
                    text::medium(14.0),
                    t.foreground,
                );
                text::paint(
                    painter,
                    Rect::from_min_max(Pos2::new(row.left(), row.top() + 21.0), row.max),
                    Align2::RIGHT_TOP,
                    &if percent < 0.1 && bytes > 0 {
                        "<0.1 %".to_owned()
                    } else {
                        format!("{percent:.1} %")
                    },
                    text::regular(12.0),
                    t.muted_foreground,
                );
            }
        });
    }
}

struct Stat {
    icon: Icon,
    label: String,
    value: String,
    note: String,
    tone: Color32,
    meter: Option<(f32, Color32)>,
}

fn stat_card(ui: &mut egui::Ui, t: &Tokens, stat: &Stat) {
    card(ui, 12, |ui| {
        ui.set_min_height(STAT_HEIGHT - 26.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let (head, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Label,
                true,
                format!("{}: {}. {}", stat.label, stat.value, stat.note),
            )
        });
        text::paint(
            ui.painter(),
            head,
            Align2::LEFT_CENTER,
            &stat.label,
            text::medium(13.5),
            t.muted_foreground,
        );
        icon::paint(
            ui.painter(),
            stat.icon,
            Pos2::new(head.right() - 8.0, head.center().y),
            15.0,
            stat.tone,
        );
        let galley = text::number_with_unit(
            ui.painter(),
            &stat.value,
            text::semibold(24.0),
            text::regular(13.0),
            t.foreground,
            t.muted_foreground,
        );
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), galley.size().y),
            Sense::hover(),
        );
        ui.painter().galley(rect.min, galley, t.foreground);
        let (note, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::hover());
        text::paint(
            ui.painter(),
            note,
            Align2::LEFT_CENTER,
            &stat.note,
            text::regular(12.0),
            t.muted_foreground,
        );
        if let Some((share, tone)) = stat.meter {
            ui.add_space(2.0);
            controls::progress(ui, share, Some(tone));
        }
    });
}

/// A chart legend entry that also shows or hides its series.
fn legend_toggle(ui: &mut egui::Ui, label: &str, color: Color32, on: &mut bool) {
    let t = tk(ui.ctx());
    let width = text::text_width(ui.painter(), label, text::medium(13.5)) + 40.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, *on, label));
    if response.clicked() {
        *on = !*on;
    }
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(8),
        if response.hovered() {
            t.accent
        } else {
            t.background
        },
    );
    painter.rect_stroke(
        rect,
        CornerRadius::same(8),
        crate::ui::tokens::hairline(ui.ctx(), t.input),
        egui::StrokeKind::Inside,
    );
    if response.has_focus() {
        crate::ui::focus_ring(painter, ui.ctx(), rect, CornerRadius::same(8), &t);
    }
    let dot = Pos2::new(rect.left() + 16.0, rect.center().y);
    if *on {
        painter.circle_filled(dot, 4.5, color);
    } else {
        painter.circle_stroke(dot, 4.0, egui::Stroke::new(1.5, t.muted_foreground));
    }
    text::paint(
        painter,
        Rect::from_min_max(Pos2::new(rect.left() + 28.0, rect.top()), rect.max),
        Align2::LEFT_CENTER,
        label,
        text::medium(13.5),
        if *on {
            t.foreground
        } else {
            t.muted_foreground
        },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// How long ago an app last used the network, in words.
pub(super) fn ago(now: i64, last: i64) -> String {
    let seconds = (now - last).max(0);
    match seconds {
        0..120 => "Just now".to_owned(),
        120..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        86_400..604_800 => format!("{} d ago", seconds / 86_400),
        _ => chrono::DateTime::from_timestamp(last, 0)
            .map(|time| {
                time.with_timezone(&chrono::Local)
                    .format("%d %b %Y")
                    .to_string()
            })
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_use_reads_in_the_largest_sensible_unit() {
        let now = 2_000_000_000;
        assert_eq!(ago(now, now), "Just now");
        assert_eq!(ago(now, now - 119), "Just now");
        assert_eq!(ago(now, now - 300), "5 min ago");
        assert_eq!(ago(now, now - 7_300), "2 h ago");
        assert_eq!(ago(now, now - 3 * 86_400), "3 d ago");
        assert!(ago(now, now - 40 * 86_400).contains("20"));
        assert_eq!(ago(now, now + 50), "Just now");
    }
}
