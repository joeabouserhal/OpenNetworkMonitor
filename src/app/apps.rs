use super::*;
use eframe::egui::{
    Align, Align2, Layout, Margin, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
    TextureId,
};
use widgets::{Col, Icon, Palette, badge, bold, paint_icon, paint_text, pal, radius, sans};

const USAGE_ROW_HEIGHT: f32 = widgets::ROW_HEIGHT;
const STRIP_HEIGHT: f32 = 58.0;

impl MonitorApp {
    pub(super) fn applications(&mut self, ui: &mut egui::Ui) {
        if self.settings.filter.source != Source::Transport {
            self.switch_source(Source::Transport);
        }
        self.toolbar(ui, true);
        let totals = self.app_totals();
        // A search may hide the previously selected app. Keep all three lists in the same scope.
        if self.data.history_revision == self.filter_revision
            && self
                .selected_app
                .as_ref()
                .is_some_and(|p| !totals.iter().any(|t| &t.path == p))
        {
            self.selected_app = None;
        }
        let data = self.traffic_breakdown(&totals);
        let lists_height = (ui.available_height() - STRIP_HEIGHT - 12.0).max(120.0);
        let mut visible_hosts = Vec::new();
        ui.allocate_ui(Vec2::new(ui.available_width(), lists_height), |ui| {
            widgets::columns(
                ui,
                [Col::Flex(1.15), Col::Flex(1.0), Col::Flex(0.85)],
                12.0,
                |cols| {
                    let [apps, hosts, types] = cols;
                    let frame = |ui: &egui::Ui| {
                        widgets::surface(ui).inner_margin(Margin {
                            left: 10,
                            right: 10,
                            top: 16,
                            bottom: 10,
                        })
                    };
                    let (apps_frame, hosts_frame, types_frame) =
                        (frame(apps), frame(hosts), frame(types));
                    widgets::surface_fill(apps, apps_frame, |ui| {
                        if list_heading(
                            ui,
                            "Apps",
                            totals.len(),
                            "Ranked by total usage. Select an app to focus the adjacent lists.",
                            self.selected_app.is_some(),
                        ) {
                            self.selected_app = None;
                        }
                        let interval = self.data.snapshot.interval.max(0.001);
                        let top = totals.first().map_or(0, |t| t.down.saturating_add(t.up));
                        ui.spacing_mut().item_spacing.y = 0.0;
                        egui::ScrollArea::vertical()
                            .id_salt("ranked-apps")
                            .auto_shrink([false, false])
                            .max_height(ui.available_height())
                            .show_rows(ui, USAGE_ROW_HEIGHT, totals.len(), |ui, range| {
                                for t in &totals[range] {
                                    let directory = executable_directory(&t.path);
                                    let icon = self.icons.texture(ui.ctx(), &t.path);
                                    let response = usage_row(
                                        ui,
                                        &t.path,
                                        &t.name,
                                        &directory,
                                        (t.down, t.up),
                                        share_of(t.down.saturating_add(t.up), top),
                                        Some(self.selected_app.as_deref() == Some(&t.path)),
                                        icon,
                                    )
                                    .on_hover_text(format!(
                                        "{}\n{}\nDownload now: {}\nUpload now: {}",
                                        t.name,
                                        t.path,
                                        self.rate(t.live_down as f64 / interval),
                                        self.rate(t.live_up as f64 / interval)
                                    ));
                                    if response.clicked() {
                                        self.selected_app =
                                            if self.selected_app.as_deref() == Some(&t.path) {
                                                None
                                            } else {
                                                Some(t.path.clone())
                                            };
                                    }
                                }
                                if totals.is_empty() {
                                    widgets::empty_state(ui, "No matching app traffic yet.");
                                }
                            });
                    });
                    visible_hosts = widgets::surface_fill(hosts, hosts_frame, |ui| {
                        ranked_list(ui, "Hosts", &data.hosts, "ranked-hosts", true)
                    })
                    .inner;
                    widgets::surface_fill(types, types_frame, |ui| {
                        ranked_list(ui, "Traffic type", &data.types, "ranked-types", false);
                    });
                },
            );
        });
        ui.add_space(12.0);
        let p = pal(ui);
        let (down, up) = (self.data.history.down, self.data.history.up);
        widgets::surface(ui)
            .inner_margin(Margin::symmetric(20, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(28.0);
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.label(RichText::new("All apps").size(13.5).color(p.dim));
                    ui.add_space(14.0);
                    footer_stat(ui, "Downloaded", down, p.down);
                    ui.add_space(8.0);
                    footer_stat(ui, "Uploaded", up, p.up);
                    ui.add_space(18.0);
                    traffic_distribution_bar(ui, down, up);
                });
            })
            .response
            .on_hover_text(
                "Total captured app traffic for the selected period, adapters, and network. Search and app selection do not change this total.",
            );
        if !visible_hosts.is_empty()
            && self
                .host_request
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            let _ = self.worker.tx.send(Command::ResolveHosts(visible_hosts));
            self.host_request = Some(Instant::now());
        }
    }
}

