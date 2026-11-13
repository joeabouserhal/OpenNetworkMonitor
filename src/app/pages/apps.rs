//! Overview, App traffic tab: the selected app's timeline and ranked app, host and
//! traffic-type tables.
use super::*;
use crate::{
    traffic::RankedUsage,
    ui::{
        chart::{self, BarValue},
        layout::{
            Col, TABLE_HEADER_HEIGHT, card_fixed, card_header, cells, header_cell, row_border,
            row_hover, tabs,
        },
    },
};
use eframe::egui::{CornerRadius, TextureId};

const ROW_HEIGHT: f32 = 62.0;
/// Fixed heights of the cards around the tables; the tables take what the window leaves.
const TIMELINE_CARD: f32 = 268.0;
const TOTAL_CARD: f32 = 116.0;
const GAP: f32 = 12.0;
/// Download and upload share one stacked column, so the tables look the same at every window
/// size and the names keep their room when three tables sit side by side.
const AMOUNT_COLUMN: f32 = 88.0;

fn columns() -> [Col; 2] {
    [Col::Flex(1.0), Col::Fixed(AMOUNT_COLUMN)]
}

/// What one table row shows.
struct RowSpec<'a> {
    identity: &'a str,
    title: &'a str,
    subtitle: &'a str,
    down: u64,
    up: u64,
    avatar: Option<(&'a str, Option<TextureId>)>,
    selected: bool,
    selectable: bool,
    /// This row's total as a share of the largest row, for the bar under it.
    share: f32,
    /// What it is moving right now, as (download, upload) speeds. Shown in place of the subtitle.
    live: Option<(String, String)>,
}

fn paint_row(ui: &mut egui::Ui, spec: &RowSpec<'_>) -> egui::Response {
    let t = tk(ui.ctx());
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let response = ui.interact(
        rect,
        ui.make_persistent_id(spec.identity),
        if spec.selectable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| {
        let mut label = format!(
            "{}: {} down, {} up",
            spec.title,
            format_bytes(spec.down as f64),
            format_bytes(spec.up as f64)
        );
        if let Some((down, up)) = &spec.live {
            label = format!("{label}; now {down} down, {up} up");
        }
        if spec.selectable {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                spec.selected,
                label,
            )
        } else {
            egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label)
        }
    });
    row_hover(ui, rect, &response, spec.selected);
    let columns = cells(rect, &columns(), 8.0, 16.0);
    let painter = ui.painter();
    let mut left = columns[0].left();
    if let Some((name, texture)) = spec.avatar {
        controls::avatar(
            painter,
            &t,
            Rect::from_center_size(Pos2::new(left + 16.0, rect.center().y), Vec2::splat(32.0)),
            name,
            texture,
        );
        left += 44.0;
    }
    let text_area = |top: f32, bottom: f32| {
        Rect::from_min_max(
            Pos2::new(left, rect.top() + top),
            Pos2::new(columns[0].right(), rect.top() + bottom),
        )
    };
    if spec.subtitle.is_empty() && spec.live.is_none() {
        text::paint(
            painter,
            text_area(0.0, ROW_HEIGHT - 8.0),
            Align2::LEFT_CENTER,
            spec.title,
            text::medium(14.0),
            t.foreground,
        );
    } else {
        text::paint(
            painter,
            text_area(8.0, 26.0),
            Align2::LEFT_CENTER,
            spec.title,
            text::medium(14.0),
            t.foreground,
        );
        let sub = text_area(26.0, 42.0);
        if let Some((down, up)) = &spec.live {
            // Speeds right now, with the arrows the rest of the app uses for each direction.
            let font = text::regular(12.0);
            let mut x = sub.left();
            for (icon, tone, amount) in [
                (Icon::ArrowDown, t.chart_down, down),
                (Icon::ArrowUp, t.chart_up, up),
            ] {
                let width = text::text_width(painter, amount, font.clone());
                if x + 14.0 + width > sub.right() {
                    break;
                }
                icon::paint(
                    painter,
                    icon,
                    Pos2::new(x + 5.0, sub.center().y),
                    11.0,
                    tone,
                );
                text::paint(
                    painter,
                    Rect::from_min_max(Pos2::new(x + 14.0, sub.top()), sub.right_bottom()),
                    Align2::LEFT_CENTER,
                    amount,
                    font.clone(),
                    t.muted_foreground,
                );
                x += 14.0 + width + 12.0;
            }
        } else {
            text::paint(
                painter,
                sub,
                Align2::LEFT_CENTER,
                spec.subtitle,
                text::regular(12.0),
                t.muted_foreground,
            );
        }
    }
    let (down, up) = (format_bytes(spec.down as f64), format_bytes(spec.up as f64));
    let cell = columns[1];
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(cell.left(), rect.top() + 9.0),
            Pos2::new(cell.right(), rect.top() + 28.0),
        ),
        Align2::RIGHT_CENTER,
        &down,
        text::medium(13.5),
        t.foreground,
    );
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(cell.left(), rect.top() + 28.0),
            Pos2::new(cell.right(), rect.top() + 46.0),
        ),
        Align2::RIGHT_CENTER,
        &up,
        text::regular(12.0),
        t.muted_foreground,
    );
    // A slim bar under the text: its length is the share of the busiest row, split between
    // download and upload.
    let track = Rect::from_min_max(
        Pos2::new(left, rect.bottom() - 14.0),
        Pos2::new(columns[1].right(), rect.bottom() - 10.0),
    );
    painter.rect_filled(track, CornerRadius::same(2), t.muted);
    let total = spec.down as f64 + spec.up as f64;
    let filled = track.width() * spec.share.clamp(0.0, 1.0);
    if total > 0.0 && filled > 0.5 {
        let split = track.left() + filled * (spec.down as f64 / total) as f32;
        let end = track.left() + filled;
        if spec.down > 0 {
            painter.rect_filled(
                Rect::from_min_max(
                    track.min,
                    Pos2::new(split.max(track.left() + 1.0), track.bottom()),
                ),
                CornerRadius::same(2),
                t.chart_down,
            );
        }
        if spec.up > 0 && end > split {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(split, track.top()),
                    Pos2::new(end.max(split + 1.0), track.bottom()),
                ),
                CornerRadius::same(2),
                t.chart_up,
            );
        }
    }
    row_border(ui, rect);
    if spec.selectable {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

#[allow(clippy::too_many_arguments)]
/// A card holding a table: a title with a count, a column header and a scrolling body.
fn table_card(
    ui: &mut egui::Ui,
    height: f32,
    title: &str,
    description: &str,
    count: usize,
    first_column: &str,
    actions: impl FnOnce(&mut egui::Ui),
    body: impl FnOnce(&mut egui::Ui),
) {
    let t = tk(ui.ctx());
    card_fixed(ui, 0, height, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 20,
                right: 20,
                top: 18,
                bottom: 12,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                card_header(ui, &format!("{title}  ·  {count}"), description, actions);
            });
        let (header, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), TABLE_HEADER_HEIGHT),
            Sense::hover(),
        );
        let columns = cells(header, &columns(), 8.0, 16.0);
        let labels = [
            (first_column, Align2::LEFT_CENTER),
            ("Down / Up", Align2::RIGHT_CENTER),
        ];
        for (cell, (label, align)) in columns.iter().zip(labels) {
            header_cell(
                ui,
                &format!("{title}-{label}"),
                *cell,
                label,
                align,
                false,
                None,
            );
        }
        ui.painter().hline(
            header.x_range(),
            header.top(),
            crate::ui::tokens::hairline(ui.ctx(), t.border),
        );
        row_border(ui, header);
        body(ui);
    });
}

