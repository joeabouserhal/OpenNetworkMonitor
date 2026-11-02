//! Connections: a sortable, filterable table of live sockets with a row menu.
use super::*;
use crate::ui::{
    controls,
    layout::{
        Col, TABLE_HEADER_HEIGHT, card_fixed, cells, dialog, header_cell, menu_action, menu_item,
        menu_separator, row_border, row_hover, select,
    },
};
use connection_tools::{Column, ProtocolFilter, Sort, StatusFilter, describe, endpoint_ip};
use eframe::egui::PopupCloseBehavior;

const ROW_HEIGHT: f32 = 60.0;
/// The gap above the footnote under the table.
const GAP_BELOW: f32 = 12.0;
const COLUMNS: [Col; 6] = [
    Col::Flex(1.7),
    Col::Fixed(72.0),
    Col::Flex(1.5),
    Col::Flex(1.2),
    Col::Fixed(84.0),
    Col::Fixed(112.0),
];

/// What a row's context menu asked for, carried out once the table has been drawn.
enum RowAction {
    Copy(String),
    Reveal(String),
    EndProcess(u32, String),
    SearchApp(String),
}

impl MonitorApp {
    /// Connections matching the search and filters, as positions in the snapshot with their
    /// display names. Rebuilt only when a new snapshot, search, sort or filter arrives.
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
        let t = tk(ui.ctx());
        let snapshot = self.data.snapshot.clone();
        let rows = self.visible_connections();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            let width = (ui.available_width() - 440.0).max(160.0);
            controls::search(
                ui,
                &mut self.connection_search,
                "Search apps, addresses, countries or status",
                width,
            );
            select(
                ui,
                "protocol",
                "Protocol",
                self.protocol_filter.label(),
                None,
                PopupCloseBehavior::CloseOnClick,
                160.0,
                |ui| {
                    for choice in ProtocolFilter::ALL {
                        if menu_item(ui, self.protocol_filter == choice, choice.label()).clicked() {
                            self.protocol_filter = choice;
                        }
                    }
                },
            );
            select(
                ui,
                "status",
                "Status",
                self.status_filter.label(),
                None,
                PopupCloseBehavior::CloseOnClick,
                180.0,
                |ui| {
                    for choice in StatusFilter::ALL {
                        if menu_item(ui, self.status_filter == choice, choice.label()).clicked() {
                            self.status_filter = choice;
                        }
                    }
                },
            );
            let (rect, _) = ui.allocate_exact_size(Vec2::new(140.0, 36.0), Sense::hover());
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                &format!(
                    "{} of {} connections",
                    rows.len(),
                    snapshot.connections.len()
                ),
                text::regular(13.5),
                t.muted_foreground,
            );
        });
        // The table fills the window under the toolbar; its rows scroll.
        let height = (ui.available_height() - 18.0 - GAP_BELOW - PAGE_BOTTOM).max(240.0);
        let mut action: Option<RowAction> = None;
        let mut visible_hosts: Vec<String> = Vec::new();
        card_fixed(ui, 0, height, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let (header, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), TABLE_HEADER_HEIGHT),
                Sense::hover(),
            );
            for (cell, column) in cells(header, &COLUMNS, 12.0, 16.0)
                .into_iter()
                .zip(Column::ALL)
            {
                let sorted = self
                    .connection_sort
                    .filter(|s| s.column == column)
                    .map(|s| s.ascending);
                let response = header_cell(
                    ui,
                    column.title(),
                    cell,
                    column.title(),
                    Align2::LEFT_CENTER,
                    true,
                    sorted,
                );
                if response
                    .on_hover_text(format!("Sort by {}", column.title().to_lowercase()))
                    .clicked()
                {
                    self.connection_sort = Sort::clicked(self.connection_sort, column);
                }
            }
            row_border(ui, header);
            egui::ScrollArea::vertical()
                .id_salt("live-connections")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, ROW_HEIGHT, rows.len(), |ui, range| {
                    for (index, name) in &rows[range] {
                        let c = &snapshot.connections[*index];
                        let texture = self.icons.texture(ui.ctx(), &c.app);
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::click());
                        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, describe(name, c)));
                        row_hover(ui, rect, &response, false);
                        let columns = cells(rect, &COLUMNS, 12.0, 16.0);
                        let painter = ui.painter();
                        controls::avatar(
                            painter,
                            &t,
                            Rect::from_center_size(Pos2::new(columns[0].left() + 16.0, rect.center().y), Vec2::splat(32.0)),
                            name,
                            texture,
                        );
                        let left = columns[0].left() + 44.0;
                        text::paint(painter, Rect::from_min_max(Pos2::new(left, rect.top() + 10.0), Pos2::new(columns[0].right(), rect.top() + 30.0)), Align2::LEFT_CENTER, name, text::medium(14.0), t.foreground);
                        text::paint(
                            painter,
                            Rect::from_min_max(Pos2::new(left, rect.top() + 30.0), Pos2::new(columns[0].right(), rect.top() + 48.0)),
                            Align2::LEFT_CENTER,
                            &format!("PID {} · {}", c.pid, executable_directory(&c.app)),
                            text::regular(12.0),
                            t.muted_foreground,
                        );
                        // Protocol as a small outline badge.
                        let protocol = if c.protocol.is_empty() { "—" } else { c.protocol.as_str() };
                        let badge_rect = Rect::from_min_size(Pos2::new(columns[1].left(), rect.center().y - 10.0), Vec2::new((text::text_width(painter, protocol, text::medium(12.0)) + 16.0).min(columns[1].width()), 20.0));
                        painter.rect_stroke(badge_rect, egui::CornerRadius::same(10), crate::ui::tokens::hairline(ui.ctx(), t.border), egui::StrokeKind::Inside);
                        text::paint(painter, badge_rect, Align2::CENTER_CENTER, protocol, text::medium(12.0), t.muted_foreground);
                        // Remote endpoint, with the resolved host name beneath it.
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
                        let remote = if c.remote.is_empty() { "—" } else { c.remote.as_str() };
                        if let Some(host) = host {
                            text::paint(painter, Rect::from_min_max(Pos2::new(columns[2].left(), rect.top() + 10.0), Pos2::new(columns[2].right(), rect.top() + 30.0)), Align2::LEFT_CENTER, remote, text::mono(13.0), t.foreground);
                            text::paint(painter, Rect::from_min_max(Pos2::new(columns[2].left(), rect.top() + 30.0), Pos2::new(columns[2].right(), rect.top() + 48.0)), Align2::LEFT_CENTER, host, text::regular(12.0), t.muted_foreground);
                        } else {
                            text::paint(painter, columns[2], Align2::LEFT_CENTER, remote, text::mono(13.0), t.foreground);
                        }
                        text::paint(painter, columns[3], Align2::LEFT_CENTER, if c.local.is_empty() { "—" } else { &c.local }, text::mono(13.0), t.muted_foreground);
                        text::paint(painter, columns[4], Align2::LEFT_CENTER, if c.country.is_empty() { "—" } else { &c.country }, text::regular(14.0), t.foreground);
                        let established = c.state.eq_ignore_ascii_case("Established");
                        painter.circle_filled(Pos2::new(columns[5].left() + 4.0, rect.center().y), 3.5, if established { t.success } else { t.muted_foreground.gamma_multiply(0.6) });
                        text::paint(
                            painter,
                            Rect::from_min_max(Pos2::new(columns[5].left() + 16.0, rect.top()), columns[5].right_bottom()),
                            Align2::LEFT_CENTER,
                            if c.state.is_empty() { "—" } else { &c.state },
                            text::regular(14.0),
                            if established { t.foreground } else { t.muted_foreground },
                        );
                        row_border(ui, rect);
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
                            ui.set_width(250.0);
                            ui.spacing_mut().item_spacing.y = 2.0;
                            let pick = |ui: &mut egui::Ui, icon: Icon, label: &str, destructive: bool| {
                                let clicked = menu_action(ui, Some(icon), label, "", destructive).clicked();
                                if clicked {
                                    ui.close();
                                }
                                clicked
                            };
                            if pick(ui, Icon::Copy, "Copy remote address", false) {
                                action = Some(RowAction::Copy(c.remote.clone()));
                            }
                            if pick(ui, Icon::Copy, "Copy local address", false) {
                                action = Some(RowAction::Copy(c.local.clone()));
                            }
                            if pick(ui, Icon::Copy, "Copy connection details", false) {
                                action = Some(RowAction::Copy(describe(name, c)));
                            }
                            menu_separator(ui);
                            if pick(ui, Icon::Filter, "Show only this app", false) {
                                action = Some(RowAction::SearchApp(name.clone()));
                            }
                            if pick(ui, Icon::External, "Open file location", false) {
                                action = Some(RowAction::Reveal(c.app.clone()));
                            }
                            menu_separator(ui);
                            if pick(ui, Icon::Close, "End process…", true) {
                                action = Some(RowAction::EndProcess(c.pid, name.clone()));
                            }
                        });
                    }
                    if rows.is_empty() {
                        layout::empty_state(ui, "No connections match your search and filters.");
                    }
                });
        });
        let note = "Refreshes every 5 seconds. UDP destinations are in Overview → App traffic.";
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        text::paint(
            ui.painter(),
            rect,
            Align2::LEFT_CENTER,
            note,
            text::regular(12.5),
            t.muted_foreground,
        );
        self.apply_row_action(ui.ctx(), action);
        let ctx = ui.ctx().clone();
        self.end_process_dialog(&ctx);
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
    }

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
        let t = tk(ctx);
        let mut decision = None;
        let (_, should_close) = dialog(ctx, "end-process-dialog", 400.0, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                "End this process?",
                text::semibold(18.0),
                t.foreground,
            );
            let body = format!(
                "{name} (PID {pid}) will be stopped immediately. Anything it has not saved is lost."
            );
            let galley = ui.painter().layout(
                body,
                text::regular(14.0),
                t.muted_foreground,
                ui.available_width(),
            );
            let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
            ui.painter().galley(rect.min, galley, t.muted_foreground);
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if Button::new("End process")
                    .variant(Variant::Destructive)
                    .show(ui)
                    .clicked()
                {
                    decision = Some(true);
                }
                if Button::new("Cancel")
                    .variant(Variant::Outline)
                    .show(ui)
                    .clicked()
                {
                    decision = Some(false);
                }
            });
        });
        if should_close {
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
}
