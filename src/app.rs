use crate::{
    graph::{Graph, destination_breakdown, download_color, format_number, upload_color},
    icons::IconCache,
    model::*,
    store::Store,
    traffic::{Breakdown, RankedUsage, breakdown, executable_directory, executable_filename},
    tray::Tray,
    widgets::{self, badge, section_title, surface},
    worker::{Command, Shared, Worker},
};
use anyhow::Result;
use eframe::egui::{self, Color32, RichText, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

mod period;
use period::{CustomPeriod, PERIODS, period_caption};

#[cfg(feature = "ui-preview")]
mod preview;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum Theme {
    #[default]
    Dark,
    Light,
    Nord,
    Mocha,
}
impl Theme {
    fn label(self) -> &'static str {
        match self {
            Self::Dark => "Slate",
            Self::Light => "Light",
            Self::Nord => "Nord",
            Self::Mocha => "Catppuccin Mocha",
        }
    }
    fn apply(self, ctx: &egui::Context) {
        let light = self == Self::Light;
        let mut v = if light {
            egui::Visuals::light()
        } else {
            egui::Visuals::dark()
        };
        let (bg, panel, raised, accent) = match self {
            Self::Light => (
                Color32::from_rgb(244, 245, 246),
                Color32::WHITE,
                Color32::from_rgb(233, 235, 237),
                Color32::from_rgb(28, 113, 83),
            ),
            Self::Dark => (
                Color32::from_rgb(23, 25, 28),
                Color32::from_rgb(31, 34, 38),
                Color32::from_rgb(45, 49, 54),
                Color32::from_rgb(133, 205, 160),
            ),
            Self::Nord => (
                Color32::from_rgb(46, 52, 64),
                Color32::from_rgb(59, 66, 82),
                Color32::from_rgb(67, 76, 94),
                Color32::from_rgb(163, 190, 140),
            ),
            Self::Mocha => (
                Color32::from_rgb(30, 30, 46),
                Color32::from_rgb(49, 50, 68),
                Color32::from_rgb(69, 71, 90),
                Color32::from_rgb(166, 227, 161),
            ),
        };
        v.panel_fill = bg;
        v.window_fill = panel;
        v.faint_bg_color = panel;
        v.extreme_bg_color = bg;
        v.override_text_color = Some(if light {
            Color32::from_rgb(32, 35, 39)
        } else {
            Color32::from_rgb(239, 241, 243)
        });
        v.weak_text_color = Some(if light {
            Color32::from_rgb(88, 94, 101)
        } else {
            Color32::from_rgb(179, 185, 193)
        });
        v.selection.bg_fill = if light {
            Color32::from_rgb(215, 234, 224)
        } else {
            Color32::from_rgb(44, 64, 53)
        };
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(
            1.0,
            if light {
                Color32::from_rgb(213, 218, 224)
            } else {
                Color32::from_rgb(58, 63, 70)
            },
        );
        v.selection.stroke.color = accent;
        v.widgets.inactive.weak_bg_fill = raised;
        v.widgets.inactive.bg_fill = raised;
        v.widgets.hovered.weak_bg_fill = raised.gamma_multiply(1.2);
        v.widgets.active.bg_fill = raised;
        v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(
            1.0,
            if light {
                Color32::from_gray(165)
            } else {
                Color32::from_gray(100)
            },
        );
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
        v.widgets.inactive.corner_radius = 5.into();
        v.widgets.hovered.corner_radius = 5.into();
        v.widgets.active.corner_radius = 5.into();
        v.widgets.hovered.fg_stroke.color = if light {
            Color32::from_rgb(25, 30, 35)
        } else {
            Color32::WHITE
        };
        v.widgets.active.fg_stroke.color = if light {
            Color32::from_rgb(32, 35, 39)
        } else {
            Color32::from_rgb(239, 241, 243)
        };
        v.hyperlink_color = accent;
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 8.0);
            s.spacing.button_padding = Vec2::new(12.0, 7.0);
            s.spacing.interact_size.y = 30.0;
            for style in [egui::TextStyle::Body, egui::TextStyle::Button] {
                s.text_styles
                    .insert(style, egui::FontId::proportional(15.0));
            }
            s.text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(13.5));
        });
    }
}
#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    theme: Theme,
    filter: Filter,
    close_to_tray: bool,
    geo: Option<PathBuf>,
    bits: bool,
    ui_scale: f32,
    wide_summary_initialized: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            filter: Filter::default(),
            close_to_tray: true,
            geo: None,
            bits: false,
            ui_scale: 0.9,
            wide_summary_initialized: false,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Overview,
    Connections,
    Adapters,
    Settings,
}
impl Page {
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Connections => "Connections",
            Self::Adapters => "Adapters",
            Self::Settings => "Settings",
        }
    }
}
#[derive(Default)]
struct AppTotal {
    path: String,
    name: String,
    down: u64,
    up: u64,
    live_down: u64,
    live_up: u64,
}