fn share_of(total: u64, top: u64) -> f32 {
    if top == 0 {
        0.0
    } else {
        (total as f64 / top as f64).clamp(0.0, 1.0) as f32
    }
}

fn footer_stat(ui: &mut egui::Ui, label: &str, value: u64, color: Color32) {
    widgets::status_dot(ui, color);
    ui.spacing_mut().item_spacing.x = 6.0;
    ui.label(RichText::new(label).size(13.5).weak());
    ui.label(widgets::strong_text(compact_bytes(value), 16.0));
}

fn list_heading(
    ui: &mut egui::Ui,
    title: &str,
    count: usize,
    help: &str,
    clear_selection: bool,
) -> bool {
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.set_height(30.0);
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(8.0);
        ui.label(widgets::strong_text(title, 16.0))
            .on_hover_text(help);
        badge(ui, &count.to_string(), pal(ui).dim);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if clear_selection {
                clear = widgets::button(ui, widgets::ButtonKind::Ghost, "All apps")
                    .on_hover_text("Clear selected app")
                    .clicked();
            } else {
                ui.add_space(8.0);
                ui.label(RichText::new("Total usage").size(12.0).weak());
            }
        });
    });
    ui.add_space(8.0);
    clear
}

fn ranked_list(
    ui: &mut egui::Ui,
    title: &str,
    rows: &[RankedUsage],
    id: &str,
    hosts: bool,
) -> Vec<String> {
    list_heading(
        ui,
        title,
        rows.len(),
        if hosts {
            "Host names from the Windows resolver; addresses are kept when a name is unavailable. Names may identify shared infrastructure."
        } else {
            "Inferred from connection ports (HTTP, DNS and other common services). Encrypted contents are not inspected. Older traffic without ports is Unclassified."
        },
        false,
    );
    let top = rows
        .iter()
        .map(|r| r.down.saturating_add(r.up))
        .max()
        .unwrap_or(0);
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .max_height(ui.available_height())
        .show_rows(ui, USAGE_ROW_HEIGHT, rows.len(), |ui, range| {
            let mut visible = Vec::new();
            for row in &rows[range] {
                if hosts {
                    visible.extend(row.addresses.iter().filter(|ip| *ip == &row.label).cloned());
                }
                let help = if hosts {
                    format!("{}\n{}\nHost name from reverse DNS when available; it may differ from the website name.", row.label, row.addresses.join("\n"))
                } else {
                    format!("{}\n{}", row.label, row.subtitle)
                };
                usage_row(
                    ui,
                    &row.label,
                    &row.label,
                    &row.subtitle,
                    (row.down, row.up),
                    share_of(row.down.saturating_add(row.up), top),
                    None,
                    None,
                )
                .on_hover_text(help);
            }
            if rows.is_empty() {
                widgets::empty_state(ui, "No matching traffic yet.");
            }
            visible
        })
        .inner
}

/// The filled portions of the period's upload/download bar. An empty total
/// leaves the neutral track visible; f64 arithmetic avoids overflowing u64.
pub(super) fn traffic_bar_segments(
    rect: egui::Rect,
    down: u64,
    up: u64,
) -> [Option<egui::Rect>; 2] {
    let total = down as f64 + up as f64;
    if total == 0.0 {
        return [None, None];
    }
    let split = rect.left() + rect.width() * (down as f64 / total) as f32;
    [
        (down > 0).then_some(egui::Rect::from_min_max(
            rect.min,
            egui::pos2(split, rect.bottom()),
        )),
        (up > 0).then_some(egui::Rect::from_min_max(
            egui::pos2(split, rect.top()),
            rect.max,
        )),
    ]
}

fn traffic_distribution_bar(ui: &mut egui::Ui, down: u64, up: u64) {
    let p = pal(ui);
    let width = ui.available_width().max(0.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 8.0), Sense::hover());
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    painter.rect_filled(rect, 4, p.raised);
    let [download, upload] = traffic_bar_segments(rect, down, up);
    let corners = |left: bool, right: bool| egui::CornerRadius {
        nw: if left { 4 } else { 0 },
        sw: if left { 4 } else { 0 },
        ne: if right { 4 } else { 0 },
        se: if right { 4 } else { 0 },
    };
    if let Some(segment) = download {
        painter.rect_filled(segment, corners(true, up == 0), p.down);
    }
    if let Some(segment) = upload {
        painter.rect_filled(
            segment.shrink2(Vec2::new(0.0, 0.0)),
            corners(down == 0, true),
            p.up,
        );
    }
    response.on_hover_text(
        "Share of downloaded and uploaded bytes in the selected period, adapters, and network.",
    );
}