impl MonitorApp {
    pub(super) fn applications(&mut self, ui: &mut egui::Ui) {
        if self.settings.filter.source != Source::Transport {
            self.switch_source(Source::Transport);
        }
        let totals = self.app_totals();
        // A search may hide the previously selected app; keep all three tables in one scope.
        if self.data.history_revision == self.filter_revision
            && self
                .selected_app
                .as_ref()
                .is_some_and(|p| !totals.iter().any(|t| &t.path == p))
        {
            self.selected_app = None;
        }
        let data = self.traffic_breakdown(&totals);
        if self.selected_app.is_some() {
            self.app_timeline_card(ui, &totals);
        }
        let width = ui.available_width();
        let gap = 12.0;
        // The three tables share the height left between the toolbar and the totals card.
        let tables = (ui.available_height() - TOTAL_CARD - GAP - PAGE_BOTTOM).max(240.0);
        let mut visible_hosts = Vec::new();
        let mut lifetime = self.hosts_lifetime;
        let lifetime_rows = if lifetime {
            self.lifetime_rows()
        } else {
            Vec::new()
        };
        // Always three columns side by side, whatever the window or interface size.
        {
            // The apps table is a little wider: its rows also carry the speeds right now.
            let room = width - gap * 2.0;
            let first = (room * 0.40).floor();
            let second = (room * 0.33).floor();
            let third = room - first - second;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                cell(ui, first, |ui| self.apps_table(ui, tables, &totals));
                cell(ui, second, |ui| {
                    let (description, rows) = if lifetime {
                        ("All-time totals per host", &lifetime_rows)
                    } else {
                        ("Reverse DNS, else address", &data.hosts)
                    };
                    visible_hosts = self.ranked_table(
                        ui,
                        tables,
                        "Hosts",
                        description,
                        "Host",
                        rows,
                        true,
                        Some(&mut lifetime),
                    )
                });
                cell(ui, third, |ui| {
                    self.ranked_table(
                        ui,
                        tables,
                        "Traffic type",
                        "Estimated from connection ports",
                        "Type",
                        &data.types,
                        false,
                        None,
                    );
                });
            });
        }
        self.hosts_lifetime = lifetime;
        self.apps_total_card(ui);
        if !visible_hosts.is_empty()
            && self
                .host_request
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            let _ = self.worker.tx.send(Command::ResolveHosts(visible_hosts));
            self.host_request = Some(Instant::now());
        }
    }

    fn apps_table(&mut self, ui: &mut egui::Ui, height: f32, totals: &[AppTotal]) {
        let mut clear = false;
        let interval = self.data.snapshot.interval.max(0.001);
        let selected = self.selected_app.clone();
        let mut chosen = None;
        let has_selection = selected.is_some();
        let top = totals.first().map_or(0, |t| t.down.saturating_add(t.up));
        table_card(
            ui,
            height,
            "Apps",
            "Select an app to focus the other tables",
            totals.len(),
            "App",
            |ui| {
                if has_selection
                    && Button::new("Clear selection")
                        .variant(Variant::Ghost)
                        .size(Size::Sm)
                        .show(ui)
                        .clicked()
                {
                    clear = true;
                }
            },
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("ranked-apps")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show_rows(ui, ROW_HEIGHT, totals.len(), |ui, range| {
                        for total in &totals[range] {
                            let texture = self.icons.texture(ui.ctx(), &total.path);
                            let directory = executable_directory(&total.path);
                            let is_selected = selected.as_deref() == Some(total.path.as_str());
                            let response = paint_row(
                                ui,
                                &RowSpec {
                                    identity: &total.path,
                                    title: &total.name,
                                    subtitle: &directory,
                                    down: total.down,
                                    up: total.up,
                                    avatar: Some((&total.name, texture)),
                                    selected: is_selected,
                                    selectable: true,
                                    share: share_of(total.down.saturating_add(total.up), top),
                                    live: Some((
                                        self.rate(total.live_down as f64 / interval),
                                        self.rate(total.live_up as f64 / interval),
                                    )),
                                },
                            )
                            .on_hover_text(format!("{}\n{}", total.name, total.path));
                            if response.clicked() {
                                chosen = Some(if is_selected {
                                    None
                                } else {
                                    Some(total.path.clone())
                                });
                            }
                        }
                        if totals.is_empty() {
                            let loading = self.data.history_revision != self.filter_revision;
                            layout::empty_state(
                                ui,
                                if loading {
                                    "Loading usage…"
                                } else {
                                    "No matching app traffic yet."
                                },
                            );
                        }
                    });
            },
        );
        if clear {
            self.selected_app = None;
        }
        if let Some(choice) = chosen {
            self.selected_app = choice;
        }
    }

    /// A hosts or traffic-type table. Returns the addresses on screen that still lack a name.
    #[allow(clippy::too_many_arguments)]
    fn ranked_table(
        &mut self,
        ui: &mut egui::Ui,
        height: f32,
        title: &str,
        description: &str,
        first_column: &str,
        rows: &[RankedUsage],
        hosts: bool,
        lifetime: Option<&mut bool>,
    ) -> Vec<String> {
        let lifetime_view = lifetime.as_deref().copied().unwrap_or(false);
        let mut unresolved = Vec::new();
        let top = rows
            .iter()
            .map(|r| r.down.saturating_add(r.up))
            .max()
            .unwrap_or(0);
        table_card(
            ui,
            height,
            title,
            description,
            rows.len(),
            first_column,
            |ui| {
                if let Some(lifetime) = lifetime {
                    let label = if *lifetime { "This period" } else { "Lifetime" };
                    if Button::new(label)
                        .variant(Variant::Outline)
                        .size(Size::Sm)
                        .show(ui)
                        .clicked()
                    {
                        *lifetime = !*lifetime;
                    }
                }
            },
            |ui| {
                egui::ScrollArea::vertical()
                .id_salt(title)
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                    for row in &rows[range] {
                        if hosts {
                            unresolved.extend(row.addresses.iter().filter(|ip| *ip == &row.label).cloned());
                        }
                        let help = if hosts {
                            format!("{}\n{}\nHost name from reverse DNS when available; it may differ from the website name.", row.label, row.addresses.join("\n"))
                        } else {
                            format!("{}\n{}", row.label, row.subtitle)
                        };
                        paint_row(
                            ui,
                            &RowSpec {
                                identity: &row.label,
                                title: &row.label,
                                subtitle: &row.subtitle,
                                down: row.down,
                                up: row.up,
                                avatar: None,
                                selected: false,
                                selectable: false,
                                share: share_of(row.down.saturating_add(row.up), top),
                                live: None,
                            },
                        )
                        .on_hover_text(help);
                    }
                    if rows.is_empty() {
                        layout::empty_state(
                            ui,
                            if hosts && lifetime_view {
                                "No hosts recorded yet."
                            } else {
                                "No matching traffic yet."
                            },
                        );
                    }
                });
            },
        );
        unresolved
    }

    /// Captured app traffic for the period, shown under the tables.
    fn apps_total_card(&self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        let (down, up) = (self.data.history.down, self.data.history.up);
        card_fixed(ui, 20, TOTAL_CARD, |ui| {
            card_header(
                ui,
                "All apps",
                "Total captured app traffic for the period. Search and selection do not change it.",
                |_| {},
            );
            ui.add_space(10.0);
            let total = down as f64 + up as f64;
            let (bar, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Label,
                    true,
                    format!(
                        "Downloaded {}, uploaded {}",
                        format_bytes(down as f64),
                        format_bytes(up as f64)
                    ),
                )
            });
            let painter = ui.painter();
            painter.rect_filled(bar, CornerRadius::same(5), t.muted);
            if total > 0.0 {
                let split = bar.left() + bar.width() * (down as f64 / total) as f32;
                if down > 0 {
                    painter.rect_filled(
                        Rect::from_min_max(bar.min, Pos2::new(split, bar.bottom())),
                        CornerRadius {
                            nw: 5,
                            sw: 5,
                            ne: 0,
                            se: 0,
                        },
                        t.chart_down,
                    );
                }
                if up > 0 {
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(split, bar.top()), bar.max),
                        CornerRadius {
                            nw: 0,
                            sw: 0,
                            ne: 5,
                            se: 5,
                        },
                        t.chart_up,
                    );
                }
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (label, value, color) in [
                    ("Downloaded", down, t.chart_down),
                    ("Uploaded", up, t.chart_up),
                ] {
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(190.0, 20.0), Sense::hover());
                    ui.painter().rect_filled(
                        Rect::from_center_size(
                            Pos2::new(rect.left() + 5.0, rect.center().y),
                            Vec2::splat(10.0),
                        ),
                        CornerRadius::same(3),
                        color,
                    );
                    text::paint(
                        ui.painter(),
                        Rect::from_min_max(Pos2::new(rect.left() + 18.0, rect.top()), rect.max),
                        Align2::LEFT_CENTER,
                        &format!("{label}  {}", format_bytes(value as f64)),
                        text::medium(14.0),
                        t.foreground,
                    );
                }
            });
        });
    }

    /// The selected app's usage over the period, with a switch between bars sized to the
    /// period and calendar days or months.
    fn app_timeline_card(&mut self, ui: &mut egui::Ui, totals: &[AppTotal]) {
        let Some(path) = self.selected_app.clone() else {
            return;
        };
        let name = totals
            .iter()
            .find(|total| total.path == path)
            .map_or_else(|| self.label(&path), |total| total.name.clone());
        let series = self.data.app_series.clone().filter(|series| {
            series.app == path
                && series.revision == self.filter_revision
                && series.view == self.timeline_mode.view()
        });
        let caption = self.timeline_mode.span_caption().map_or_else(
            || "Download and upload over the selected period".to_owned(),
            |c| format!("{c}, whatever the period"),
        );
        card_fixed(ui, 20, TIMELINE_CARD, |ui| {
            card_header(ui, &name, &caption, |ui| {
                let mut mode = self.timeline_mode;
                if tabs(
                    ui,
                    "app-timeline-mode",
                    &mut mode,
                    &app_timeline::TimelineMode::ALL,
                ) {
                    self.timeline_mode = mode;
                }
            });
            ui.add_space(10.0);
            match series {
                Some(series) => {
                    let bars = app_timeline::bars(&series, self.timeline_mode);
                    let values: Vec<BarValue<'_>> = bars
                        .iter()
                        .map(|bar| BarValue {
                            label: &bar.label,
                            down: bar.down,
                            up: bar.up,
                        })
                        .collect();
                    chart::bars(
                        ui,
                        &values,
                        ui.available_height().max(100.0),
                        "No traffic from this app in this period",
                    );
                }
                None => {
                    let t = tk(ui.ctx());
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::new(ui.available_width(), ui.available_height().max(100.0)),
                        Sense::hover(),
                    );
                    text::paint(
                        ui.painter(),
                        rect,
                        Align2::CENTER_CENTER,
                        "Loading usage…",
                        text::regular(14.0),
                        t.muted_foreground,
                    );
                }
            }
        });
    }
}

/// A row's total as a fraction of the largest one.
fn share_of(total: u64, top: u64) -> f32 {
    if top == 0 {
        0.0
    } else {
        (total as f64 / top as f64).clamp(0.0, 1.0) as f32
    }
}