struct FrozenChart {
    points: Vec<Point>,
    window: (i64, i64),
}
pub struct MonitorApp {
    worker: Worker,
    icons: IconCache,
    startup: crate::startup::Controller,
    tray: Option<Tray>,
    data: Shared,
    settings: Settings,
    page: Page,
    graph: Graph,
    show_down: bool,
    show_up: bool,
    log: bool,
    paused: bool,
    frozen: Option<FrozenChart>,
    filter_revision: u64,
    selected_app: Option<String>,
    connection_search: String,
    app_search: String,
    data_dir: PathBuf,
    view_warning: String,
    show_graph_options: bool,
    custom_period: Option<CustomPeriod>,
    app_cache: Arc<Vec<AppTotal>>,
    app_cache_key: Option<(u64, u64, u64, String)>,
    breakdown_cache: Arc<Breakdown>,
    breakdown_cache_key: Option<(u64, u64, u64, String, Option<String>)>,
    host_request: Option<Instant>,
}
impl MonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, data_dir: PathBuf) -> Result<Self> {
        let mut settings = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, "settings"))
            .unwrap_or_default();
        settings.filter.search.clear();
        settings.filter.apps.clear();
        settings.filter.scope = Scope::All;
        install_fonts(&cc.egui_ctx);
        settings.theme.apply(&cc.egui_ctx);
        settings.ui_scale = valid_ui_scale(settings.ui_scale);
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let worker = Worker::start(
            store,
            settings.filter.clone(),
            settings.geo.clone(),
            cc.egui_ctx.clone(),
        )?;
        let (tray, view_warning) = match Tray::new(cc.egui_ctx.clone()) {
            Ok(t) => (Some(t), String::new()),
            Err(e) => (
                None,
                format!("Tray unavailable: {e}. Closing the window will exit."),
            ),
        };
        if std::env::args().any(|arg| arg == "--startup") {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(tray.is_none()));
        }
        Ok(Self {
            worker,
            icons: IconCache::new(),
            startup: crate::startup::Controller::new(cc.egui_ctx.clone()),
            tray,
            data: Shared::default(),
            settings,
            page: Page::Overview,
            graph: Graph,
            show_down: true,
            show_up: true,
            log: false,
            paused: false,
            frozen: None,
            filter_revision: 0,
            selected_app: None,
            connection_search: String::new(),
            app_search: String::new(),
            data_dir,
            view_warning,
            show_graph_options: false,
            custom_period: None,
            app_cache: Arc::new(Vec::new()),
            app_cache_key: None,
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
        })
    }
    fn update_filter(&mut self) {
        if self.settings.filter.source == Source::Adapter {
            self.settings.filter.scope = Scope::All;
        }
        self.filter_revision += 1;
        let _ = self.worker.tx.send(Command::Filter(
            self.settings.filter.clone(),
            self.filter_revision,
        ));
        self.paused = false;
        self.frozen = None;
        self.data.history = Arc::default();
        self.data.classified = ScopeTotals::default();
        self.data.top_apps.clear();
    }
    fn switch_source(&mut self, source: Source) {
        if self.settings.filter.source != source {
            self.settings.filter.source = source;
            self.selected_app = None;
            self.update_filter();
        }
    }
    fn adapter_choices(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if ui
            .selectable_label(self.settings.filter.adapters.is_empty(), "All adapters")
            .clicked()
        {
            self.settings.filter.adapters.clear();
            changed = true;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("adapter-choices")
            .max_height(120.0)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                for adapter in &self.data.snapshot.adapters {
                    let mut checked = self.settings.filter.adapters.is_empty()
                        || self.settings.filter.adapters.contains(&adapter.id);
                    if ui
                        .checkbox(
                            &mut checked,
                            format!(
                                "{}{}",
                                adapter.name,
                                if adapter.up { "" } else { " · disconnected" }
                            ),
                        )
                        .changed()
                    {
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
                        || self.settings.filter.adapters.contains("unassigned");
                    if ui.checkbox(&mut checked, "Unassigned traffic").changed() {
                        toggle_adapter(
                            &mut self.settings.filter.adapters,
                            &self.data.snapshot.adapters,
                            "unassigned",
                            checked,
                            true,
                        );
                        changed = true;
                    }
                }
            });
        ui.separator();
        ui.label(RichText::new("Wi-Fi connections").strong())
            .on_hover_text("A specific connection shows only its Wi-Fi traffic within the selected adapters. Older and unidentifiable traffic cannot be assigned to a named connection.");
        if ui
            .selectable_label(
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
            .max_height(120.0)
            .auto_shrink([true, true])
            .show(ui, |ui| {
                for network in &self.data.snapshot.wifi_networks {
                    ui.push_id(&network.id, |ui| {
                        let name = wifi_display_name(&network.name);
                        if ui
                            .selectable_label(
                                self.settings.filter.wifi_network.as_deref()
                                    == Some(network.id.as_str()),
                                name,
                            )
                            .clicked()
                        {
                            self.settings.filter.wifi_network = Some(network.id.clone());
                            changed = true;
                        }
                    });
                }
                if ui
                    .selectable_label(
                        self.settings.filter.wifi_network.as_deref() == Some(WIFI_UNKNOWN),
                        "Unidentified Wi-Fi",
                    )
                    .clicked()
                {
                    self.settings.filter.wifi_network = Some(WIFI_UNKNOWN.to_owned());
                    changed = true;
                }
            });
        changed
    }
    fn adapter_filter_caption(&self) -> (String, String) {
        let adapters = if self.settings.filter.adapters.is_empty() {
            "All adapters".to_owned()
        } else if self.settings.filter.adapters.contains("__none__") {
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
            (
                format!("Wi-Fi: {name}"),
                format!(
                    "Wi-Fi: {name} · {adapters}. Only traffic on this connection within the selected adapters is shown."
                ),
            )
        } else {
            let label = if self.settings.filter.adapters.is_empty() {
                "All".to_owned()
            } else if self.settings.filter.adapters.contains("__none__") {
                "None".to_owned()
            } else {
                self.settings.filter.adapters.len().to_string()
            };
            (
                format!("Adapters: {label}"),
                format!("{adapters} · All Wi-Fi connections"),
            )
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui, apps: bool) {
        let before = (
            self.settings.filter.seconds,
            self.settings.filter.time_range,
            self.settings.filter.scope,
        );
        let mut adapters_changed = false;
        surface(ui).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.spacing_mut().button_padding = Vec2::new(9.0, 5.0);
            ui.spacing_mut().interact_size.y = 28.0;
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let widths = [126.0, 148.0, 144.0];
                    let label = period_caption(&self.settings.filter);
                    filter_dropdown(
                        ui,
                        if apps { "app-period" } else { "overall-period" },
                        widths[0],
                        format!("Period: {label}"),
                        false,
                        |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.spacing_mut().button_padding.y = 2.0;
                            ui.spacing_mut().interact_size.y = 22.0;
                            if period::choice(
                                ui,
                                self.settings.filter.month_to_date(),
                                "Since beginning of month",
                            )
                            .clicked()
                            {
                                self.settings.filter.seconds = PERIOD_MONTH_TO_DATE;
                                self.settings.filter.time_range = None;
                            }
                            if period::choice(ui, self.settings.filter.all_time(), "All time")
                                .clicked()
                            {
                                self.settings.filter.seconds = 0;
                                self.settings.filter.time_range = None;
                            }
                            if period::choice(ui, label == "Custom", "Custom…").clicked() {
                                self.custom_period =
                                    Some(CustomPeriod::from_filter(&self.settings.filter));
                            }
                            ui.separator();
                            for &(seconds, _, label) in PERIODS.iter().filter(|p| p.0 > 0) {
                                let selected = self.settings.filter.time_range.is_none()
                                    && self.settings.filter.seconds == seconds;
                                if period::choice(ui, selected, label).clicked() {
                                    self.settings.filter.seconds = seconds;
                                    self.settings.filter.time_range = None;
                                }
                            }
                        },
                    );
                    let (adapter_caption, adapter_tooltip) = self.adapter_filter_caption();
                    filter_dropdown(
                        ui,
                        if apps {
                            "app-adapters"
                        } else {
                            "overall-adapters"
                        },
                        widths[1],
                        adapter_caption,
                        true,
                        |ui| {
                            adapters_changed = self.adapter_choices(ui);
                        },
                    )
                    .on_hover_text(adapter_tooltip);
                    if apps {
                        let label = match self.settings.filter.scope {
                            Scope::All => "All",
                            Scope::Lan => "LAN",
                            Scope::Wan => "WAN",
                            Scope::Unknown => "Other",
                        };
                        filter_dropdown(
                            ui,
                            "app-networks",
                            widths[2],
                            format!("Networks: {label}"),
                            false,
                            |ui| {
                                for (scope, label) in [
                                    (Scope::All, "All networks"),
                                    (Scope::Lan, "LAN"),
                                    (Scope::Wan, "WAN"),
                                    (Scope::Unknown, "Other"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.settings.filter.scope,
                                        scope,
                                        label,
                                    );
                                }
                            },
                        );
                        let search = ui.available_width().floor().max(120.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.app_search)
                                .hint_text("Search apps or folders")
                                .desired_width(search)
                                .margin(egui::Margin::symmetric(8, 5)),
                        );
                    }
                },
            );
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
        ui.add_space(4.0);
    }
    fn export(&mut self) {
        let name = format!(
            "network-usage-{}.csv",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("CSV", &["csv"])
            .set_file_name(name)
            .save_file()
        {
            let mut filter = self.settings.filter.clone();
            if filter.source == Source::Transport {
                if let Some(app) = &self.selected_app {
                    filter.apps = HashSet::from([app.clone()]);
                } else if !self.app_search.is_empty() {
                    filter.apps = self.app_totals().iter().map(|a| a.path.clone()).collect();
                    if filter.apps.is_empty() {
                        filter.apps.insert("__none__".into());
                    }
                }
            }
            let _ = self.worker.tx.send(Command::Export(path, filter));
            self.data.notice = "Preparing export…".into();
        }
    }
    fn source_toggle(&mut self, ui: &mut egui::Ui) {
        for (source, label) in [
            (Source::Adapter, "Overall Traffic"),
            (Source::Transport, "App traffic"),
        ] {
            if ui
                .add_sized(
                    [132.0, 32.0],
                    egui::Button::new(label).selected(self.settings.filter.source == source),
                )
                .clicked()
            {
                self.switch_source(source);
            }
        }
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        if self.settings.filter.source == Source::Transport {
            self.applications(ui);
        } else {
            self.adapter_overview(ui);
        }
    }
    fn page_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.page.label()).size(26.0).strong());
            if self.page == Page::Overview {
                ui.add_space(12.0);
                self.source_toggle(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Export CSV").clicked() {
                        self.export();
                    }
                });
            }
        });
        let description = match self.page {
            Page::Overview => None,
            Page::Connections => Some("Live TCP connections and UDP sockets across your PC."),
            Page::Adapters => Some("Your network interfaces, connection status and live speeds."),
            Page::Settings => Some("Appearance, background monitoring and saved history."),
        };
        if let Some(description) = description {
            ui.label(RichText::new(description).size(14.0).weak());
        }
        ui.add_space(8.0);
    }
    fn adapter_overview(&mut self, ui: &mut egui::Ui) {
        self.toolbar(ui, false);
        let (live_down, live_up) = self.live_rates(Source::Adapter);
        let down_label = format!("Down {}", self.rate(live_down));
        let up_label = format!("Up {}", self.rate(live_up));
        // The compact summary has a fixed height; the chart uses the remaining space.
        let plot_height =
            (ui.available_height() - SUMMARY_CONTENT_HEIGHT - 26.0 - 12.0 - 26.0 - 46.0).max(100.0);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.show_down,
                    RichText::new(&down_label)
                        .size(15.0)
                        .strong()
                        .color(download_color(ui)),
                );
                ui.add_space(8.0);
                ui.checkbox(
                    &mut self.show_up,
                    RichText::new(&up_label)
                        .size(15.0)
                        .strong()
                        .color(upload_color(ui)),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Chart options").clicked() {
                        self.show_graph_options = !self.show_graph_options;
                    }
                });
            });
            let points = self.chart_points();
            self.graph.show(
                ui,
                &points,
                self.chart_window(),
                plot_height,
                self.show_down,
                self.show_up,
                self.settings.bits,
                self.log,
            );
        });
        ui.add_space(4.0);
        self.overall_summary(ui);
    }
    fn overall_summary(&self, ui: &mut egui::Ui) -> egui::Rect {
        surface(ui).show(ui,|ui| {
            let (rect,_)=ui.allocate_exact_size(Vec2::new(ui.available_width(),SUMMARY_CONTENT_HEIGHT),egui::Sense::hover());
            let parts=summary_columns(rect);
            for (i,label,value,color) in [(0,"Downloaded",self.data.history.down,download_color(ui)),(1,"Uploaded",self.data.history.up,upload_color(ui))] {
                let totals=parts[0];
                let (x,y,width)=if totals.width()>=240.0 {
                    let width=(totals.width()-14.0)/2.0;
                    (totals.left()+i as f32*(width+14.0),totals.top()+25.0,width)
                }else{(totals.left(),totals.top()+i as f32*54.0,totals.width())};
                row_text(ui,egui::Rect::from_min_size(egui::pos2(x,y),Vec2::new(width,18.0)),RichText::new(label).size(13.0).weak());
                row_text(ui,egui::Rect::from_min_size(egui::pos2(x,y+20.0),Vec2::new(width,29.0)),RichText::new(compact_bytes(value)).size(22.0).strong().color(color));
            }
            for pair in parts.windows(2) {
                let x=(pair[0].right()+pair[1].left())/2.0;
                ui.painter().vline(x,rect.top()+2.0..=rect.bottom()-2.0,egui::Stroke::new(1.0,widgets::border(ui)));
            }
            let destination_rect=parts[1].translate(Vec2::new(0.0,8.0));
            destination_breakdown(ui,destination_rect,&self.data.classified);
            let apps=parts[2];
            row_text(ui,egui::Rect::from_min_max(apps.min,egui::pos2(apps.right(),apps.top()+18.0)),RichText::new("Top 5 apps").size(14.0).strong());
            widgets::text_at(ui,egui::Rect::from_min_max(apps.min,egui::pos2(apps.right(),apps.top()+18.0)),RichText::new("Total").size(12.0).weak(),egui::Align::Max);
            for (i,app) in self.data.top_apps.iter().take(5).enumerate() {
                let top=apps.top()+20.0+i as f32*16.8;
                let row=egui::Rect::from_min_size(egui::pos2(apps.left(),top),Vec2::new(apps.width(),16.8));
                let name=self.label(&app.app);
                row_text(ui,egui::Rect::from_min_size(row.min,Vec2::new(16.0,16.8)),RichText::new(format!("{}",i+1)).size(12.0).weak());
                row_text(ui,egui::Rect::from_min_max(row.min+Vec2::new(20.0,0.0),egui::pos2(row.right()-78.0,row.bottom())),RichText::new(&name).size(13.0));
                widgets::text_at(ui,egui::Rect::from_min_max(egui::pos2(row.right()-74.0,row.top()),row.max),RichText::new(compact_bytes(app.down.saturating_add(app.up))).size(13.0).strong(),egui::Align::Max);
                ui.interact(row,ui.make_persistent_id(("summary-app",&app.app)),egui::Sense::hover()).on_hover_text(format!("{name}\n{}\nDownloaded: {}\nUploaded: {}\nCaptured TCP/UDP app traffic in the selected period and adapters.",app.app,format_bytes(app.down as f64),format_bytes(app.up as f64)));
            }
            if self.data.top_apps.is_empty() {
                row_text(ui,egui::Rect::from_min_max(apps.min+Vec2::new(0.0,37.0),apps.max),RichText::new("No app traffic recorded").size(13.0).weak());
            }
        }).response.rect
    }
    fn chart_points(&self) -> Vec<Point> {
        self.frozen
            .as_ref()
            .map(|f| f.points.clone())
            .unwrap_or_else(|| self.data.history.points.clone())
    }
    fn chart_window(&self) -> (i64, i64) {
        if let Some(frozen) = &self.frozen {
            return frozen.window;
        }
        let h = &self.data.history;
        if h.window_end > h.window_start {
            return (h.window_start, h.window_end);
        }
        let now = chrono::Utc::now().timestamp();
        self.settings.filter.time_bounds(now)
    }
    fn live_rates(&self, source: Source) -> (f64, f64) {
        let mut f = self.settings.filter.clone();
        f.source = source;
        f.search.clear();
        let (down, up) = self
            .data
            .snapshot
            .live_rows
            .iter()
            .filter(|r| f.matches(r))
            .fold((0u64, 0u64), |(d, u), r| {
                (d.saturating_add(r.down), u.saturating_add(r.up))
            });
        let interval = self.data.snapshot.interval.max(0.001);
        (down as f64 / interval, up as f64 / interval)
    }
    fn rate(&self, value: f64) -> String {
        if self.settings.bits {
            format!("{}bit/s", format_number(value * 8.0))
        } else {
            format!("{}/s", format_bytes(value))
        }
    }
    fn label(&self, path: &str) -> String {
        self.data
            .snapshot
            .app_labels
            .get(path)
            .filter(|name| !name.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| app_name(path))
    }
    fn app_totals(&mut self) -> Arc<Vec<AppTotal>> {
        let key = (
            self.data.revision,
            self.data.history_generation,
            self.filter_revision,
            self.app_search.clone(),
        );
        if self.app_cache_key.as_ref() == Some(&key) {
            return self.app_cache.clone();
        }
        let mut totals = BTreeMap::<String, AppTotal>::new();
        for r in &self.data.history.rows {
            let t = totals.entry(r.app.clone()).or_default();
            t.path = r.app.clone();
            t.down += r.down;
            t.up += r.up;
        }
        for r in &self.data.snapshot.live_rows {
            if !self.settings.filter.matches(r) {
                continue;
            }
            let t = totals.entry(r.app.clone()).or_default();
            t.path = r.app.clone();
            t.live_down += r.down;
            t.live_up += r.up;
        }
        let search = self.app_search.to_lowercase();
        let mut totals: Vec<_> = totals
            .into_values()
            .filter_map(|mut t| {
                t.name = self.label(&t.path);
                if search.is_empty()
                    || format!("{} {}", t.name, t.path)
                        .to_lowercase()
                        .contains(&search)
                {
                    Some(t)
                } else {
                    None
                }
            })
            .collect();
        totals.sort_by(|a, b| {
            (b.down.saturating_add(b.up))
                .cmp(&a.down.saturating_add(a.up))
                .then(a.path.cmp(&b.path))
        });
        self.app_cache = Arc::new(totals);
        self.app_cache_key = Some(key);
        self.app_cache.clone()
    }
    fn traffic_breakdown(&mut self, totals: &[AppTotal]) -> Arc<Breakdown> {
        let key = (
            self.data.revision,
            self.data.history_generation,
            self.filter_revision,
            self.app_search.clone(),
            self.selected_app.clone(),
        );
        if self.breakdown_cache_key.as_ref() != Some(&key) {
            let apps = totals.iter().map(|t| t.path.clone()).collect();
            self.breakdown_cache = Arc::new(breakdown(
                &self.data.history.rows,
                &apps,
                self.selected_app.as_deref(),
                &self.data.snapshot.host_labels,
            ));
            self.breakdown_cache_key = Some(key);
        }
        self.breakdown_cache.clone()
    }
    fn applications(&mut self, ui: &mut egui::Ui) {
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
        // Keep a compact period summary visible beneath all three lists.
        let height = (ui.available_height() - 70.0).max(100.0);
        let mut visible_hosts = Vec::new();
        widgets::three_columns(ui, |cols| {
            for col in cols.iter_mut() {
                col.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
            }
            surface(&cols[0]).inner_margin(10).show(&mut cols[0], |ui| {
                ui.set_height((height - 22.0).max(100.0));
                ui.set_width(ui.available_width());
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
                                self.selected_app = if self.selected_app.as_deref() == Some(&t.path)
                                {
                                    None
                                } else {
                                    Some(t.path.clone())
                                };
                            }
                        }
                        if totals.is_empty() {
                            ui.label("No matching app traffic yet.");
                        }
                    });
            });
            visible_hosts = surface(&cols[1])
                .inner_margin(10)
                .show(&mut cols[1], |ui| {
                    ui.set_height((height - 22.0).max(100.0));
                    ranked_list(ui, "Hosts", &data.hosts, "ranked-hosts", true)
                })
                .inner;
            surface(&cols[2]).inner_margin(10).show(&mut cols[2], |ui| {
                ui.set_height((height - 22.0).max(100.0));
                ranked_list(ui, "Traffic type", &data.types, "ranked-types", false);
            });
        });
        ui.add_space(6.0);
        surface(ui).inner_margin(10).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("All apps").size(13.0).weak());
                ui.add_space(18.0);
                ui.label(RichText::new("Downloaded").size(13.0).weak());
                ui.label(
                    RichText::new(compact_bytes(self.data.history.down))
                        .size(16.0)
                        .strong()
                        .color(download_color(ui)),
                );
                ui.add_space(18.0);
                ui.label(RichText::new("Uploaded").size(13.0).weak());
                ui.label(
                    RichText::new(compact_bytes(self.data.history.up))
                        .size(16.0)
                        .strong()
                        .color(upload_color(ui)),
                );
                ui.add_space(20.0);
                traffic_distribution_bar(ui, self.data.history.down, self.data.history.up);
            });
        }).response.on_hover_text(
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
    fn connections(&mut self, ui: &mut egui::Ui) {
        let search = self.connection_search.to_lowercase();
        let rows: Vec<_> = self
            .data
            .snapshot
            .connections
            .iter()
            .filter_map(|c| {
                let name = self.label(&c.app);
                (search.is_empty()
                    || format!(
                        "{name} {} {} {} {} {} {}",
                        c.app, c.pid, c.local, c.remote, c.state, c.country
                    )
                    .to_lowercase()
                    .contains(&search))
                .then_some((c, name))
            })
            .collect();
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.connection_search)
                        .hint_text("Search apps, addresses, countries or status")
                        .desired_width((ui.available_width() - 90.0).max(160.0))
                        .margin(egui::Margin::symmetric(10, 7)),
                );
                ui.label(
                    RichText::new(format!(
                        "{} of {}",
                        rows.len(),
                        self.data.snapshot.connections.len()
                    ))
                    .size(13.5)
                    .weak(),
                );
            });
        });
        ui.label(
            RichText::new(
                "Refreshes every 5 seconds · UDP destinations are in Overview → App traffic",
            )
            .size(13.0)
            .weak(),
        );
        let height = ui.available_height();
        surface(ui).show(ui, |ui| {
            ui.set_height((height - 26.0).max(90.0));
            ui.set_width(ui.available_width());
            let (header, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), 30.0), egui::Sense::hover());
            let columns = connection_columns(header);
            for (rect, title) in columns.into_iter().zip([
                "Application",
                "Protocol",
                "Remote endpoint",
                "Local endpoint",
                "Country",
                "Status",
            ]) {
                row_text(ui, rect, RichText::new(title).size(12.5).strong().weak());
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("live-connections")
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show_rows(ui, 60.0, rows.len(), |ui, range| {
                    for (c, name) in &rows[range] {
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 60.0),
                            egui::Sense::hover(),
                        );
                        let cells = connection_columns(rect);
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 4, widgets::row_hover(ui));
                        }
                        row_text(
                            ui,
                            egui::Rect::from_min_max(
                                cells[0].min + Vec2::new(0.0, 7.0),
                                egui::pos2(cells[0].right(), rect.top() + 28.0),
                            ),
                            RichText::new(name).size(15.0).strong(),
                        );
                        row_text(
                            ui,
                            egui::Rect::from_min_max(
                                cells[0].min + Vec2::new(0.0, 32.0),
                                egui::pos2(cells[0].right(), rect.bottom() - 4.0),
                            ),
                            RichText::new(format!(
                                "PID {} · {}",
                                c.pid,
                                executable_directory(&c.app)
                            ))
                            .size(12.5)
                            .weak(),
                        );
                        for (i, value) in [
                            (1, c.protocol.as_str()),
                            (2, c.remote.as_str()),
                            (3, c.local.as_str()),
                            (4, c.country.as_str()),
                            (5, c.state.as_str()),
                        ] {
                            let value = if value.is_empty() { "—" } else { value };
                            let text = RichText::new(value).size(if i == 5 { 12.5 } else { 13.0 });
                            let text = if i == 2 || i == 3 {
                                text.monospace()
                            } else {
                                text
                            };
                            let text = if i == 5 && c.state.eq_ignore_ascii_case("Established") {
                                text.color(download_color(ui))
                            } else {
                                text
                            };
                            row_text(
                                ui,
                                egui::Rect::from_min_max(
                                    cells[i].min + Vec2::new(0.0, 20.0),
                                    egui::pos2(cells[i].right(), rect.bottom()),
                                ),
                                text,
                            );
                        }
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom(),
                            egui::Stroke::new(0.6, widgets::border(ui)),
                        );
                        response.on_hover_text(format!(
                            "{name}\n{}\nPID {} · {}\nRemote: {}\nLocal: {}\n{} · {}",
                            c.app, c.pid, c.protocol, c.remote, c.local, c.country, c.state
                        ));
                    }
                    if rows.is_empty() {
                        ui.label("No connections match your search.");
                    }
                });
        });
    }
    fn adapters(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            badge(
                ui,
                &format!("{} adapters", self.data.snapshot.adapters.len()),
                ui.visuals().text_color(),
            );
            badge(
                ui,
                &format!(
                    "{} connected",
                    self.data.snapshot.adapters.iter().filter(|a| a.up).count()
                ),
                download_color(ui),
            );
        });
        ui.add_space(4.0);
        let mut view_adapter = None;
        for a in &self.data.snapshot.adapters {
            surface(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let title_width = (ui.available_width() - 230.0).max(100.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(title_width, 32.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(title_width);
                            ui.add(
                                egui::Label::new(RichText::new(&a.name).size(17.0).strong())
                                    .truncate(),
                            )
                            .on_hover_text(&a.name);
                        },
                    );
                    ui.allocate_ui_with_layout(
                        Vec2::new(110.0, 32.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(110.0);
                            badge(
                                ui,
                                if a.up { "Connected" } else { "Disconnected" },
                                if a.up {
                                    download_color(ui)
                                } else {
                                    ui.visuals().weak_text_color()
                                },
                            );
                        },
                    );
                    if ui
                        .add_sized([100.0, 32.0], egui::Button::new("View traffic"))
                        .clicked()
                    {
                        view_adapter = Some(a.id.clone());
                    }
                });
                ui.add(
                    egui::Label::new(RichText::new(&a.description).size(13.5).weak()).truncate(),
                )
                .on_hover_text(&a.description);
                ui.add_space(3.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(if a.physical {
                            "Physical adapter"
                        } else {
                            "Virtual / VPN adapter"
                        })
                        .size(13.0)
                        .weak(),
                    );
                    let row = self
                        .data
                        .snapshot
                        .live_rows
                        .iter()
                        .find(|r| r.source == "adapter" && r.adapter_id == a.id);
                    let interval = self.data.snapshot.interval.max(0.001);
                    ui.colored_label(
                        download_color(ui),
                        format!(
                            "Down {}",
                            self.rate(row.map_or(0, |r| r.down) as f64 / interval)
                        ),
                    );
                    ui.colored_label(
                        upload_color(ui),
                        format!(
                            "Up {}",
                            self.rate(row.map_or(0, |r| r.up) as f64 / interval)
                        ),
                    );
                });
                if !a.addresses.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(
                                a.addresses
                                    .iter()
                                    .map(|(ip, prefix)| format!("{ip}/{prefix}"))
                                    .collect::<Vec<_>>()
                                    .join("   ·   "),
                            )
                            .monospace()
                            .size(13.0),
                        )
                        .wrap(),
                    );
                }
            });
            ui.add_space(4.0);
        }
        if let Some(id) = view_adapter {
            self.page = Page::Overview;
            self.settings.filter.adapters = HashSet::from([id]);
            self.settings.filter.source = Source::Adapter;
            self.update_filter();
        }
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(ui, "Appearance", "Choose a palette and speed display.");
            let old = self.settings.theme;
            ui.horizontal_wrapped(|ui| {
                for theme in [Theme::Dark, Theme::Light, Theme::Nord, Theme::Mocha] {
                    ui.selectable_value(&mut self.settings.theme, theme, theme.label());
                }
            });
            if old != self.settings.theme {
                self.settings.theme.apply(ui.ctx());
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Interface size");
                let before = self.settings.ui_scale;
                ui.add(
                    egui::Slider::new(&mut self.settings.ui_scale, 0.8..=1.1)
                        .step_by(0.05)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                );
                if ui.small_button("Reset").clicked() {
                    self.settings.ui_scale = 0.9;
                }
                if before != self.settings.ui_scale {
                    ui.ctx().set_zoom_factor(self.settings.ui_scale);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Live speed units");
                ui.selectable_value(&mut self.settings.bits, false, "Bytes/s");
                ui.selectable_value(&mut self.settings.bits, true, "Bits/s");
            });
            ui.label(
                RichText::new("Usage totals and CSV exports always use bytes.")
                    .size(13.5)
                    .weak(),
            );
        });
        ui.add_space(4.0);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(
                ui,
                "Background monitoring",
                "Keep collecting when the window is closed.",
            );
            self.startup.poll();
            let mut enabled = self.startup.enabled;
            let startup_toggle = ui.add_enabled(
                self.startup.available && !self.startup.busy,
                egui::Checkbox::new(&mut enabled, "Launch when I sign in to Windows"),
            );
            if startup_toggle.changed() {
                self.startup.set_enabled(enabled);
            }
            ui.label(
                RichText::new("Starts in the tray and monitors in the background.")
                    .size(13.5)
                    .weak(),
            );
            if self.startup.busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Updating startup settings…");
                });
            }
            if self.startup.enabled && self.startup.different_copy {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new("Startup points to another copy of the app.")
                            .size(13.0)
                            .weak(),
                    );
                    if ui
                        .add_enabled(!self.startup.busy, egui::Button::new("Use this copy"))
                        .clicked()
                    {
                        self.startup.set_enabled(true);
                    }
                });
            }
            if !self.startup.error.is_empty() {
                widgets::message(ui, &self.startup.error, true, false);
            }
            ui.separator();
            ui.add_enabled(
                self.tray.is_some(),
                egui::Checkbox::new(
                    &mut self.settings.close_to_tray,
                    "Close window to system tray",
                ),
            );
            ui.label(
                RichText::new(
                    "Click the tray icon to reopen. Tray → Exit stops monitoring immediately.",
                )
                .size(13.5)
                .weak(),
            );
        });
        ui.add_space(4.0);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(
                ui,
                "Monitoring",
                "Built-in Windows capture. Administrator access is requested on launch.",
            );
            ui.horizontal(|ui| {
                badge(
                    ui,
                    if self.data.snapshot.etw_active {
                        "Detailed capture active"
                    } else {
                        "Detailed capture unavailable"
                    },
                    if self.data.snapshot.etw_active {
                        download_color(ui)
                    } else {
                        upload_color(ui)
                    },
                );
                if ui.button("Retry capture").clicked() {
                    let _ = self.worker.tx.send(Command::RetryCapture);
                }
            });
            if !self.data.snapshot.etw_active {
                ui.label(&self.data.snapshot.capture_status);
            }
            ui.separator();
            ui.columns(4, |cols| {
                for (col, (label, value)) in cols.iter_mut().zip([
                    ("Events", self.data.snapshot.parsed_events),
                    ("Decode errors", self.data.snapshot.parse_errors),
                    ("Queue drops", self.data.snapshot.queue_drops),
                    ("Kernel losses", self.data.snapshot.kernel_losses),
                ]) {
                    col.label(RichText::new(label).size(13.0).weak());
                    col.label(RichText::new(format_count(value)).size(20.0).strong());
                }
            });
        });
        ui.add_space(4.0);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(
                ui,
                "IP countries",
                "Optional offline lookup using a GeoLite2 / GeoIP2 .mmdb database.",
            );
            let selected = self
                .settings
                .geo
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "No database selected".into());
            ui.add(egui::Label::new(selected.clone()).truncate())
                .on_hover_text(selected);
            ui.horizontal(|ui| {
                if ui.button("Choose database…").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("MaxMind Country or City database", &["mmdb"])
                        .pick_file()
                {
                    self.settings.geo = Some(path.clone());
                    let _ = self.worker.tx.send(Command::Geo(Some(path)));
                }
                if self.settings.geo.is_some() && ui.button("Disable lookup").clicked() {
                    self.settings.geo = None;
                    let _ = self.worker.tx.send(Command::Geo(None));
                }
            });
            ui.label(
                RichText::new(
                    "Locations are approximate and can represent a VPN exit or hosting provider.",
                )
                .size(13.5)
                .weak(),
            );
        });
        ui.add_space(4.0);
        surface(ui).show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_title(
                ui,
                "History & exports",
                "Usage history is kept indefinitely. CSV follows the current Overview filters.",
            );
            ui.add(
                egui::Label::new(
                    RichText::new(self.data_dir.display().to_string())
                        .monospace()
                        .size(13.0),
                )
                .truncate(),
            )
            .on_hover_text(self.data_dir.display().to_string());
            if ui.button("Export selected usage to CSV…").clicked() {
                self.export();
            }
            ui.label(
                RichText::new("Exported timestamps use UTC.")
                    .size(13.5)
                    .weak(),
            );
        });
        ui.add_space(4.0);
        surface(ui).show(ui,|ui| {
            ui.set_width(ui.available_width());
            egui::CollapsingHeader::new(RichText::new("How measurements work").size(15.0).strong()).show(ui,|ui| {
                ui.label("Adapters count interface bytes. App traffic counts observed TCP/UDP transfers. These views measure different layers and are never added together.");
                ui.label("LAN includes private, loopback, link-local, multicast and on-link addresses. WAN covers public destinations; reserved/shared addresses are Other.");
                ui.label("App traffic is assigned by local IP. Ambiguous matches stay Unassigned. VPN traffic can appear at multiple layers, so All adapters is a sum of observations rather than a guaranteed unique internet total.");
                ui.label("Traffic types are inferred from ports. Host names use Windows reverse DNS and may identify shared infrastructure rather than the visited website.");
                ui.label("Monitoring continues while this desktop app is running. A Windows service and start-at-login are not included yet.");
            });
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new("OpenNetworkMonitor 0.3.4 · MIT · Rust + egui")
                .size(13.0)
                .weak(),
        );
    }
}

