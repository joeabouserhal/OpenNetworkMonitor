use super::*;
use eframe::egui::{Align2, PopupCloseBehavior, Rect, Sense, StrokeKind};
use widgets::{bold, check_item, menu_heading, menu_item, paint_text, pal, radius};

impl MonitorApp {
    pub(super) fn adapter_choices(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if menu_item(ui, self.settings.filter.adapters.is_empty(), "All adapters").clicked() {
            self.settings.filter.adapters.clear();
            changed = true;
        }
        ui.add_space(4.0);
        widgets::divider(ui);
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("adapter-choices")
            .max_height(150.0)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for adapter in &self.data.snapshot.adapters {
                    let mut checked = self.settings.filter.adapters.is_empty()
                        || self.settings.filter.adapters.contains(&adapter.id);
                    let label = format!(
                        "{}{}",
                        adapter.name,
                        if adapter.up { "" } else { " · disconnected" }
                    );
                    if check_item(ui, &mut checked, &label) {
                        toggle_adapter(
                            &mut self.settings.filter.adapters,
                            &self.data.snapshot.adapters,
                            &adapter.id,
                            checked,
                            self.settings.filter.source == Source::Transport,
                        );
                        changed = true;
                    }
                }
                if self.settings.filter.source == Source::Transport {
                    let mut checked = self.settings.filter.adapters.is_empty()
                        || self.settings.filter.adapters.contains(UNASSIGNED_ADAPTER);
                    if check_item(ui, &mut checked, "Unassigned traffic") {
                        toggle_adapter(
                            &mut self.settings.filter.adapters,
                            &self.data.snapshot.adapters,
                            UNASSIGNED_ADAPTER,
                            checked,
                            true,
                        );
                        changed = true;
                    }
                }
            });
        ui.add_space(4.0);
        widgets::divider(ui);
        ui.add_space(4.0);
        menu_heading(ui, "Wi-Fi connections")
            .on_hover_text("A specific connection shows only its Wi-Fi traffic within the selected adapters. Older and unidentifiable traffic cannot be assigned to a named connection.");
        if menu_item(
            ui,
            self.settings.filter.wifi_network.is_none(),
            "All connections",
        )
        .clicked()
        {
            self.settings.filter.wifi_network = None;
            changed = true;
        }
        egui::ScrollArea::vertical()
            .id_salt("wifi-connection-choices")
            .max_height(150.0)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for network in &self.data.snapshot.wifi_networks {
                    ui.push_id(&network.id, |ui| {
                        let selected = self.settings.filter.wifi_network.as_deref()
                            == Some(network.id.as_str());
                        if menu_item(ui, selected, &wifi_display_name(&network.name)).clicked() {
                            self.settings.filter.wifi_network = Some(network.id.clone());
                            changed = true;
                        }
                    });
                }
                let unknown = self.settings.filter.wifi_network.as_deref() == Some(WIFI_UNKNOWN);
                if menu_item(ui, unknown, "Unidentified Wi-Fi").clicked() {
                    self.settings.filter.wifi_network = Some(WIFI_UNKNOWN.to_owned());
                    changed = true;
                }
            });
        changed
    }
    /// Chip label, chip value and the full explanation shown on hover.
    pub(super) fn adapter_filter_caption(&self) -> (String, String, String) {
        let adapters = if self.settings.filter.adapters.is_empty() {
            "All adapters".to_owned()
        } else if self.settings.filter.adapters.contains(NO_ADAPTERS) {
            "No adapters".to_owned()
        } else {
            format!("{} adapters selected", self.settings.filter.adapters.len())
        };
        if let Some(selected) = &self.settings.filter.wifi_network {
            let name = if selected == WIFI_UNKNOWN {
                "Unidentified Wi-Fi".to_owned()
            } else {
                self.data
                    .snapshot
                    .wifi_networks
                    .iter()
                    .find(|network| network.id == *selected)
                    .map(|network| wifi_display_name(&network.name))
                    .unwrap_or_else(|| "Unavailable network".to_owned())
            };
            let tooltip = format!(
                "Wi-Fi: {name} · {adapters}. Only traffic on this connection within the selected adapters is shown."
            );
            ("Wi-Fi".to_owned(), name, tooltip)
        } else {
            let value = if self.settings.filter.adapters.is_empty() {
                "All".to_owned()
            } else if self.settings.filter.adapters.contains("__none__") {
                "None".to_owned()
            } else {
                self.settings.filter.adapters.len().to_string()
            };
            (
                "Adapters".to_owned(),
                value,
                format!("{adapters} · All Wi-Fi connections"),
            )
        }
    }
    pub(super) fn toolbar(&mut self, ui: &mut egui::Ui, apps: bool) {
        let before = (
            self.settings.filter.seconds,
            self.settings.filter.time_range,
            self.settings.filter.scope,
        );
        let mut adapters_changed = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_min_height(widgets::CONTROL_HEIGHT);
            let caption = period_caption(&self.settings.filter);
            widgets::dropdown(
                ui,
                if apps { "app-period" } else { "overall-period" },
                "Period",
                caption,
                PopupCloseBehavior::CloseOnClick,
                244.0,
                |ui| self.period_menu(ui, caption),
            );
            let (label, value, tooltip) = self.adapter_filter_caption();
            widgets::dropdown(
                ui,
                if apps {
                    "app-adapters"
                } else {
                    "overall-adapters"
                },
                &label,
                &value,
                PopupCloseBehavior::CloseOnClickOutside,
                280.0,
                |ui| adapters_changed = self.adapter_choices(ui),
            )
            .on_hover_text(tooltip);
            if apps {
                let value = match self.settings.filter.scope {
                    Scope::All => "All",
                    Scope::Lan => "LAN",
                    Scope::Wan => "WAN",
                    Scope::Unknown => "Other",
                };
                widgets::dropdown(
                    ui,
                    "app-networks",
                    "Networks",
                    value,
                    PopupCloseBehavior::CloseOnClick,
                    180.0,
                    |ui| {
                        for (scope, label) in [
                            (Scope::All, "All networks"),
                            (Scope::Lan, "LAN"),
                            (Scope::Wan, "WAN"),
                            (Scope::Unknown, "Other"),
                        ] {
                            if menu_item(ui, self.settings.filter.scope == scope, label).clicked() {
                                self.settings.filter.scope = scope;
                            }
                        }
                    },
                );
                let width = ui.available_width().floor().max(160.0);
                widgets::search_field(ui, &mut self.app_search, "Search apps or folders", width);
            }
        });
        if adapters_changed
            || before
                != (
                    self.settings.filter.seconds,
                    self.settings.filter.time_range,
                    self.settings.filter.scope,
                )
        {
            self.update_filter();
        }
        self.custom_period_dialog(ui.ctx());
        ui.add_space(14.0);
    }
    /// Quick ranges as a grid, with the calendar-based choices listed below.
    fn period_menu(&mut self, ui: &mut egui::Ui, caption: &str) {
        let filter = &mut self.settings.filter;
        menu_heading(ui, "Last");
        let rolling: Vec<_> = PERIODS.iter().filter(|p| p.0 > 0).collect();
        for row in rolling.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &&(seconds, short, long) in row {
                    let selected = filter.time_range.is_none() && filter.seconds == seconds;
                    if period_cell(ui, selected, short, long).clicked() {
                        filter.seconds = seconds;
                        filter.time_range = None;
                    }
                }
            });
        }
        ui.add_space(4.0);
        widgets::divider(ui);
        ui.add_space(4.0);
        if menu_item(ui, filter.all_time(), "All time").clicked() {
            filter.seconds = 0;
            filter.time_range = None;
        }
        if menu_item(ui, filter.month_to_date(), "Since beginning of month").clicked() {
            filter.seconds = PERIOD_MONTH_TO_DATE;
            filter.time_range = None;
        }
        if menu_item(ui, caption == "Custom", "Custom range…").clicked() {
            self.custom_period = Some(CustomPeriod::from_filter(&self.settings.filter));
        }
    }
}

/// One quick-range button in the period grid.
fn period_cell(ui: &mut egui::Ui, selected: bool, label: &str, tip: &str) -> egui::Response {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(56.0, 32.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            tip,
        )
    });
    let painter = ui.painter();
    if selected || response.hovered() {
        painter.rect_filled(
            rect,
            radius(8.0),
            if selected && !response.hovered() {
                p.accent_soft
            } else {
                p.raised
            },
        );
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            radius(8.0),
            widgets::focus_ring(ui),
            StrokeKind::Inside,
        );
    }
    paint_text(
        painter,
        Rect::from_min_max(rect.min, rect.max),
        Align2::CENTER_CENTER,
        label,
        bold(14.0),
        if selected { p.accent } else { p.text },
    );
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
