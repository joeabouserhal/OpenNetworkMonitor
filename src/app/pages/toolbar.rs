//! The filter row shared by both Overview tabs: source tabs, period, adapters, networks
//! and search.
use super::*;
use crate::ui::{
    controls,
    layout::{menu_check, menu_item, menu_label, menu_separator, select, tabs},
};
use eframe::egui::PopupCloseBehavior;

impl MonitorApp {
    pub(super) fn filter_bar(&mut self, ui: &mut egui::Ui, apps: bool) {
        let before = (
            self.settings.filter.seconds,
            self.settings.filter.time_range,
            self.settings.filter.scope,
        );
        let mut adapters_changed = false;
        let mut source = self.settings.filter.source;
        // One row at every window size; the search box takes the width that is left.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            if tabs(
                ui,
                "source",
                &mut source,
                &[
                    (Source::Adapter, "Overall traffic"),
                    (Source::Transport, "App traffic"),
                ],
            ) {
                self.switch_source(source);
            }
            let caption = period_caption(&self.settings.filter);
            select(
                ui,
                "period",
                "Period",
                caption,
                None,
                PopupCloseBehavior::CloseOnClick,
                248.0,
                |ui| self.period_menu(ui, caption),
            );
            let (label, value, tooltip) = self.adapter_filter_caption();
            select(
                ui,
                "adapters",
                &label,
                &value,
                None,
                PopupCloseBehavior::CloseOnClickOutside,
                290.0,
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
                select(
                    ui,
                    "networks",
                    "Networks",
                    value,
                    None,
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
                let width = ui.available_width().max(120.0);
                controls::search(ui, &mut self.app_search, "Search apps or folders", width);
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
        let ctx = ui.ctx().clone();
        self.custom_period_dialog(&ctx);
    }

    fn period_menu(&mut self, ui: &mut egui::Ui, caption: &str) {
        let filter = &mut self.settings.filter;
        menu_label(ui, "Last");
        let rolling: Vec<_> = PERIODS.iter().filter(|p| p.0 > 0).collect();
        for row in rolling.chunks(4) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for &&(seconds, short, long) in row {
                    let selected = filter.time_range.is_none() && filter.seconds == seconds;
                    if Button::new(short)
                        .variant(if selected {
                            Variant::Secondary
                        } else {
                            Variant::Ghost
                        })
                        .size(Size::Sm)
                        .width(56.0)
                        .show(ui)
                        .on_hover_text(long)
                        .clicked()
                    {
                        filter.seconds = seconds;
                        filter.time_range = None;
                    }
                }
            });
        }
        menu_separator(ui);
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

    /// The adapter and Wi-Fi choices; returns true when the selection changed.
    fn adapter_choices(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if menu_item(ui, self.settings.filter.adapters.is_empty(), "All adapters").clicked() {
            self.settings.filter.adapters.clear();
            changed = true;
        }
        menu_separator(ui);
        egui::ScrollArea::vertical()
            .id_salt("adapter-choices")
            .max_height(170.0)
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
                    if menu_check(ui, &mut checked, &label) {
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
                    if menu_check(ui, &mut checked, "Unassigned traffic") {
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
        menu_separator(ui);
        menu_label(ui, "Wi-Fi connections");
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
}