impl MonitorApp {
    fn draw(&mut self, root: &mut egui::Ui) -> egui::Rect {
        let ctx = root.ctx().clone();
        if self.show_graph_options {
            let mut open = true;
            egui::Window::new("Chart options")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.checkbox(&mut self.log, "Logarithmic scale");
                    if ui
                        .selectable_label(
                            self.paused,
                            if self.paused {
                                "Resume chart"
                            } else {
                                "Freeze chart"
                            },
                        )
                        .clicked()
                    {
                        self.paused = !self.paused;
                        self.frozen = if self.paused {
                            Some(FrozenChart {
                                points: self.chart_points(),
                                window: self.chart_window(),
                            })
                        } else {
                            None
                        };
                    }
                    ui.weak("Freezing the chart keeps monitoring active.");
                });
            self.show_graph_options = open;
        }
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(widgets::NAV_WIDTH)
            .frame(
                egui::Frame::new()
                    .fill(root.visuals().extreme_bg_color)
                    .inner_margin(14),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    let (logo, _) = ui.allocate_exact_size(Vec2::splat(31.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(logo, 7, ui.visuals().selection.bg_fill);
                    for (x, h) in [(8.0, 7.0), (15.0, 16.0), (22.0, 11.0)] {
                        ui.painter().line_segment(
                            [
                                logo.min + Vec2::new(x, 23.0),
                                logo.min + Vec2::new(x, 23.0 - h),
                            ],
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        );
                    }
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("OpenNetwork").size(13.5).strong());
                        ui.label(RichText::new("Monitor").size(13.5).weak());
                    });
                });
                ui.add_space(24.0);
                for (i, page) in [
                    Page::Overview,
                    Page::Connections,
                    Page::Adapters,
                    Page::Settings,
                ]
                .into_iter()
                .enumerate()
                {
                    if widgets::navigation(ui, page.label(), i, self.page == page).clicked() {
                        self.page = page;
                        self.selected_app = None;
                    }
                    ui.add_space(3.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(RichText::new("History kept indefinitely").size(12.5).weak());
                    ui.label(
                        RichText::new(format!(
                            "{} connections",
                            self.data.snapshot.connections.len()
                        ))
                        .size(13.5)
                        .weak(),
                    );
                    ui.label(
                        RichText::new(format!("{} adapters", self.data.snapshot.adapters.len()))
                            .size(13.5)
                            .weak(),
                    );
                    ui.add_space(3.0);
                    badge(
                        ui,
                        if self.data.snapshot.etw_active {
                            "Monitoring active"
                        } else {
                            "Capture unavailable"
                        },
                        if self.data.snapshot.etw_active {
                            download_color(ui)
                        } else {
                            upload_color(ui)
                        },
                    );
                    ui.separator();
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(root.visuals().panel_fill)
                    .inner_margin(widgets::PAGE_MARGIN),
            )
            .show(root, |ui| {
                self.page_header(ui);
                if !self.view_warning.is_empty() {
                    widgets::message(ui,&self.view_warning,true,false);
                }
                if !self.data.notice.is_empty() && widgets::message(ui,&self.data.notice,false,true) {
                    self.data.notice.clear();
                    if let Ok(mut state)=self.worker.shared.lock(){state.notice.clear();}
                }
                for warning in &self.data.snapshot.warnings {
                    widgets::message(ui,warning,true,false);
                }
                if !self.data.snapshot.etw_active && !self.data.snapshot.capture_status.is_empty() {
                    widgets::message(ui,&self.data.snapshot.capture_status,true,false);
                }
                if self.data.snapshot.parse_errors
                    + self.data.snapshot.queue_drops
                    + self.data.snapshot.kernel_losses
                    > 0
                {
                    widgets::message(ui,"Some traffic events were lost. App totals may be incomplete; see Settings.",true,false);
                }
                ui.add_space(4.0);
                match self.page {
                    Page::Overview => self.overview(ui),
                    Page::Connections => self.connections(ui),
                    Page::Adapters | Page::Settings => {
                        egui::ScrollArea::vertical()
                            .id_salt(self.page.label())
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if self.page == Page::Adapters {
                                    self.adapters(ui);
                                } else {
                                    self.settings(ui);
                                }
                            });
                    }
                }
            }).response.rect
    }
}

