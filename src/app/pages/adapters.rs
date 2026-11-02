//! Adapters: one card per network adapter with live speeds and a shortcut to its traffic.
use super::*;
use crate::ui::{
    controls::{self, BadgeVariant},
    layout::card,
};

const CARD_HEIGHT: f32 = 236.0;

impl MonitorApp {
    pub(super) fn adapters(&mut self, ui: &mut egui::Ui) {
        let t = tk(ui.ctx());
        ui.horizontal(|ui| {
            let adapters = &self.data.snapshot.adapters;
            controls::badge(
                ui,
                BadgeVariant::Secondary,
                &format!("{} adapters", adapters.len()),
            );
            controls::badge(
                ui,
                BadgeVariant::Success,
                &format!("{} connected", adapters.iter().filter(|a| a.up).count()),
            );
        });
        let width = ui.available_width();
        // Always three cards across.
        let columns = 3;
        let gap = 12.0;
        let column = ((width - gap * (columns as f32 - 1.0)) / columns as f32).floor();
        let interval = self.data.snapshot.interval.max(0.001);
        let snapshot = self.data.snapshot.clone();
        let mut view_adapter = None;
        // The cards scroll within the page, so the badges above stay put.
        egui::ScrollArea::vertical()
            .id_salt("adapter-cards")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 12.0;
                for chunk in snapshot.adapters.chunks(columns) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for adapter in chunk {
                            let row = snapshot
                                .live_rows
                                .iter()
                                .find(|r| r.source == "adapter" && r.adapter_id == adapter.id);
                            let down = self.rate(row.map_or(0, |r| r.down) as f64 / interval);
                            let up = self.rate(row.map_or(0, |r| r.up) as f64 / interval);
                            cell(ui, column, |ui| {
                                if adapter_card(ui, &t, adapter, &down, &up) {
                                    view_adapter = Some(adapter.id.clone());
                                }
                            });
                        }
                    });
                }
                if snapshot.adapters.is_empty() {
                    card(ui, 20, |ui| {
                        layout::empty_state(ui, "No network adapters were found.")
                    });
                }
            });
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

/// One adapter. Returns true when "View traffic" was chosen.
fn adapter_card(
    ui: &mut egui::Ui,
    t: &crate::ui::tokens::Tokens,
    a: &Adapter,
    down: &str,
    up: &str,
) -> bool {
    let mut view = false;
    card(ui, 20, |ui| {
        ui.set_min_height(CARD_HEIGHT - 42.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(Vec2::new(12.0, 22.0), Sense::hover());
            ui.painter().circle_filled(
                dot.center(),
                4.0,
                if a.up { t.success } else { t.muted_foreground },
            );
            let (rect, response) = ui.allocate_exact_size(
                Vec2::new(ui.available_width() - 120.0, 22.0),
                Sense::hover(),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Label, true, a.name.clone())
            });
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                &a.name,
                text::semibold(15.0),
                t.foreground,
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                view = Button::new("View traffic")
                    .variant(Variant::Outline)
                    .size(Size::Sm)
                    .show(ui)
                    .clicked();
            });
        });
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        text::paint(
            ui.painter(),
            rect,
            Align2::LEFT_CENTER,
            &a.description,
            text::regular(13.0),
            t.muted_foreground,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            controls::badge(
                ui,
                BadgeVariant::Outline,
                if a.physical {
                    "Physical"
                } else {
                    "Virtual / VPN"
                },
            );
            if a.is_wifi {
                controls::badge(ui, BadgeVariant::Outline, "Wi-Fi");
            }
            controls::badge(
                ui,
                if a.up {
                    BadgeVariant::Success
                } else {
                    BadgeVariant::Secondary
                },
                if a.up { "Connected" } else { "Disconnected" },
            );
        });
        ui.add_space(4.0);
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
        let half = (rect.width() / 2.0).floor();
        for (i, label, value, color) in [
            (0.0, "Download", down, t.chart_down),
            (1.0, "Upload", up, t.chart_up),
        ] {
            let cell = Rect::from_min_size(
                rect.min + Vec2::new(i * half, 0.0),
                Vec2::new(half, rect.height()),
            );
            ui.painter().rect_filled(
                Rect::from_center_size(
                    Pos2::new(cell.left() + 5.0, cell.top() + 8.0),
                    Vec2::splat(9.0),
                ),
                egui::CornerRadius::same(2),
                color,
            );
            text::paint(
                ui.painter(),
                Rect::from_min_size(
                    cell.min + Vec2::new(16.0, 0.0),
                    Vec2::new(cell.width() - 16.0, 16.0),
                ),
                Align2::LEFT_CENTER,
                label,
                text::regular(12.5),
                t.muted_foreground,
            );
            let galley = text::number_with_unit(
                ui.painter(),
                value,
                text::semibold(20.0),
                text::regular(13.0),
                t.foreground,
                t.muted_foreground,
            );
            ui.painter()
                .galley(cell.min + Vec2::new(0.0, 22.0), galley, t.foreground);
            crate::ui::a11y::label(ui, cell, &format!("{label}: {value}"));
        }
        for (ip, prefix) in a.addresses.iter().take(2) {
            let (rect, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                &format!("{ip}/{prefix}"),
                text::mono(12.0),
                t.muted_foreground,
            );
        }
        if a.addresses.len() > 2 {
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                &format!("+{} more", a.addresses.len() - 2),
                text::medium(12.0),
                t.muted_foreground,
            );
            response.on_hover_text(
                a.addresses
                    .iter()
                    .map(|(ip, prefix)| format!("{ip}/{prefix}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    });
    view
}
