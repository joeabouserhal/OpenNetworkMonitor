use crate::{
    graph::{Graph, format_number},
    icons::IconCache,
    model::*,
    store::Store,
    traffic::{Breakdown, RankedUsage, breakdown, executable_directory, executable_filename},
    tray::Tray,
    widgets::{self, install_fonts},
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

mod adapters;
mod apps;
mod connections;
mod filters;
mod overall;
mod period;
mod settings_page;
mod shell;
mod theme;
use period::{CustomPeriod, PERIODS, period_caption};
use theme::Theme;

#[cfg(feature = "ui-preview")]
mod preview;
#[cfg(test)]
mod tests;

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
    fn icon(self) -> widgets::Icon {
        match self {
            Self::Overview => widgets::Icon::Overview,
            Self::Connections => widgets::Icon::Connections,
            Self::Adapters => widgets::Icon::Adapters,
            Self::Settings => widgets::Icon::Settings,
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
    custom_period: Option<CustomPeriod>,
    app_cache: Arc<Vec<AppTotal>>,
    app_cache_key: Option<(u64, u64, u64, String)>,
    breakdown_cache: Arc<Breakdown>,
    breakdown_cache_key: Option<(u64, u64, u64, String, Option<String>)>,
    host_request: Option<Instant>,
    /// Theme the title bar was last coloured for.
    titlebar: Option<Theme>,
    /// The last window record eframe saved that described a real window.
    window_record: Option<String>,
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
        let window_record = cc
            .storage
            .and_then(|storage| storage.get_string(WINDOW_KEY))
            .filter(|record| crate::window_state::is_sane(record));
        // Done before the window is first shown so the title bar never flashes its default colour.
        let titlebar = crate::titlebar::apply(
            cc,
            crate::titlebar::Colors::from_palette(&settings.theme.palette()),
        )
        .then_some(settings.theme);
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
        crate::tray::remember_window(cc);
        // A newer build asks this copy to exit the same way Tray > Exit does, so data is saved first.
        let exit = tray.as_ref().map_or_else(
            || Arc::new(std::sync::atomic::AtomicBool::new(false)),
            |tray| tray.exit.clone(),
        );
        let wake = cc.egui_ctx.clone();
        crate::capture::spawn_quit_listener(move || {
            crate::tray::request_exit(&wake, &exit);
            wake.request_repaint();
        });
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
            custom_period: None,
            app_cache: Arc::new(Vec::new()),
            app_cache_key: None,
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
            titlebar,
            window_record,
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
}

/// Key under which eframe stores the window's size and position.
const WINDOW_KEY: &str = "window";

impl MonitorApp {
    /// eframe has just stored the window's geometry. A minimized window reports a
    /// collapsed one, which must not replace the last real size and position.
    fn keep_window_record(&mut self, storage: &mut dyn eframe::Storage) {
        let Some(record) = storage.get_string(WINDOW_KEY) else {
            return;
        };
        if crate::window_state::is_sane(&record) {
            self.window_record = Some(record);
        } else {
            let kept = self
                .window_record
                .clone()
                .unwrap_or_else(|| crate::window_state::DEFAULT_RECORD.to_owned());
            storage.set_string(WINDOW_KEY, kept);
        }
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
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.titlebar != Some(self.settings.theme) {
            crate::titlebar::apply(
                &*frame,
                crate::titlebar::Colors::from_palette(&self.settings.theme.palette()),
            );
            self.titlebar = Some(self.settings.theme);
        }
        self.draw(root);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Remove the tray icon promptly while the worker saves its final data.
        drop(self.tray.take());
        self.worker.shutdown();
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
        self.keep_window_record(storage);
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
fn compact_bytes(value: u64) -> String {
    crate::model::format_compact_bytes(value)
}
fn app_name(path: &str) -> String {
    executable_filename(path)
}