impl eframe::App for MonitorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.settings.wide_summary_initialized {
            let viewport = ctx.input(|i| i.viewport().clone());
            if let Some(rect) = viewport.inner_rect {
                if viewport.maximized != Some(true) {
                    let size = initial_wide_size(rect.size(), viewport.monitor_size);
                    if size.x > rect.width() + 1.0 {
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                    }
                }
                self.settings.wide_summary_initialized = true;
            }
        }
        if let Ok(data) = self.worker.shared.lock() {
            if data.revision != self.data.revision
                || data.history_generation != self.data.history_generation
                || data.notice != self.data.notice
            {
                self.data = data.clone();
            }
            if self.data.history_revision != self.filter_revision {
                self.data.history = Arc::default();
                self.data.classified = ScopeTotals::default();
                self.data.top_apps.clear();
            }
        }
        let exiting = self
            .tray
            .as_ref()
            .is_some_and(|t| t.exit.load(Ordering::Acquire));
        if ctx.input(|i| i.viewport().close_requested())
            && !exiting
            && self.settings.close_to_tray
            && self.tray.is_some()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.draw(root);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Remove the tray icon promptly while the worker saves its final data.
        drop(self.tray.take());
        self.worker.shutdown();
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
    }
}
fn install_fonts(ctx: &egui::Context) {
    if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("Segoe UI".into(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Segoe UI".into());
        ctx.set_fonts(fonts);
    }
}
fn wifi_display_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    if clean.trim().is_empty() {
        "Unnamed Wi-Fi".to_owned()
    } else {
        clean
    }
}
fn toggle_adapter(
    selected: &mut HashSet<String>,
    adapters: &[Adapter],
    id: &str,
    enabled: bool,
    include_unassigned: bool,
) {
    if selected.is_empty() {
        *selected = adapters.iter().map(|a| a.id.clone()).collect();
        if include_unassigned {
            selected.insert("unassigned".into());
        }
    }
    selected.remove("__none__");
    if enabled {
        selected.insert(id.into());
    } else {
        selected.remove(id);
    }
    if selected.is_empty() {
        selected.insert("__none__".into());
    }
}
fn connection_columns(rect: egui::Rect) -> [egui::Rect; 6] {
    let fractions = [0.24, 0.08, 0.22, 0.22, 0.10, 0.14];
    let width = (rect.width() - 50.0).max(1.0);
    let mut x = rect.left() + 2.0;
    fractions.map(|fraction| {
        let cell = egui::Rect::from_min_size(
            egui::pos2(x, rect.top()),
            Vec2::new(width * fraction, rect.height()),
        );
        x += cell.width() + 8.0;
        cell
    })
}
fn filter_dropdown(
    ui: &mut egui::Ui,
    id: &str,
    width: f32,
    label: String,
    multiple: bool,
    contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, 28.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_width(width);
            egui::ComboBox::from_id_salt(id)
                .width(width)
                .height(if multiple {
                    360.0
                } else if id.ends_with("period") {
                    440.0
                } else {
                    ui.spacing().combo_height
                })
                .selected_text(label)
                .wrap_mode(egui::TextWrapMode::Truncate)
                .close_behavior(if multiple {
                    egui::PopupCloseBehavior::CloseOnClickOutside
                } else {
                    egui::PopupCloseBehavior::CloseOnClick
                })
                .show_ui(ui, contents)
                .response
        },
    )
    .inner
}
const SUMMARY_CONTENT_HEIGHT: f32 = 104.0;
fn summary_columns(rect: egui::Rect) -> [egui::Rect; 3] {
    let gap = 24.0;
    let width = rect.width() - gap * 2.0;
    let first = (width * 0.26).floor();
    let second = (width * 0.34).floor();
    [
        egui::Rect::from_min_size(rect.min, Vec2::new(first, rect.height())),
        egui::Rect::from_min_size(
            rect.min + Vec2::new(first + gap, 0.0),
            Vec2::new(second, rect.height()),
        ),
        egui::Rect::from_min_max(
            rect.min + Vec2::new(first + second + 2.0 * gap, 0.0),
            rect.max,
        ),
    ]
}
fn initial_wide_size(current: Vec2, monitor: Option<Vec2>) -> Vec2 {
    let width = current.x.max(widgets::DEFAULT_WINDOW_SIZE[0]);
    let width = monitor.map_or(width, |m| {
        width.min((m.x - 32.0).max(widgets::MIN_WINDOW_SIZE[0]))
    });
    Vec2::new(width, current.y)
}
fn valid_ui_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.8, 1.1)
    } else {
        0.9
    }
}
const USAGE_ROW_HEIGHT: f32 = widgets::ROW_HEIGHT;
fn list_heading(
    ui: &mut egui::Ui,
    title: &str,
    count: usize,
    help: &str,
    clear_selection: bool,
) -> bool {
    let mut clear = false;
    ui.horizontal(|ui| {
        ui.set_height(24.0);
        ui.label(RichText::new(title).size(16.0).strong())
            .on_hover_text(help);
        badge(ui, &count.to_string(), ui.visuals().weak_text_color());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if clear_selection {
                clear = ui
                    .small_button("All apps")
                    .on_hover_text("Clear selected app")
                    .clicked();
            } else {
                ui.label(RichText::new("Total usage").size(12.0).weak());
            }
        });
    });
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(2.0);
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
                usage_row(ui, &row.label, &row.label, &row.subtitle, (row.down, row.up), None, None).on_hover_text(help);
            }
            if rows.is_empty() {
                ui.weak("No matching traffic yet.");
            }
            visible
        }).inner
}
fn row_text(ui: &mut egui::Ui, rect: egui::Rect, text: impl Into<egui::WidgetText>) {
    widgets::text_at(ui, rect, text, egui::Align::Min);
}
fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
/// The filled portions of the period's upload/download bar. An empty total
/// leaves the neutral track visible; f64 arithmetic avoids overflowing u64.
fn traffic_bar_segments(rect: egui::Rect, down: u64, up: u64) -> [Option<egui::Rect>; 2] {
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
    let width = ui.available_width().max(0.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 10.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    painter.rect_filled(rect, 5, widgets::border(ui));
    let [download, upload] = traffic_bar_segments(rect, down, up);
    if let Some(segment) = download {
        painter.rect_filled(
            segment,
            egui::CornerRadius {
                nw: 5,
                sw: 5,
                ne: if up == 0 { 5 } else { 0 },
                se: if up == 0 { 5 } else { 0 },
            },
            download_color(ui),
        );
    }
    if let Some(segment) = upload {
        painter.rect_filled(
            segment,
            egui::CornerRadius {
                nw: if down == 0 { 5 } else { 0 },
                sw: if down == 0 { 5 } else { 0 },
                ne: 5,
                se: 5,
            },
            upload_color(ui),
        );
    }
    response.on_hover_text(
        "Share of downloaded and uploaded bytes in the selected period, adapters, and network.",
    );
}

fn compact_bytes(value: u64) -> String {
    crate::model::format_compact_bytes(value)
}
fn usage_row(
    ui: &mut egui::Ui,
    identity: &str,
    name: &str,
    subtitle: &str,
    bytes: (u64, u64),
    selection: Option<bool>,
    icon: Option<egui::TextureId>,
) -> egui::Response {
    let (down, up) = bytes;
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), USAGE_ROW_HEIGHT),
        egui::Sense::hover(),
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id(identity),
        if selection.is_some() {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
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
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect.shrink2(Vec2::new(0.0, 2.0)),
            5,
            if selected {
                ui.visuals().selection.bg_fill
            } else {
                widgets::row_hover(ui)
            },
        );
        if selected {
            ui.painter().vline(
                rect.left() + 1.5,
                rect.top() + 10.0..=rect.bottom() - 10.0,
                egui::Stroke::new(3.0, ui.visuals().selection.stroke.color),
            );
        }
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            5,
            egui::Stroke::new(1.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    let left = rect.left() + 7.0;
    let right = rect.right() - 7.0;
    let top = rect.top();
    let text_left = if selection.is_some() {
        let icon_rect = egui::Rect::from_min_size(egui::pos2(left, top + 6.0), Vec2::splat(24.0));
        if let Some(texture) = icon {
            ui.painter().image(
                texture,
                icon_rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            // Neutral fallback for inaccessible, exited or not-yet-loaded executables.
            ui.painter().rect_filled(icon_rect, 4, widgets::border(ui));
            let glyph = icon_rect.shrink(6.0);
            let stroke = egui::Stroke::new(1.2, ui.visuals().text_color().gamma_multiply(0.75));
            ui.painter()
                .rect_stroke(glyph, 1, stroke, egui::StrokeKind::Inside);
            ui.painter()
                .hline(glyph.x_range(), glyph.top() + 3.0, stroke);
        }
        left + 30.0
    } else {
        left
    };
    let total = compact_bytes(down.saturating_add(up));
    let total_width = ui
        .painter()
        .layout_no_wrap(
            total.clone(),
            egui::FontId::proportional(16.0),
            ui.visuals().text_color(),
        )
        .size()
        .x
        + 2.0;
    row_text(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(text_left, top + 3.0),
            egui::pos2((right - total_width - 10.0).max(text_left), top + 22.0),
        ),
        RichText::new(name).size(15.0).strong(),
    );
    widgets::text_at(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(right - total_width, top + 2.0),
            egui::pos2(right, top + 23.0),
        ),
        RichText::new(total).size(16.0).strong(),
        egui::Align::Max,
    );
    row_text(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(text_left, top + 22.0),
            egui::pos2(right, top + 36.0),
        ),
        RichText::new(subtitle).size(11.5).weak(),
    );
    let middle = left + (right - left) * 0.53;
    row_text(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(left, top + 38.0),
            egui::pos2(middle - 3.0, top + 53.0),
        ),
        RichText::new(format!("Down {}", compact_bytes(down)))
            .size(11.5)
            .color(download_color(ui)),
    );
    row_text(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(middle, top + 38.0),
            egui::pos2(right, top + 53.0),
        ),
        RichText::new(format!("Up {}", compact_bytes(up)))
            .size(11.5)
            .color(upload_color(ui)),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(0.75, widgets::border(ui)),
    );
    if selection.is_some() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}
