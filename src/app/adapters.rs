use super::*;
use eframe::egui::{Align, Align2, Layout, Pos2, Rect, Sense};
use widgets::{ButtonKind, bold, paint_text, paint_value, pal, sans, strong_text, thin};

const CARD_HEIGHT: f32 = 196.0;

impl MonitorApp {
    pub(super) fn adapters(&mut self, ui: &mut egui::Ui) {
        let p = pal(ui);
        ui.horizontal(|ui| {
            widgets::badge(
                ui,
                &format!("{} adapters", self.data.snapshot.adapters.len()),
                p.dim,
            );
            widgets::badge(
                ui,
                &format!(
                    "{} connected",
                    self.data.snapshot.adapters.iter().filter(|a| a.up).count()
                ),
                p.good,
            );
        });
        ui.add_space(10.0);
        let columns = if ui.available_width() >= 660.0 { 2 } else { 1 };
        let interval = self.data.snapshot.interval.max(0.001);
        let mut view_adapter = None;
        for chunk in self.data.snapshot.adapters.chunks(columns) {
            ui.columns(columns, |cols| {
                for (col, a) in cols.iter_mut().zip(chunk) {
                    let row = self
                        .data
                        .snapshot
                        .live_rows
                        .iter()
                        .find(|r| r.source == "adapter" && r.adapter_id == a.id);
                    let down = self.rate(row.map_or(0, |r| r.down) as f64 / interval);
                    let up = self.rate(row.map_or(0, |r| r.up) as f64 / interval);
                    if adapter_card(col, a, &down, &up) {
                        view_adapter = Some(a.id.clone());
                    }
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = view_adapter {
            self.page = Page::Overview;
            self.settings.filter.adapters = HashSet::from([id]);
            // The card promises everything on this adapter, not one Wi-Fi network of it.
            self.settings.filter.wifi_network = None;
            self.settings.filter.source = Source::Adapter;
            self.update_filter();
        }
    }
}

/// One adapter as a card. Returns true when "View traffic" was chosen.
fn adapter_card(ui: &mut egui::Ui, a: &Adapter, down: &str, up: &str) -> bool {
    let p = pal(ui);
    let mut view = false;
    widgets::surface(ui).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(CARD_HEIGHT);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let title_width = (ui.available_width() - 118.0).max(80.0);
            ui.allocate_ui_with_layout(
                Vec2::new(title_width, 32.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(title_width);
                    widgets::status_dot(ui, if a.up { p.good } else { p.dim });
                    ui.add(egui::Label::new(strong_text(&a.name, 16.5)).truncate())
                        .on_hover_text(&a.name);
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                view = widgets::button(ui, ButtonKind::Secondary, "View traffic").clicked();
            });
        });
        ui.add(egui::Label::new(RichText::new(&a.description).size(13.5).weak()).truncate())
            .on_hover_text(&a.description);
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            widgets::badge(
                ui,
                if a.physical {
                    "Physical"
                } else {
                    "Virtual / VPN"
                },
                p.dim,
            );
            widgets::badge(
                ui,
                if a.up { "Connected" } else { "Disconnected" },
                if a.up { p.good } else { p.dim },
            );
        });
        ui.add_space(12.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::hover());
        let half = (rect.width() / 2.0).floor();
        for (i, label, value, color) in [(0.0, "Download", down, p.down), (1.0, "Upload", up, p.up)]
        {
            let cell = Rect::from_min_size(
                rect.min + Vec2::new(i * half, 0.0),
                Vec2::new(half, rect.height()),
            );
            ui.painter()
                .circle_filled(Pos2::new(cell.left() + 4.0, cell.top() + 8.0), 3.5, color);
            paint_text(
                ui.painter(),
                Rect::from_min_size(
                    cell.min + Vec2::new(14.0, 0.0),
                    Vec2::new(cell.width() - 14.0, 16.0),
                ),
                Align2::LEFT_CENTER,
                label,
                sans(12.5),
                p.dim,
            );
            paint_value(
                ui.painter(),
                Rect::from_min_size(
                    cell.min + Vec2::new(0.0, 18.0),
                    Vec2::new(cell.width(), 32.0),
                ),
                Align2::LEFT_CENTER,
                value,
                (thin(22.0), sans(13.0)),
                p.text,
                p.dim,
            );
            widgets::describe(ui, cell, &format!("{label}: {value}"));
        }
        if !a.addresses.is_empty() {
            ui.add_space(10.0);
            for (ip, prefix) in a.addresses.iter().take(2) {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("{ip}/{prefix}"))
                            .font(widgets::mono(12.5))
                            .color(p.dim),
                    )
                    .truncate(),
                );
            }
            if a.addresses.len() > 2 {
                ui.label(
                    RichText::new(format!("+{} more", a.addresses.len() - 2))
                        .font(bold(12.0))
                        .color(p.dim),
                )
                .on_hover_text(
                    a.addresses
                        .iter()
                        .map(|(ip, prefix)| format!("{ip}/{prefix}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        }
    });
    view
}