/// Rounded app icon, or a coloured initial when the executable has none.
pub(super) fn app_tile(
    painter: &Painter,
    rect: Rect,
    name: &str,
    icon: Option<TextureId>,
    p: &Palette,
) {
    let corner = radius(rect.width() * 0.28);
    if let Some(texture) = icon {
        painter.rect_filled(rect, corner, p.raised);
        painter.image(
            texture,
            rect.shrink(rect.width() / 8.0),
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    let hash = name
        .to_lowercase()
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
    let tint = [p.down, p.up, p.good, p.bad, p.accent][(hash % 5) as usize];
    painter.rect_filled(rect, corner, p.card.lerp_to_gamma(tint, 0.2));
    let initial: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into());
    paint_text(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &initial,
        bold(rect.width() * 0.46),
        tint,
    );
}

#[allow(clippy::too_many_arguments)]
fn usage_row(
    ui: &mut egui::Ui,
    identity: &str,
    name: &str,
    subtitle: &str,
    bytes: (u64, u64),
    share: f32,
    selection: Option<bool>,
    icon: Option<TextureId>,
) -> Response {
    let p = pal(ui);
    let (down, up) = bytes;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), USAGE_ROW_HEIGHT),
        Sense::hover(),
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id(identity),
        if selection.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let selected = selection.unwrap_or(false);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            name,
        )
    });
    let body = rect.shrink2(Vec2::new(0.0, 2.0));
    let painter = ui.painter();
    if selected {
        painter.rect_filled(body, radius(11.0), p.accent_soft);
    } else if response.hovered() {
        painter.rect_filled(body, radius(11.0), p.raised);
    }
    if response.has_focus() {
        painter.rect_stroke(
            body,
            radius(11.0),
            Stroke::new(1.5, p.accent),
            StrokeKind::Inside,
        );
    }
    let left = body.left() + 10.0;
    let right = body.right() - 12.0;
    let top = rect.top();
    let text_left = if selection.is_some() {
        let tile = Rect::from_min_size(Pos2::new(left, rect.center().y - 16.0), Vec2::splat(32.0));
        app_tile(painter, tile, name, icon, &p);
        left + 44.0
    } else {
        left
    };
    let total = compact_bytes(down.saturating_add(up));
    let total_width = widgets::text_width(painter, &total, bold(14.5)) + 2.0;
    paint_text(
        painter,
        Rect::from_min_max(
            Pos2::new(text_left, top + 9.0),
            Pos2::new((right - total_width - 10.0).max(text_left), top + 27.0),
        ),
        Align2::LEFT_CENTER,
        name,
        bold(14.5),
        p.text,
    );
    paint_text(
        painter,
        Rect::from_min_max(
            Pos2::new(right - total_width, top + 9.0),
            Pos2::new(right, top + 27.0),
        ),
        Align2::RIGHT_CENTER,
        &total,
        bold(14.5),
        p.text,
    );
    // Direction values sit on the second line, right-aligned, with the subtitle truncating before them.
    let (down_text, up_text) = (compact_bytes(down), compact_bytes(up));
    let (down_width, up_width) = (
        widgets::text_width(painter, &down_text, sans(12.0)),
        widgets::text_width(painter, &up_text, sans(12.0)),
    );
    let line = top + 37.0;
    let mut x = right;
    let mut start = right;
    for (icon, color, value, width) in [
        (Icon::Up, p.up, &up_text, up_width),
        (Icon::Down, p.down, &down_text, down_width),
    ] {
        start = x - (14.0 + width);
        paint_icon(painter, icon, Pos2::new(start + 5.0, line), 11.0, color);
        paint_text(
            painter,
            Rect::from_min_max(
                Pos2::new(start + 14.0, line - 8.0),
                Pos2::new(x + 1.0, line + 8.0),
            ),
            Align2::LEFT_CENTER,
            value,
            sans(12.0),
            p.dim,
        );
        x = start - 12.0;
    }
    paint_text(
        painter,
        Rect::from_min_max(
            Pos2::new(text_left, line - 8.0),
            Pos2::new((start - 10.0).max(text_left), line + 8.0),
        ),
        Align2::LEFT_CENTER,
        subtitle,
        sans(12.0),
        p.dim,
    );
    // A slim bar ranks the row against the busiest one and splits it by direction.
    let track = Rect::from_min_max(
        Pos2::new(text_left, top + 46.0),
        Pos2::new(right, top + 49.0),
    );
    painter.rect_filled(track, radius(1.5), p.text.gamma_multiply(0.07));
    if share > 0.0 && down.saturating_add(up) > 0 {
        let filled = Rect::from_min_size(
            track.min,
            Vec2::new(
                (track.width() * share).clamp(3.0, track.width()),
                track.height(),
            ),
        );
        let down_part = (down as f64 / (down as f64 + up as f64)) as f32;
        widgets::split_bar(
            painter,
            filled,
            &[(down_part, p.down), (1.0 - down_part, p.up)],
            Color32::TRANSPARENT,
        );
    }
    if selection.is_some() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