fn app_name(path: &str) -> String {
    executable_filename(path)
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn wifi_names_stay_single_line_without_changing_identity() {
        assert_eq!(wifi_display_name("home\nnetwork"), "home network");
        assert_eq!(wifi_display_name("\t"), "Unnamed Wi-Fi");
    }
    #[test]
    fn removing_an_adapter_does_not_add_an_invisible_sixth_selection() {
        let adapters = vec![
            Adapter {
                id: "a".into(),
                ..Default::default()
            },
            Adapter {
                id: "b".into(),
                ..Default::default()
            },
        ];
        let mut selected = HashSet::new();
        toggle_adapter(&mut selected, &adapters, "a", false, false);
        assert_eq!(selected, HashSet::from(["b".into()]));
        toggle_adapter(&mut selected, &adapters, "b", false, false);
        assert!(selected.contains("__none__"));
    }
}

#[cfg(test)]
mod compact_layout_tests {
    use super::*;
    fn fixture() -> MonitorApp {
        MonitorApp {
            worker: Worker::for_ui_test(),
            icons: IconCache::disabled(),
            startup: crate::startup::Controller::preview(),
            tray: None,
            data: Shared::default(),
            settings: Settings::default(),
            page: Page::Overview,
            graph: Graph,
            show_down: true,
            show_up: true,
            log: false,
            paused: false,
            frozen: None,
            filter_revision: 0,
            selected_app: None,
            connection_search: String::new(),
            app_search: String::new(),
            data_dir: PathBuf::new(),
            view_warning: String::new(),
            show_graph_options: false,
            custom_period: None,
            app_cache: Arc::default(),
            app_cache_key: None,
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
        }
    }
    #[test]
    fn adapter_dashboard_fits_without_scrolling_at_supported_window_sizes() {
        for (width, height) in [(960.0, 680.0), (1200.0, 820.0), (1440.0, 900.0)] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = fixture();
            let mut bottom = 0.0;
            let mut limit = 0.0;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(width, height),
                    )),
                    ..Default::default()
                },
                |root| {
                    egui::Panel::left("test-navigation")
                        .resizable(false)
                        .exact_size(178.0)
                        .show(root, |_| {});
                    egui::CentralPanel::default()
                        .frame(egui::Frame::new().inner_margin(16))
                        .show(root, |ui| {
                            app.page_header(ui);
                            ui.add_space(4.0);
                            limit = ui.max_rect().bottom();
                            app.adapter_overview(ui);
                            bottom = ui.min_rect().bottom();
                        });
                },
            );
            output.textures_delta.clear();
            assert!(
                bottom <= limit + 1.0,
                "Dashboard overflows at {width}x{height}: {bottom} > {limit}"
            );
        }
    }
    fn app_fixture() -> MonitorApp {
        let mut app = fixture();
        app.settings.filter.source = Source::Transport;
        app.data.history = Arc::new(History {
            rows: (0..80)
                .map(|i| UsageRow {
                    source: "transport".into(),
                    app: format!(r"C:\Apps\Long application directory {i}\plain{i}.exe"),
                    remote: format!("203.0.113.{}", i + 1),
                    traffic_type: if i % 2 == 0 { "HTTP" } else { "DNS" }.into(),
                    down: (80 - i) * 1000,
                    up: i * 100,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        });
        app.data.snapshot.app_labels.insert(
            r"C:\Apps\Long application directory 0\plain0.exe".into(),
            "Friendly Browser".into(),
        );
        app
    }
    fn app_frame(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        size: Vec2,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, f32, f32) {
        let mut bottom = 0.0;
        let mut limit = 0.0;
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            },
            |root| {
                egui::Panel::left("test-navigation")
                    .resizable(false)
                    .exact_size(178.0)
                    .show(root, |_| {});
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().inner_margin(16))
                    .show(root, |ui| {
                        app.page_header(ui);
                        ui.add_space(4.0);
                        limit = ui.max_rect().bottom();
                        app.overview(ui);
                        bottom = ui.min_rect().bottom();
                    });
            },
        );
        (out, bottom, limit)
    }
    fn text_positions(out: &egui::FullOutput) -> Vec<(String, egui::Pos2)> {
        fn collect(shape: &egui::Shape, into: &mut Vec<(String, egui::Pos2)>) {
            match shape {
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        collect(s, into);
                    }
                }
                egui::Shape::Text(t) => into.push((t.galley.job.text.clone(), t.pos)),
                _ => {}
            }
        }
        let mut all = Vec::new();
        for s in &out.shapes {
            collect(&s.shape, &mut all);
        }
        all
    }
    #[test]
    fn app_icon_sits_before_the_name_and_remains_part_of_the_clickable_row() {
        fn find_icon(shape: &egui::Shape, texture: egui::TextureId) -> Option<egui::Rect> {
            match shape {
                egui::Shape::Vec(shapes) => {
                    shapes.iter().find_map(|shape| find_icon(shape, texture))
                }
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
                _ => None,
            }
        }
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        let path = r"C:\Apps\Long application directory 0\plain0.exe";
        app.icons.insert_preview(
            &ctx,
            path,
            egui::ColorImage::new([2, 2], vec![Color32::WHITE; 4]),
        );
        let texture = app.icons.texture(&ctx, path).unwrap();
        let size = Vec2::new(1200.0, 820.0);
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![]);
        let icon = out
            .shapes
            .iter()
            .find_map(|shape| find_icon(&shape.shape, texture))
            .expect("App icon was not painted");
        let name = text_positions(&out)
            .into_iter()
            .find(|(text, _)| text == "Friendly Browser")
            .unwrap()
            .1;
        assert!(icon.right() < name.x && icon.top() <= name.y + 10.0);
        assert!((icon.width() - 24.0).abs() < 0.1);
        out.textures_delta.clear();
        let (mut clicked, _, _) = app_frame(&mut app, &ctx, size, click_events(icon.center()));
        clicked.textures_delta.clear();
        assert_eq!(app.selected_app.as_deref(), Some(path));
    }
    #[test]
    fn app_lists_fit_and_keep_the_directory_directly_below_the_name() {
        for size in [
            Vec2::new(960.0, 680.0),
            Vec2::new(1200.0, 820.0),
            Vec2::new(1440.0, 900.0),
        ] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = app_fixture();
            let (mut out, bottom, limit) = app_frame(&mut app, &ctx, size, vec![]);
            let text = text_positions(&out);
            out.textures_delta.clear();
            assert!(
                bottom <= limit + 1.0,
                "App lists overflow at {size:?}: {bottom} > {limit}"
            );
            let at = |s: &str| {
                text.iter()
                    .find(|(t, _)| t == s)
                    .unwrap_or_else(|| panic!("Missing {s} at {size:?}"))
                    .1
            };
            let apps = at("Apps");
            let hosts = at("Hosts");
            let types = at("Traffic type");
            assert!(apps.x < hosts.x && hosts.x < types.x);
            assert!((apps.y - hosts.y).abs() < 1.0 && (hosts.y - types.y).abs() < 1.0);
            let name = at("Friendly Browser");
            let directory = at(r"C:\Apps\Long application directory 0");
            assert!(
                (name.x - directory.x).abs() < 1.0,
                "Name {name:?} directory {directory:?}"
            );
            assert!(directory.y > name.y && directory.y < name.y + 26.0);
            assert_eq!(app.label(r"C:\Apps\plain.EXE"), "plain.EXE");
        }
    }
    #[test]
    fn clicking_an_app_filters_the_adjacent_lists_and_clicking_again_clears_it() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        let size = Vec2::new(1200.0, 820.0);
        let mut app = app_fixture();
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![]);
        out.textures_delta.clear();
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, _)| s == "Friendly Browser")
            .unwrap()
            .1
            + Vec2::new(3.0, 5.0);
        out.textures_delta.clear();
        let click = || {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(pos)]);
        out.textures_delta.clear();
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, click());
        out.textures_delta.clear();
        assert!(
            app.selected_app.is_some(),
            "Row labels must not swallow the click"
        );
        let totals = app.app_totals();
        let data = app.traffic_breakdown(&totals);
        assert_eq!(data.hosts.len(), 1);
        assert_eq!(data.types.len(), 1);
        assert_eq!(data.types[0].label, "HTTP");
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, click());
        out.textures_delta.clear();
        assert!(app.selected_app.is_none());
        assert_eq!(app.traffic_breakdown(&totals).hosts.len(), 80);
    }
    #[test]
    fn every_page_uses_the_same_bounded_shell_in_all_themes() {
        for theme in [Theme::Dark, Theme::Light, Theme::Nord, Theme::Mocha] {
            for size in [Vec2::new(960.0, 680.0), Vec2::new(1440.0, 900.0)] {
                for (page, source) in [
                    (Page::Overview, Source::Adapter),
                    (Page::Overview, Source::Transport),
                    (Page::Connections, Source::Transport),
                    (Page::Adapters, Source::Adapter),
                    (Page::Settings, Source::Adapter),
                ] {
                    let ctx = egui::Context::default();
                    theme.apply(&ctx);
                    install_fonts(&ctx);
                    let mut app = app_fixture();
                    app.page = page;
                    app.settings.filter.source = source;
                    app.settings.theme = theme;
                    app.data.snapshot.etw_active = true;
                    app.data.snapshot.connections = (0..65)
                        .map(|i| Connection {
                            app: format!(r"C:\Applications\Long folder {i}\browser.exe"),
                            pid: i,
                            local: "[2001:db8:84a3:8d3:1319:8a2e:370:7344]:53044".into(),
                            remote: "[2001:db8:84a3:8d3:1319:8a2e:370:7348]:443".into(),
                            protocol: "TCP".into(),
                            country: "Germany".into(),
                            state: "Established".into(),
                        })
                        .collect();
                    app.data.snapshot.adapters = (0..8)
                        .map(|i| Adapter {
                            id: i.to_string(),
                            name: format!("Ethernet {i}"),
                            description: "Long network controller description".into(),
                            up: i == 0,
                            addresses: vec![(
                                "2001:db8:84a3:8d3:1319:8a2e:370:7344".parse().unwrap(),
                                64,
                            )],
                            ..Default::default()
                        })
                        .collect();
                    let mut bounds = egui::Rect::NOTHING;
                    let mut out = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                            ..Default::default()
                        },
                        |root| {
                            bounds = app.draw(root);
                        },
                    );
                    let text = text_positions(&out);
                    out.textures_delta.clear();
                    assert!(
                        bounds.right() <= size.x + 1.0 && bounds.bottom() <= size.y + 1.0,
                        "{} {:?} {size:?} overflowed: {bounds:?}",
                        page.label(),
                        theme
                    );
                    assert!(
                        !text.iter().any(|(label, _)| label == "Applications"),
                        "The duplicate navigation must stay removed"
                    );
                    if page == Page::Connections {
                        assert!(text.iter().any(|(label, _)| label == "Remote endpoint"));
                        assert!(text.iter().any(|(label, _)| label == "Local endpoint"));
                    }
                }
            }
        }
    }
    #[test]
    fn table_columns_do_not_overlap_or_extend_beyond_the_page() {
        for width in [700.0, 950.0, 1200.0] {
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(width, 60.0));
            let cells = connection_columns(rect);
            assert!(
                cells
                    .windows(2)
                    .all(|pair| pair[0].right() + 7.0 <= pair[1].left())
            );
            assert!(cells.last().unwrap().right() <= rect.right());
        }
    }
    fn shell_frame(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(1200.0, 820.0),
                )),
                events,
                ..Default::default()
            },
            |root| {
                app.draw(root);
            },
        )
    }
    fn click_events(pos: egui::Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    #[test]
    fn sidebar_and_adapter_shortcut_keep_app_traffic_in_overview() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        app.data.snapshot.etw_active = true;
        app.data.snapshot.adapters = vec![Adapter {
            id: "eth".into(),
            name: "Ethernet".into(),
            ..Default::default()
        }];
        for _ in 0..2 {
            let mut out = shell_frame(&mut app, &ctx, vec![]);
            out.textures_delta.clear();
        }
        for (title, expected) in [
            ("Connections", Page::Connections),
            ("Adapters", Page::Adapters),
            ("Settings", Page::Settings),
            ("Overview", Page::Overview),
            ("Adapters", Page::Adapters),
        ] {
            let mut out = shell_frame(&mut app, &ctx, vec![]);
            let pos = text_positions(&out)
                .into_iter()
                .find(|(s, p)| s == title && p.x < widgets::NAV_WIDTH)
                .unwrap()
                .1
                + Vec2::new(6.0, 6.0);
            out.textures_delta.clear();
            let mut out = shell_frame(&mut app, &ctx, click_events(pos));
            out.textures_delta.clear();
            assert!(app.page == expected, "Sidebar {title} did not navigate");
        }
        let mut out = shell_frame(&mut app, &ctx, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, _)| s == "View traffic")
            .unwrap()
            .1
            + Vec2::new(5.0, 5.0);
        out.textures_delta.clear();
        let mut out = shell_frame(&mut app, &ctx, click_events(pos));
        out.textures_delta.clear();
        assert!(app.page == Page::Overview);
        assert_eq!(app.settings.filter.source, Source::Adapter);
        assert_eq!(app.settings.filter.adapters, HashSet::from(["eth".into()]));
    }
    #[test]
    fn primary_secondary_and_direction_text_stay_readable_in_every_palette() {
        fn luminance(c: Color32) -> f64 {
            let linear = |v: u8| {
                let v = f64::from(v) / 255.0;
                if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        }
        for theme in [Theme::Dark, Theme::Light, Theme::Nord, Theme::Mocha] {
            let ctx = egui::Context::default();
            theme.apply(&ctx);
            let mut out = ctx.run_ui(egui::RawInput::default(), |root| {
                for background in [
                    root.visuals().window_fill,
                    widgets::row_hover(root),
                    root.visuals().selection.bg_fill,
                ] {
                    let bg = luminance(background);
                    for (label, color) in [
                        ("primary", root.visuals().text_color()),
                        ("secondary", root.visuals().weak_text_color()),
                        ("download", download_color(root)),
                        ("upload", upload_color(root)),
                    ] {
                        let fg = luminance(color);
                        let contrast = (bg.max(fg) + 0.05) / (bg.min(fg) + 0.05);
                        assert!(
                            contrast >= 4.5,
                            "{theme:?} {label} contrast {contrast:.2} is too low"
                        );
                    }
                }
            });
            out.textures_delta.clear();
        }
    }
    #[test]
    fn legacy_preferences_get_compact_scaling_and_custom_size_survives_save() {
        let old: Settings = serde_json::from_str(r#"{"theme":"Dark","bits":true}"#).unwrap();
        assert_eq!(old.ui_scale, 0.9);
        let custom = Settings {
            ui_scale: 1.05,
            ..old
        };
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(restored.ui_scale, 1.05);
        assert_eq!(valid_ui_scale(f32::NAN), 0.9);
        assert_eq!(valid_ui_scale(2.0), 1.1);
    }
    #[test]
    fn summary_is_compact_and_keeps_usage_separate_from_captured_destinations() {
        for unknown in [0, 500_000_000] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = fixture();
            app.data.history = Arc::new(History {
                down: 7_945_689_497,
                up: 262_878_003,
                ..Default::default()
            });
            app.data.classified = ScopeTotals {
                lan: 41851,
                wan: 619_635_835,
                unknown,
            };
            app.data.top_apps = (0..6)
                .map(|i| AppUsage {
                    app: format!("app{i}.exe"),
                    down: 1000 - i * 100,
                    up: 10,
                })
                .collect();
            let mut summary = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(744.0, 150.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    summary = app.overall_summary(root);
                },
            );
            let labels = text_positions(&out);
            out.textures_delta.clear();
            assert!(
                summary.height() <= 132.0,
                "Summary grew to {}",
                summary.height()
            );
            assert!(summary.right() <= 745.0);
            for required in [
                "Downloaded",
                "Uploaded",
                "7.4 GiB",
                "250.7 MiB",
                "LAN / WAN",
                "LAN",
                "WAN",
                "Top 5 apps",
                "app0.exe",
                "app4.exe",
            ] {
                assert!(
                    labels.iter().any(|(text, _)| text == required),
                    "Missing {required}"
                );
            }
            assert!(
                labels.iter().any(|(text, _)| text.ends_with(" captured")),
                "App-byte total needs its own label"
            );
            assert_eq!(labels.iter().any(|(text, _)| text == "Other"), unknown > 0);
            assert!(!labels.iter().any(|(text, _)| text == "app5.exe"));
            assert!(
                !labels
                    .iter()
                    .any(|(text, _)| text == "Usage" || text.starts_with("last "))
            );
            let at = |needle: &str| labels.iter().find(|(text, _)| text == needle).unwrap().1;
            assert!(at("Downloaded").y < at("Uploaded").y && at("Uploaded").x < at("LAN / WAN").x);
        }
    }
    #[test]
    fn every_page_stays_within_the_window_at_supported_interface_sizes() {
        for scale in [0.8, 0.9, 1.1] {
            for (page, source) in [
                (Page::Overview, Source::Adapter),
                (Page::Overview, Source::Transport),
                (Page::Connections, Source::Transport),
                (Page::Adapters, Source::Adapter),
                (Page::Settings, Source::Adapter),
            ] {
                let ctx = egui::Context::default();
                Theme::Dark.apply(&ctx);
                install_fonts(&ctx);
                ctx.set_zoom_factor(scale);
                let size = Vec2::new(960.0, 680.0);
                let mut warm = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                        ..Default::default()
                    },
                    |_| {},
                );
                warm.textures_delta.clear();
                let mut app = app_fixture();
                app.page = page;
                app.settings.filter.source = source;
                app.settings.ui_scale = scale;
                let mut bounds = egui::Rect::NOTHING;
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            size / scale,
                        )),
                        ..Default::default()
                    },
                    |root| {
                        bounds = app.draw(root);
                    },
                );
                assert!((out.pixels_per_point - scale).abs() < 0.001);
                out.textures_delta.clear();
                assert!(
                    bounds.right() * scale <= size.x + 1.0
                        && bounds.bottom() * scale <= size.y + 1.0,
                    "{} at {}% exceeded the window: {bounds:?}",
                    page.label(),
                    scale * 100.0
                );
            }
        }
    }
    #[test]
    fn wider_default_respects_monitor_limits_and_preserves_height() {
        assert_eq!(
            initial_wide_size(Vec2::new(1200.0, 760.0), None),
            Vec2::new(1440.0, 760.0)
        );
        assert_eq!(
            initial_wide_size(Vec2::new(1600.0, 900.0), None),
            Vec2::new(1600.0, 900.0)
        );
        assert_eq!(
            initial_wide_size(Vec2::new(1000.0, 720.0), Some(Vec2::new(1280.0, 800.0))),
            Vec2::new(1248.0, 720.0)
        );
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.wide_summary_initialized);
        let restored: Settings = serde_json::from_str(
            &serde_json::to_string(&Settings {
                wide_summary_initialized: true,
                ..old
            })
            .unwrap(),
        )
        .unwrap();
        assert!(restored.wide_summary_initialized);
    }
    #[test]
    fn app_period_bar_proportions_cover_empty_single_and_large_totals() {
        let rect = egui::Rect::from_min_size(egui::pos2(20.0, 5.0), Vec2::new(120.0, 10.0));
        assert_eq!(traffic_bar_segments(rect, 0, 0), [None, None]);
        assert_eq!(traffic_bar_segments(rect, 1, 0), [Some(rect), None]);
        assert_eq!(traffic_bar_segments(rect, 0, 1), [None, Some(rect)]);
        for (down, up, expected_fraction) in [(3, 1, 0.75), (1, 3, 0.25), (u64::MAX, u64::MAX, 0.5)]
        {
            let [Some(download), Some(upload)] = traffic_bar_segments(rect, down, up) else {
                panic!("Missing bar segment");
            };
            assert!((download.width() / rect.width() - expected_fraction).abs() < 0.0001);
            assert_eq!(download.left(), rect.left());
            assert_eq!(download.right(), upload.left());
            assert_eq!(upload.right(), rect.right());
        }
    }
    #[test]
    fn app_period_summary_stays_visible_and_uses_all_filtered_history() {
        for size in [Vec2::new(960.0, 680.0), Vec2::new(1200.0, 820.0)] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = app_fixture();
            let history = Arc::make_mut(&mut app.data.history);
            history.down = 4 * 1024 * 1024;
            history.up = 2 * 1024 * 1024;
            app.app_search = "plain1".into();
            app.selected_app = Some(r"C:\Apps\Long application directory 1\plain1.exe".into());
            let (mut out, bottom, limit) = app_frame(&mut app, &ctx, size, vec![]);
            let labels = text_positions(&out);
            out.textures_delta.clear();
            let at = |needle: &str| {
                labels
                    .iter()
                    .find(|(text, _)| text == needle)
                    .unwrap_or_else(|| panic!("Missing {needle}"))
                    .1
            };
            let down_label = at("Downloaded");
            let up_label = at("Uploaded");
            assert!(down_label.y > at("Hosts").y && (down_label.y - up_label.y).abs() < 1.0);
            assert!(at("4.0 MiB").x > down_label.x && at("2.0 MiB").x > up_label.x);
            assert!(down_label.y < limit && bottom <= limit + 1.0);
        }
    }
    #[test]
    fn app_controls_share_one_line_and_dense_rows_show_more_items() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        let size = Vec2::new(1200.0, 820.0);
        let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![]);
        let text = text_positions(&out);
        out.textures_delta.clear();
        let at = |needle: &str| {
            text.iter()
                .find(|(s, _)| s == needle)
                .unwrap_or_else(|| panic!("Missing {needle}"))
                .1
        };
        let search = at("Search apps or folders");
        let period = at("Period: 1h");
        let adapters = at("Adapters: All");
        let network = at("Networks: All");
        assert!(period.x < adapters.x && adapters.x < network.x && network.x < search.x);
        assert!(
            [period, adapters, network]
                .iter()
                .all(|p| (p.y - search.y).abs() < 2.0)
        );
        assert!(!text.iter().any(|(s, _)| s.contains("Highest usage first")
            || s.contains("Network usage across your adapters")
            || s.contains("Hosts and types for")));
        let count = text
            .iter()
            .filter(|(s, p)| {
                (s == "Friendly Browser" || (s.starts_with("plain") && s.ends_with(".exe")))
                    && p.y < size.y - 20.0
            })
            .count();
        assert!(count >= 10, "Only {count} apps fit in the dense list");
    }
    fn click_app_control(
        app: &mut MonitorApp,
        ctx: &egui::Context,
        label: &str,
    ) -> egui::FullOutput {
        let size = Vec2::new(1200.0, 820.0);
        let (mut out, _, _) = app_frame(app, ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, _)| s == label)
            .unwrap_or_else(|| panic!("Missing {label}"))
            .1
            + Vec2::new(3.0, 5.0);
        out.textures_delta.clear();
        app_frame(app, ctx, size, click_events(pos)).0
    }
    #[test]
    fn compact_period_menu_has_dense_rows_and_shows_longer_presets() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        let mut output = click_app_control(&mut app, &ctx, "Period: 1h");
        output.textures_delta.clear();
        let (mut output, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        let positions = text_positions(&output);
        output.textures_delta.clear();
        let at = |label| {
            positions
                .iter()
                .find(|(text, _)| text == label)
                .map(|(_, position)| *position)
                .unwrap_or_else(|| panic!("Missing period {label}"))
        };
        let first = at("5 minutes");
        let second = at("15 minutes");
        assert!(
            second.y - first.y <= 24.0,
            "Period rows have excessive spacing"
        );
        assert!(
            at("1 year").y < 820.0,
            "Longer presets require unnecessary scrolling"
        );
        assert!(at("Since beginning of month").y < first.y);
    }
    #[test]
    fn expanded_period_menu_supports_both_views_and_custom_cancel() {
        for source in [Source::Adapter, Source::Transport] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = app_fixture();
            app.settings.filter.source = source;
            for (caption, choice, seconds) in [
                ("Period: 1h", "15 minutes", 900),
                ("Period: 15m", "30 minutes", 1800),
                (
                    "Period: 30m",
                    "Since beginning of month",
                    PERIOD_MONTH_TO_DATE,
                ),
                ("Period: Month", "All time", 0),
            ] {
                let mut out = click_app_control(&mut app, &ctx, caption);
                out.textures_delta.clear();
                let mut out = click_app_control(&mut app, &ctx, choice);
                out.textures_delta.clear();
                assert_eq!(app.settings.filter.seconds, seconds);
                assert!(app.settings.filter.time_range.is_none());
            }
            let mut out = click_app_control(&mut app, &ctx, "Period: All time");
            out.textures_delta.clear();
            let mut out = click_app_control(&mut app, &ctx, "Custom…");
            out.textures_delta.clear();
            assert!(app.custom_period.is_some());
            let mut out = click_app_control(&mut app, &ctx, "Cancel");
            out.textures_delta.clear();
            assert!(app.custom_period.is_none());
            assert!(app.settings.filter.all_time());
        }
    }
    #[test]
    fn many_adapters_leave_wifi_choices_accessible() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        app.data.snapshot.adapters = (0..40)
            .map(|i| Adapter {
                id: format!("adapter-{i}"),
                name: format!("Adapter {i}"),
                up: true,
                ..Default::default()
            })
            .collect();
        app.data.snapshot.wifi_networks = vec![WifiNetwork {
            id: "home-id".into(),
            name: "Home Wi-Fi".into(),
        }];
        let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        warm.textures_delta.clear();
        let mut opened = click_app_control(&mut app, &ctx, "Adapters: All");
        opened.textures_delta.clear();
        let (mut opened, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        let position = text_positions(&opened)
            .into_iter()
            .find(|(text, _)| text == "Home Wi-Fi")
            .map(|(_, position)| position);
        opened.textures_delta.clear();
        let position =
            position.expect("Wi-Fi network should remain accessible below a long adapter list");
        assert!(position.y < 820.0, "Wi-Fi choices extend beyond the window");
        let mut selected = click_app_control(&mut app, &ctx, "Home Wi-Fi");
        selected.textures_delta.clear();
        assert_eq!(app.settings.filter.wifi_network.as_deref(), Some("home-id"));
    }
    #[test]
    fn overview_and_app_filter_bars_have_the_same_height() {
        for width in [960.0, 1440.0] {
            for scale in [0.8, 0.9, 1.1] {
                let mut heights = Vec::new();
                for apps in [false, true] {
                    let ctx = egui::Context::default();
                    Theme::Dark.apply(&ctx);
                    install_fonts(&ctx);
                    ctx.set_zoom_factor(scale);
                    let mut app = app_fixture();
                    app.settings.filter.source = if apps {
                        Source::Transport
                    } else {
                        Source::Adapter
                    };
                    let mut height = 0.0;
                    let mut out = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(width, 680.0) / scale,
                            )),
                            ..Default::default()
                        },
                        |ui| {
                            let top = ui.cursor().top();
                            app.toolbar(ui, apps);
                            height = ui.cursor().top() - top;
                        },
                    );
                    out.textures_delta.clear();
                    heights.push(height);
                }
                assert!(
                    (heights[0] - heights[1]).abs() * scale <= 1.0,
                    "width {width}, scale {scale}: {heights:?}"
                );
            }
        }
    }
    #[test]
    fn overall_dropdowns_change_period_and_support_multiple_adapter_choices() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        app.settings.filter.source = Source::Adapter;
        app.data.snapshot.adapters = vec![
            Adapter {
                id: "eth".into(),
                name: "Ethernet".into(),
                up: true,
                ..Default::default()
            },
            Adapter {
                id: "wifi".into(),
                name: "Wi-Fi".into(),
                up: true,
                ..Default::default()
            },
            Adapter {
                id: "vpn".into(),
                name: "VPN".into(),
                up: true,
                ..Default::default()
            },
        ];
        let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        warm.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Period: 1h");
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "30 minutes");
        out.textures_delta.clear();
        assert_eq!(app.settings.filter.seconds, 1800);
        let mut out = click_app_control(&mut app, &ctx, "Adapters: All");
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Ethernet");
        assert_eq!(
            app.settings.filter.adapters,
            HashSet::from(["wifi".into(), "vpn".into()])
        );
        assert!(text_positions(&out).iter().any(|(s, _)| s == "Wi-Fi"));
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Wi-Fi");
        out.textures_delta.clear();
        assert_eq!(app.settings.filter.adapters, HashSet::from(["vpn".into()]));
    }
    #[test]
    fn wifi_connection_filter_is_independent_of_adapters_and_can_be_cleared() {
        for source in [Source::Adapter, Source::Transport] {
            let ctx = egui::Context::default();
            Theme::Dark.apply(&ctx);
            install_fonts(&ctx);
            let mut app = app_fixture();
            app.settings.filter.source = source;
            app.settings.filter.adapters = HashSet::from(["wifi".into()]);
            if source == Source::Transport {
                app.settings.filter.scope = Scope::Wan;
            }
            app.data.snapshot.adapters = vec![
                Adapter {
                    id: "eth".into(),
                    name: "Ethernet".into(),
                    up: true,
                    ..Default::default()
                },
                Adapter {
                    id: "wifi".into(),
                    name: "Wi-Fi".into(),
                    up: true,
                    ..Default::default()
                },
            ];
            app.data.snapshot.wifi_networks = vec![
                WifiNetwork {
                    id: "home-id".into(),
                    name: "Home Wi-Fi".into(),
                },
                WifiNetwork {
                    id: "office-id".into(),
                    name: "Office Wi-Fi".into(),
                },
            ];
            let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
            warm.textures_delta.clear();
            let mut out = click_app_control(&mut app, &ctx, "Adapters: 1");
            out.textures_delta.clear();
            let (mut out, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
            let present = text_positions(&out)
                .iter()
                .any(|(text, _)| text == "Office Wi-Fi");
            out.textures_delta.clear();
            assert!(present, "Remembered network is missing");
            let mut out = click_app_control(&mut app, &ctx, "Office Wi-Fi");
            out.textures_delta.clear();
            assert_eq!(
                app.settings.filter.wifi_network.as_deref(),
                Some("office-id")
            );
            assert_eq!(app.adapter_filter_caption().0, "Wi-Fi: Office Wi-Fi");
            assert_eq!(app.settings.filter.adapters, HashSet::from(["wifi".into()]));
            let mut out = click_app_control(&mut app, &ctx, "Home Wi-Fi");
            out.textures_delta.clear();
            assert_eq!(app.settings.filter.wifi_network.as_deref(), Some("home-id"));
            let mut out = click_app_control(&mut app, &ctx, "Unidentified Wi-Fi");
            out.textures_delta.clear();
            assert_eq!(
                app.settings.filter.wifi_network.as_deref(),
                Some(WIFI_UNKNOWN)
            );
            let mut out = click_app_control(&mut app, &ctx, "All connections");
            out.textures_delta.clear();
            assert_eq!(app.settings.filter.wifi_network, None);
            assert_eq!(app.settings.filter.adapters, HashSet::from(["wifi".into()]));
            assert_eq!(
                app.settings.filter.scope,
                if source == Source::Transport {
                    Scope::Wan
                } else {
                    Scope::All
                }
            );
        }
    }
    #[test]
    fn dropdowns_change_filters_and_keep_the_adapter_menu_open_for_multiple_choices() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        app.data.snapshot.adapters = vec![
            Adapter {
                id: "eth".into(),
                name: "Ethernet".into(),
                up: true,
                ..Default::default()
            },
            Adapter {
                id: "wifi".into(),
                name: "Wi-Fi".into(),
                up: true,
                ..Default::default()
            },
        ];
        let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        warm.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Period: 1h");
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "30 minutes");
        out.textures_delta.clear();
        assert_eq!(app.settings.filter.seconds, 1800);
        let mut out = click_app_control(&mut app, &ctx, "Adapters: All");
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Ethernet");
        assert!(!app.settings.filter.adapters.contains("eth"));
        assert!(app.settings.filter.adapters.contains("wifi"));
        assert!(app.settings.filter.adapters.contains("unassigned"));
        assert!(
            text_positions(&out).iter().any(|(s, _)| s == "Wi-Fi"),
            "Adapter popup closed after a checkbox click"
        );
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "Wi-Fi");
        out.textures_delta.clear();
        assert_eq!(
            app.settings.filter.adapters,
            HashSet::from(["unassigned".into()])
        );
        let mut out = click_app_control(&mut app, &ctx, "Networks: All");
        out.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "WAN");
        out.textures_delta.clear();
        assert_eq!(app.settings.filter.scope, Scope::Wan);
    }
    #[test]
    fn selection_can_be_cleared_from_the_apps_heading() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut app = app_fixture();
        app.selected_app = Some(r"C:\Apps\Long application directory 0\plain0.exe".into());
        let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
        warm.textures_delta.clear();
        let mut out = click_app_control(&mut app, &ctx, "All apps");
        out.textures_delta.clear();
        assert!(app.selected_app.is_none());
    }
}
