use super::connection_tools::{
    self, Column, ProtocolFilter, Sort, StatusFilter, describe, endpoint_ip,
};
use super::*;
use eframe::egui::{Align2, Margin, PopupCloseBehavior, Pos2, Rect, Sense, Shape, Stroke};
use widgets::{bold, mono, paint_text, pal, radius, sans};

/// What a row's context menu asked for, carried out once the list has been drawn.
enum RowAction {
    Copy(String),
    Reveal(String),
    EndProcess(u32, String),
    SearchApp(String),
}

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
    fn apply_row_action(&mut self, ctx: &egui::Context, action: Option<RowAction>) {
        match action {
            Some(RowAction::Copy(text)) => ctx.copy_text(text),
            Some(RowAction::Reveal(path)) => {
                if !connection_tools::reveal_in_explorer(&path) {
                    self.set_notice("That file location is not available.");
                }
            }
            Some(RowAction::EndProcess(pid, name)) => self.pending_end = Some((pid, name)),
            Some(RowAction::SearchApp(name)) => self.connection_search = name,
            None => {}
        }
    }
    /// Asks before ending a process; the app that owns it may lose unsaved work.
    fn end_process_dialog(&mut self, ctx: &egui::Context) {
        let Some((pid, name)) = self.pending_end.clone() else {
            return;
        };
        let mut decision = None;
        let response = egui::Modal::new(egui::Id::new("end-process-dialog"))
            .area(egui::Modal::default_area(egui::Id::new("end-process-dialog")).fade_in(false))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .inner_margin(24)
                    .corner_radius(18),
            )
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(widgets::strong_text("End this process?", 20.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{name} (PID {pid}) will be stopped immediately. Anything it has not saved is lost."
                    ))
                    .size(14.0)
                    .weak(),
                );
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button_with(
                        ui,
                        widgets::ButtonKind::Primary,
                        None,
                        "End process",
                        Some(130.0),
                    )
                    .clicked()
                    {
                        decision = Some(true);
                    }
                    if widgets::button_with(
                        ui,
                        widgets::ButtonKind::Ghost,
                        None,
                        "Cancel",
                        Some(90.0),
                    )
                    .clicked()
                    {
                        decision = Some(false);
                    }
                });
            });
        if response.should_close() {
            decision.get_or_insert(false);
        }
        match decision {
            Some(true) => {
                self.pending_end = None;
                match connection_tools::end_process(pid) {
                    Ok(()) => self.set_notice(&format!("Ended {name} (PID {pid}).")),
                    Err(error) => self.set_notice(&error),
                }
            }
            Some(false) => self.pending_end = None,
            None => {}
        }
    }
    /// Connections matching the search, as positions in the snapshot with their display
    /// names. Rebuilt when a new snapshot or a different search arrives, not on every
    /// frame, since the text search formats and lowercases a string per connection.
    fn visible_connections(&mut self) -> Arc<Vec<(usize, String)>> {
        let key = (
            self.data.revision,
            self.connection_search.clone(),
            self.connection_sort,
            self.protocol_filter,
            self.status_filter,
        );
        if self.connection_cache.key.as_ref() != Some(&key) {
            let search = self.connection_search.to_lowercase();
            let connections = &self.data.snapshot.connections;
            let mut rows: Vec<_> = connections
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    self.protocol_filter.matches(&c.protocol)
                        && self.status_filter.matches(&c.state)
                })
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
            if let Some(sort) = self.connection_sort {
                rows.sort_by(|(a, name_a), (b, name_b)| {
                    let order = connection_tools::compare(
                        sort.column,
                        (&connections[*a], name_a),
                        (&connections[*b], name_b),
                    );
                    if sort.ascending {
                        order
                    } else {
                        order.reverse()
                    }
                });
            }
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
            let width = (ui.available_width() - 520.0).clamp(180.0, 420.0);
            widgets::search_field(
                ui,
                &mut self.connection_search,
                "Search apps, addresses, countries or status",
                width,
            );
            widgets::dropdown(
                ui,
                "connection-protocol",
                "Protocol",
                self.protocol_filter.label(),
                PopupCloseBehavior::CloseOnClick,
                160.0,
                |ui| {
                    for choice in ProtocolFilter::ALL {
                        if widgets::menu_item(ui, self.protocol_filter == choice, choice.label())
                            .clicked()
                        {
                            self.protocol_filter = choice;
                        }
                    }
                },
            );
            widgets::dropdown(
                ui,
                "connection-status",
                "Status",
                self.status_filter.label(),
                PopupCloseBehavior::CloseOnClick,
                180.0,
                |ui| {
                    for choice in StatusFilter::ALL {
                        if widgets::menu_item(ui, self.status_filter == choice, choice.label())
                            .clicked()
                        {
                            self.status_filter = choice;
                        }
                    }
                },
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
        let mut action: Option<RowAction> = None;
        let mut visible_hosts: Vec<String> = Vec::new();
        ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
            widgets::surface_fill(ui, frame, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (header, _) =
                    ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::hover());
                for (rect, column) in connection_columns(header).into_iter().zip(Column::ALL) {
                    let title = column.title();
                    let click = ui.interact(
                        rect,
                        ui.id().with(("connection-sort", title)),
                        Sense::click(),
                    );
                    click.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Sort by {title}"),
                        )
                    });
                    let sorted = self.connection_sort.filter(|sort| sort.column == column);
                    paint_text(
                        ui.painter(),
                        rect,
                        Align2::LEFT_CENTER,
                        title,
                        bold(12.5),
                        if sorted.is_some() || click.hovered() {
                            p.text
                        } else {
                            p.dim
                        },
                    );
                    if let Some(sort) = sorted {
                        let x = rect.left() + widgets::text_width(ui.painter(), title, bold(12.5)) + 10.0;
                        let y = rect.center().y;
                        let points = if sort.ascending {
                            vec![
                                Pos2::new(x - 4.0, y + 2.5),
                                Pos2::new(x + 4.0, y + 2.5),
                                Pos2::new(x, y - 3.0),
                            ]
                        } else {
                            vec![
                                Pos2::new(x - 4.0, y - 2.5),
                                Pos2::new(x + 4.0, y - 2.5),
                                Pos2::new(x, y + 3.0),
                            ]
                        };
                        ui.painter()
                            .add(Shape::convex_polygon(points, p.text, Stroke::NONE));
                    }
                    if click
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(format!("Sort by {}", title.to_lowercase()))
                        .clicked()
                    {
                        self.connection_sort = Sort::clicked(self.connection_sort, column);
                    }
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
                                Sense::click(),
                            );
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    describe(name, c),
                                )
                            });
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
                            let remote_ip = endpoint_ip(&c.remote);
                            let host = remote_ip
                                .and_then(|ip| snapshot.host_labels.get(&ip.to_string()))
                                .filter(|host| !host.is_empty());
                            if let Some(ip) = remote_ip
                                && host.is_none()
                                && classify(ip, &[]) == Scope::Wan
                            {
                                visible_hosts.push(ip.to_string());
                            }
                            for (i, value) in [(2, c.remote.as_str()), (3, c.local.as_str())] {
                                let value = if value.is_empty() { "—" } else { value };
                                // A resolved name gets a second line under the address.
                                let (top, bottom) = if i == 2 && host.is_some() {
                                    (mid - 17.0, mid + 1.0)
                                } else {
                                    (mid - 10.0, mid + 10.0)
                                };
                                paint_text(
                                    painter,
                                    Rect::from_min_max(
                                        Pos2::new(cells[i].left(), top),
                                        Pos2::new(cells[i].right(), bottom),
                                    ),
                                    Align2::LEFT_CENTER,
                                    value,
                                    mono(12.5),
                                    if i == 2 { p.text } else { p.dim },
                                );
                                if i == 2
                                    && let Some(host) = host
                                {
                                    paint_text(
                                        painter,
                                        Rect::from_min_max(
                                            Pos2::new(cells[i].left(), mid + 2.0),
                                            Pos2::new(cells[i].right(), mid + 19.0),
                                        ),
                                        Align2::LEFT_CENTER,
                                        host,
                                        sans(12.0),
                                        p.dim,
                                    );
                                }
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
                            let response = response.on_hover_text(format!(
                                "{name}\n{}\nPID {} · {}\nRemote: {}{}\nLocal: {}\n{} · {}\nRight-click for actions",
                                c.app,
                                c.pid,
                                c.protocol,
                                c.remote,
                                host.map(|host| format!(" ({host})")).unwrap_or_default(),
                                c.local,
                                c.country,
                                c.state
                            ));
                            response.context_menu(|ui| {
                                ui.set_width(236.0);
                                ui.spacing_mut().item_spacing.y = 2.0;
                                let pick = |ui: &mut egui::Ui, label: &str| {
                                    let clicked = widgets::menu_item(ui, false, label).clicked();
                                    if clicked {
                                        ui.close();
                                    }
                                    clicked
                                };
                                if pick(ui, "Copy remote address") {
                                    action = Some(RowAction::Copy(c.remote.clone()));
                                }
                                if pick(ui, "Copy local address") {
                                    action = Some(RowAction::Copy(c.local.clone()));
                                }
                                if pick(ui, "Copy connection details") {
                                    action = Some(RowAction::Copy(describe(name, c)));
                                }
                                widgets::divider(ui);
                                if pick(ui, "Show only this app") {
                                    action = Some(RowAction::SearchApp(name.clone()));
                                }
                                if pick(ui, "Open file location") {
                                    action = Some(RowAction::Reveal(c.app.clone()));
                                }
                                if pick(ui, "End process…") {
                                    action = Some(RowAction::EndProcess(c.pid, name.clone()));
                                }
                            });
                        }
                        if rows.is_empty() {
                            widgets::empty_state(ui, "No connections match your search.");
                        }
                    });
            });
        });
        self.apply_row_action(ui.ctx(), action);
        self.end_process_dialog(ui.ctx());
        if !visible_hosts.is_empty()
            && self
                .host_request
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(2))
        {
            visible_hosts.sort();
            visible_hosts.dedup();
            let _ = self.worker.tx.send(Command::ResolveHosts(visible_hosts));
            self.host_request = Some(Instant::now());
        }
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
