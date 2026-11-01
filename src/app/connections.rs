use super::*;
use eframe::egui::{Align2, Margin, Pos2, Rect, Sense};
use widgets::{bold, mono, paint_text, pal, radius, sans};

const CONNECTION_ROW_HEIGHT: f32 = 52.0;

/// Six column cells with a fixed gap, inset so row highlights keep breathing room.
pub(super) fn connection_columns(rect: Rect) -> [Rect; 6] {
    let fractions = [0.24, 0.08, 0.22, 0.22, 0.10, 0.14];
    let width = (rect.width() - 28.0 - 40.0).max(1.0);
    let mut x = rect.left() + 14.0;
    fractions.map(|fraction| {
        let cell = Rect::from_min_size(
            Pos2::new(x, rect.top()),
            Vec2::new(width * fraction, rect.height()),
        );
        x += cell.width() + 8.0;
        cell
    })
}

impl MonitorApp {
    /// Connections matching the search, as positions in the snapshot with their display
    /// names. Rebuilt when a new snapshot or a different search arrives, not on every
    /// frame, since the text search formats and lowercases a string per connection.
    fn visible_connections(&mut self) -> Arc<Vec<(usize, String)>> {
        let key = (self.data.revision, self.connection_search.clone());
        if self.connection_cache.key.as_ref() != Some(&key) {
            let search = self.connection_search.to_lowercase();
            let rows: Vec<_> = self
                .data
                .snapshot
                .connections
                .iter()
                .enumerate()
                .filter_map(|(index, c)| {
                    let name = self.label(&c.app);
                    (search.is_empty()
                        || format!(
                            "{name} {} {} {} {} {} {}",
                            c.app, c.pid, c.local, c.remote, c.state, c.country
                        )
                        .to_lowercase()
                        .contains(&search))
                    .then_some((index, name))
                })
                .collect();
            self.connection_cache = ConnectionCache {
                key: Some(key),
                rows: Arc::new(rows),
            };
        }
        self.connection_cache.rows.clone()
    }
    pub(super) fn connections(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.spacing_mut().item_spacing.y = 0.0;
        let snapshot = self.data.snapshot.clone();
        let rows = self.visible_connections();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let width = (ui.available_width() - 100.0).clamp(180.0, 520.0);
            widgets::search_field(
                ui,
                &mut self.connection_search,
                "Search apps, addresses, countries or status",
                width,
            );
            ui.label(
                RichText::new(format!("{} of {}", rows.len(), snapshot.connections.len()))
                    .size(13.5)
                    .weak(),
            );
        });
        ui.add_space(14.0);
        let height = (ui.available_height() - 28.0).max(90.0);
        let frame = widgets::surface(ui).inner_margin(Margin {
            left: 4,
            right: 4,
            top: 10,
            bottom: 8,
        });
        ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
            widgets::surface_fill(ui, frame, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (header, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                for (rect, title) in connection_columns(header).into_iter().zip([
                    "Application",
                    "Protocol",
                    "Remote endpoint",
                    "Local endpoint",
                    "Country",
                    "Status",
                ]) {
                    paint_text(
                        ui.painter(),
                        rect,
                        Align2::LEFT_CENTER,
                        title,
                        bold(12.5),
                        p.dim,
                    );
                }
                widgets::divider(ui);
                egui::ScrollArea::vertical()
                    .id_salt("live-connections")
                    .auto_shrink([false, false])
                    .max_height(ui.available_height())
                    .show_rows(ui, CONNECTION_ROW_HEIGHT, rows.len(), |ui, range| {
                        for (index, name) in &rows[range] {
                            let c = &snapshot.connections[*index];
                            let icon = self.icons.texture(ui.ctx(), &c.app);
                            let (rect, response) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), CONNECTION_ROW_HEIGHT),
                                Sense::hover(),
                            );
                            let cells = connection_columns(rect);
                            let painter = ui.painter();
                            if response.hovered() {
                                painter.rect_filled(
                                    rect.shrink2(Vec2::new(0.0, 2.0)),
                                    radius(10.0),
                                    p.raised,
                                );
                            }
                            let mid = rect.center().y;
                            apps::app_tile(
                                painter,
                                Rect::from_center_size(
                                    Pos2::new(cells[0].left() + 15.0, mid),
                                    Vec2::splat(30.0),
                                ),
                                name,
                                icon,
                                &p,
                            );
                            let text_left = cells[0].left() + 42.0;
                            paint_text(
                                painter,
                                Rect::from_min_max(
                                    Pos2::new(text_left, rect.top() + 9.0),
                                    Pos2::new(cells[0].right(), rect.top() + 27.0),
                                ),
                                Align2::LEFT_CENTER,
                                name,
                                bold(14.0),
                                p.text,
                            );
                            paint_text(
                                painter,
                                Rect::from_min_max(
                                    Pos2::new(text_left, rect.top() + 28.0),
                                    Pos2::new(cells[0].right(), rect.top() + 44.0),
                                ),
                                Align2::LEFT_CENTER,
                                &format!("PID {} · {}", c.pid, executable_directory(&c.app)),
                                sans(12.0),
                                p.dim,
                            );
                            let protocol = if c.protocol.is_empty() {
                                "—"
                            } else {
                                c.protocol.as_str()
                            };
                            let pill_width =
                                widgets::text_width(painter, protocol, bold(12.0)) + 16.0;
                            let pill = Rect::from_min_size(
                                Pos2::new(cells[1].left(), mid - 10.0),
                                Vec2::new(pill_width.min(cells[1].width()), 20.0),
                            );
                            painter.rect_filled(pill, radius(10.0), p.raised);
                            paint_text(
                                painter,
                                pill,
                                Align2::CENTER_CENTER,
                                protocol,
                                bold(12.0),
                                p.dim,
                            );
                            for (i, value) in [(2, c.remote.as_str()), (3, c.local.as_str())] {
                                let value = if value.is_empty() { "—" } else { value };
                                paint_text(
                                    painter,
                                    Rect::from_min_max(
                                        Pos2::new(cells[i].left(), mid - 10.0),
                                        Pos2::new(cells[i].right(), mid + 10.0),
                                    ),
                                    Align2::LEFT_CENTER,
                                    value,
                                    mono(12.5),
                                    if i == 2 { p.text } else { p.dim },
                                );
                            }
                            let country = if c.country.is_empty() {
                                "—"
                            } else {
                                c.country.as_str()
                            };
                            paint_text(
                                painter,
                                Rect::from_min_max(
                                    Pos2::new(cells[4].left(), mid - 10.0),
                                    Pos2::new(cells[4].right(), mid + 10.0),
                                ),
                                Align2::LEFT_CENTER,
                                country,
                                sans(13.5),
                                p.text,
                            );
                            let state = if c.state.is_empty() {
                                "—"
                            } else {
                                c.state.as_str()
                            };
                            let established = c.state.eq_ignore_ascii_case("Established");
                            painter.circle_filled(
                                Pos2::new(cells[5].left() + 4.0, mid),
                                3.5,
                                if established {
                                    p.good
                                } else {
                                    p.dim.gamma_multiply(0.6)
                                },
                            );
                            paint_text(
                                painter,
                                Rect::from_min_max(
                                    Pos2::new(cells[5].left() + 16.0, mid - 10.0),
                                    Pos2::new(cells[5].right(), mid + 10.0),
                                ),
                                Align2::LEFT_CENTER,
                                state,
                                sans(13.0),
                                if established { p.text } else { p.dim },
                            );
                            response.on_hover_text(format!(
                                "{name}\n{}\nPID {} · {}\nRemote: {}\nLocal: {}\n{} · {}",
                                c.app, c.pid, c.protocol, c.remote, c.local, c.country, c.state
                            ));
                        }
                        if rows.is_empty() {
                            widgets::empty_state(ui, "No connections match your search.");
                        }
                    });
            });
        });
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Refreshes every 5 seconds · UDP destinations are in Overview → App traffic",
            )
            .size(12.5)
            .weak(),
        );
    }
}
