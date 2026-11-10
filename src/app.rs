use crate::{
    alerts::AlertConfig,
    icons::IconCache,
    model::*,
    store::Store,
    traffic::{Breakdown, breakdown, executable_directory, executable_filename},
    tray::Tray,
    ui::tokens::{BaseColor, Scheme, Tokens},
    worker::{Command, Shared, Worker},
};
use anyhow::Result;
use eframe::egui::{self, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

mod app_timeline;
mod connection_tools;
mod dialogs;
mod logic;
mod pages;
mod period;
pub use pages::{DEFAULT_WINDOW_SIZE, MIN_LOGICAL_SIZE, MIN_WINDOW_SIZE};
use period::{CustomPeriod, DateTimeFields, PERIODS, PeriodMode, Unit, period_caption};

#[cfg(any(test, feature = "ui-preview"))]
mod preview;
#[cfg(test)]
mod tests;

/// The palette names older releases saved. They are only read, to carry a light or dark
/// choice over to the current appearance setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum LegacyTheme {
    #[default]
    Dark,
    Light,
    Nord,
    Mocha,
}

/// The colour of the tray icon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum TrayIconStyle {
    /// The app's own teal.
    #[default]
    Default,
    /// The accent colour chosen for the app.
    Accent,
    /// White on a dark taskbar, dark gray on a light one.
    Monochrome,
}

/// Light, dark, or whichever Windows is using.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    theme: LegacyTheme,
    /// The appearance chosen in the current interface. Unknown (none) in settings saved
    /// before it existed, which fall back to `follow_system` and `theme`.
    appearance: Option<Appearance>,
    base_color: BaseColor,
    filter: Filter,
    close_to_tray: bool,
    /// The user accepts launching at sign-in with administrator rights from a folder that
    /// non-administrators can change.
    accept_startup_risk: bool,
    tray_icon: TrayIconStyle,
    /// After this many days minute-by-minute history is merged into hourly totals; 0 keeps
    /// every minute for ever.
    detail_days: u32,
    /// After this many days hourly totals are merged into daily ones; 0 leaves them hourly.
    daily_days: u32,
    geo: Option<PathBuf>,
    bits: bool,
    ui_scale: f32,
    wide_summary_initialized: bool,
    /// Follow the Windows light/dark setting instead of `theme`.
    follow_system: bool,
    /// Whether the welcome card has been dismissed. Unknown (none) in settings saved
    /// before it existed, which are treated as already seen.
    intro_seen: Option<bool>,
    alert_new_apps: bool,
    cap_enabled: bool,
    /// Monthly allowance in GiB.
    cap_gib: f32,
    /// Day of the month, 1 to 28, on which the allowance starts over.
    cap_reset_day: u8,
}
impl Settings {
    fn alert_config(&self) -> AlertConfig {
        AlertConfig {
            new_apps: self.alert_new_apps,
            cap_bytes: (self.cap_enabled && self.cap_gib > 0.0)
                .then(|| (f64::from(self.cap_gib) * 1_073_741_824.0) as u64),
            reset_day: self.cap_reset_day.clamp(1, 28),
        }
    }
    fn appearance(&self) -> Appearance {
        self.appearance.unwrap_or(if self.follow_system {
            Appearance::System
        } else if self.theme == LegacyTheme::Light {
            Appearance::Light
        } else {
            Appearance::Dark
        })
    }
    /// What old history is merged into, as the worker needs it.
    fn compaction_plan(&self) -> CompactionPlan {
        // A hand-edited settings file could hold anything; a century is more than any history.
        let days = |days: u32| (days > 0).then_some(days.min(36_500));
        CompactionPlan {
            detail_days: days(self.detail_days),
            daily_days: days(self.daily_days),
        }
    }
    /// The tray icon's colour, or `None` for its own. The taskbar's theme decides what is
    /// visible, which can differ from the app's when the app is set to a fixed theme.
    fn tray_icon_color(&self, taskbar_dark: bool) -> Option<[u8; 3]> {
        match self.tray_icon {
            TrayIconStyle::Default => None,
            TrayIconStyle::Accent => {
                let scheme = if taskbar_dark {
                    Scheme::Dark
                } else {
                    Scheme::Light
                };
                let color = self.base_color.primary(scheme);
                Some([color.r(), color.g(), color.b()])
            }
            TrayIconStyle::Monochrome => Some(if taskbar_dark {
                [255, 255, 255]
            } else {
                [63, 63, 70]
            }),
        }
    }
    /// The colour scheme to draw with now.
    fn scheme(&self, ctx: &egui::Context) -> Scheme {
        match self.appearance() {
            Appearance::Light => Scheme::Light,
            Appearance::Dark => Scheme::Dark,
            Appearance::System => match ctx.system_theme() {
                Some(egui::Theme::Dark) => Scheme::Dark,
                _ => Scheme::Light,
            },
        }
    }
    fn tokens(&self, ctx: &egui::Context) -> Tokens {
        Tokens::new(self.scheme(ctx), self.base_color)
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: LegacyTheme::Dark,
            appearance: None,
            base_color: BaseColor::Zinc,
            filter: Filter::default(),
            close_to_tray: true,
            accept_startup_risk: false,
            tray_icon: TrayIconStyle::Default,
            detail_days: 30,
            daily_days: 0,
            geo: None,
            bits: false,
            ui_scale: DEFAULT_UI_SCALE,
            wide_summary_initialized: false,
            follow_system: false,
            intro_seen: None,
            alert_new_apps: false,
            cap_enabled: false,
            cap_gib: 100.0,
            cap_reset_day: 1,
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
    /// In sidebar order, which is also the Ctrl+1 to Ctrl+4 order.
    const ALL: [Self; 4] = [
        Self::Overview,
        Self::Connections,
        Self::Adapters,
        Self::Settings,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Connections => "Connections",
            Self::Adapters => "Adapters",
            Self::Settings => "Settings",
        }
    }
}
/// What the Overview's app list shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TopAppsView {
    /// The busiest apps in the selected period.
    #[default]
    Top,
    /// The apps that last used the network, however long ago.
    Recent,
}
/// The filtered Connections list, remembered until a new snapshot or search.
#[derive(Default)]
struct ConnectionCache {
    key: Option<(
        u64,
        String,
        Option<connection_tools::Sort>,
        connection_tools::ProtocolFilter,
        connection_tools::StatusFilter,
    )>,
    rows: Arc<Vec<(usize, String)>>,
}
/// History bytes per app, remembered until the next history load.
#[derive(Default)]
struct HistoryTotals {
    key: Option<(u64, u64)>,
    by_app: Arc<BTreeMap<String, (u64, u64)>>,
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
    /// How much space the data folder takes, and when that was measured.
    storage: Option<(std::time::Instant, logic::StorageUsage)>,
    view_warning: String,
    custom_period: Option<CustomPeriod>,
    app_cache: Arc<Vec<AppTotal>>,
    app_cache_key: Option<(u64, u64, u64, String)>,
    /// Per-app byte totals of the loaded history, which only change with a new load.
    history_totals: HistoryTotals,
    dialogs: dialogs::FileDialogs,
    connection_cache: ConnectionCache,
    connection_sort: Option<connection_tools::Sort>,
    protocol_filter: connection_tools::ProtocolFilter,
    status_filter: connection_tools::StatusFilter,
    pending_end: Option<(u32, String)>,
    timeline_mode: app_timeline::TimelineMode,
    /// The app whose timeline the worker was last asked to read.
    focus_sent: Option<(String, SeriesView)>,
    top_apps_view: TopAppsView,
    /// The interface size the window's minimum size was last set for.
    min_size_for: Option<f32>,
    /// The pause choice the worker was last given.
    pause_sent: bool,
    /// The last time the tray tooltip was refreshed, by snapshot.
    tooltip_revision: u64,
    /// Whether the taskbar is dark, read from Windows about once a second.
    taskbar_dark: bool,
    /// The message in the banner and when this window first showed it.
    notice_seen: Option<(String, Instant)>,
    /// The alert choices the worker was last given.
    alerts_sent: AlertConfig,
    compaction_sent: Option<CompactionPlan>,
    /// When the window last drew a frame. Hidden in the tray it draws none.
    last_ui: Instant,
    shown_sent: Option<bool>,
    /// The Hosts table shows lifetime totals rather than the period's.
    hosts_lifetime: bool,
    lifetime_sent: bool,
    /// The allowance as typed, so a half-typed number is not rewritten.
    cap_input: String,
    breakdown_cache: Arc<Breakdown>,
    breakdown_cache_key: Option<(u64, u64, u64, String, Option<String>)>,
    host_request: Option<Instant>,
    /// Colours the title bar was last painted with.
    titlebar: Option<Tokens>,
    /// Colours installed in the window.
    applied: Tokens,
    /// The last window record eframe saved that described a real window.
    window_record: Option<String>,
}
impl MonitorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, store: Store, data_dir: PathBuf) -> Result<Self> {
        let saved = cc
            .storage
            .and_then(|s| eframe::get_value::<Settings>(s, "settings"));
        let first_run = saved.is_none();
        let mut settings = saved.unwrap_or_default();
        // New installs match Windows; a saved choice is never changed behind its back.
        if first_run {
            settings.appearance = Some(Appearance::System);
        }
        settings.intro_seen.get_or_insert(!first_run);
        settings.filter.search.clear();
        settings.filter.apps.clear();
        settings.filter.scope = Scope::All;
        crate::ui::text::install_fonts(&cc.egui_ctx);
        let tokens = settings.tokens(&cc.egui_ctx);
        crate::ui::tokens::install(&cc.egui_ctx, tokens);
        let window_record = cc
            .storage
            .and_then(|storage| storage.get_string(WINDOW_KEY))
            .filter(|record| crate::window_state::is_sane(record));
        // Done before the window is first shown so the title bar never flashes its default colour.
        let titlebar = crate::titlebar::apply(cc, crate::titlebar::Colors::from_tokens(&tokens))
            .then_some(tokens);
        settings.ui_scale = valid_ui_scale(settings.ui_scale);
        crate::tray_menu::set_palette(crate::tray_menu::Palette::from_tokens(&tokens));
        cc.egui_ctx.set_zoom_factor(settings.ui_scale);
        let cap_input = format_allowance(settings.cap_gib);
        let worker = Worker::start(
            store,
            settings.filter.clone(),
            settings.geo.clone(),
            cc.egui_ctx.clone(),
        )?;
        // Windows ends the session by terminating the app, so the normal exit never runs. Ask the
        // worker to save what is waiting as soon as the notice arrives, and wait briefly for it.
        crate::tray::on_session_end({
            let commands = worker.tx.clone();
            move || {
                let (done, saved) = std::sync::mpsc::channel();
                if commands.send(Command::SaveNow(done)).is_ok() {
                    let _ = saved.recv_timeout(std::time::Duration::from_secs(3));
                }
            }
        });
        let (tray, view_warning) = match Tray::new(cc.egui_ctx.clone()) {
            Ok(t) => (Some(t), String::new()),
            Err(e) => (
                None,
                format!("Tray unavailable: {e}. Closing the window will exit."),
            ),
        };
        crate::tray::remember_window(cc);
        crate::tray::set_caption_icon(tokens.is_dark());
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
            storage: None,
            view_warning,
            custom_period: None,
            app_cache: Arc::new(Vec::new()),
            app_cache_key: None,
            history_totals: HistoryTotals::default(),
            dialogs: dialogs::FileDialogs::default(),
            connection_cache: ConnectionCache::default(),
            connection_sort: None,
            protocol_filter: Default::default(),
            status_filter: Default::default(),
            pending_end: None,
            timeline_mode: Default::default(),
            focus_sent: None,
            alerts_sent: AlertConfig::default(),
            compaction_sent: None,
            last_ui: Instant::now(),
            shown_sent: None,
            hosts_lifetime: false,
            lifetime_sent: false,
            notice_seen: None,
            top_apps_view: TopAppsView::Top,
            min_size_for: None,
            pause_sent: false,
            tooltip_revision: 0,
            taskbar_dark: true,
            cap_input,
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
            titlebar,
            applied: tokens,
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
    /// What the tray icon says: the current speeds, or that recording is paused.
    fn tray_tooltip(&self) -> String {
        let (down, up) = self.live_rates(Source::Adapter);
        let mut text = format!(
            "OpenNetworkMonitor\n↓ {}   ↑ {}",
            self.rate(down),
            self.rate(up)
        );
        if self.data.snapshot.recording_paused {
            text.push_str("\nRecording paused");
        }
        text
    }
    /// Keyboard shortcuts that work on every page. Typing in a field is left alone.
    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let (page, search, export, escape) = ctx.input_mut(|input| {
            let command = Modifiers::COMMAND;
            let page = [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                .into_iter()
                .position(|key| input.consume_key(command, key));
            (
                page,
                input.consume_key(command, Key::F),
                input.consume_key(command, Key::E),
                input.key_pressed(Key::Escape),
            )
        });
        if let Some(index) = page {
            let target = Page::ALL[index];
            self.page = target;
        }
        if search {
            ctx.memory_mut(|memory| {
                memory.request_focus(egui::Id::new(crate::ui::controls::SEARCH_ID))
            });
        }
        if export {
            self.export(ctx);
        }
        if escape && !ctx.egui_wants_keyboard_input() && self.selected_app.is_some() {
            self.selected_app = None;
        }
    }
    fn switch_source(&mut self, source: Source) {
        if self.settings.filter.source != source {
            self.settings.filter.source = source;
            self.selected_app = None;
            self.update_filter();
        }
    }
    fn export(&mut self, ctx: &egui::Context) {
        let name = format!(
            "network-usage-{}.csv",
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        );
        self.dialogs.save_csv(ctx, name);
    }
    /// Starts the export once the user has picked where to save it.
    fn start_export(&mut self, path: PathBuf) {
        let mut filter = self.settings.filter.clone();
        if filter.source == Source::Transport {
            if let Some(app) = &self.selected_app {
                filter.apps = HashSet::from([app.clone()]);
            } else if !self.app_search.is_empty() {
                filter.apps = self.app_totals().iter().map(|a| a.path.clone()).collect();
                if filter.apps.is_empty() {
                    filter.apps.insert(NO_APPS.into());
                }
            }
        }
        let _ = self.worker.tx.send(Command::Export(path, filter));
        self.set_notice("Preparing export…");
    }
    /// Shows a message in the banner. It goes through the shared state as well, since
    /// the window copies that over its own every time it changes.
    fn set_notice(&mut self, text: &str) {
        self.data.notice = text.to_owned();
        if let Ok(mut state) = self.worker.shared.lock() {
            state.notice = text.to_owned();
        }
    }
    fn use_country_database(&mut self, path: PathBuf) {
        self.settings.geo = Some(path.clone());
        let _ = self.worker.tx.send(Command::Geo(Some(path)));
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
        crate::ui::chart::speed_label(value, self.settings.bits)
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
        let history_key = (self.data.history_generation, self.filter_revision);
        if self.history_totals.key != Some(history_key) {
            let mut by_app = BTreeMap::<String, (u64, u64)>::new();
            for r in &self.data.history.rows {
                let t = by_app.entry(r.app.clone()).or_default();
                t.0 = t.0.saturating_add(r.down);
                t.1 = t.1.saturating_add(r.up);
            }
            self.history_totals = HistoryTotals {
                key: Some(history_key),
                by_app: Arc::new(by_app),
            };
        }
        let mut totals = BTreeMap::<String, AppTotal>::new();
        for (app, (down, up)) in self.history_totals.by_app.iter() {
            totals.insert(
                app.clone(),
                AppTotal {
                    path: app.clone(),
                    down: *down,
                    up: *up,
                    ..Default::default()
                },
            );
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
        // Host names only ever get added, so their count tells when they changed.
        let key = (
            self.data.snapshot.host_labels.len() as u64,
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
        match self.dialogs.poll() {
            Some((dialogs::Purpose::ExportCsv, path)) => self.start_export(path),
            Some((dialogs::Purpose::CountryDatabase, path)) => self.use_country_database(path),
            None => {}
        }
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
                || data.series_generation != self.data.series_generation
                || data.lifetime_generation != self.data.lifetime_generation
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
        // Only the app being looked at on the App traffic page needs its own timeline.
        let focus = (self.page == Page::Overview
            && self.settings.filter.source == Source::Transport)
            .then(|| self.selected_app.clone())
            .flatten()
            .map(|app| (app, self.timeline_mode.view()));
        if let Some(tray) = &self.tray {
            let paused = tray.paused.load(Ordering::Acquire);
            if paused != self.pause_sent {
                let _ = self.worker.tx.send(Command::Pause(paused));
                self.pause_sent = paused;
            }
            if tray.open_settings.swap(false, Ordering::AcqRel) {
                self.page = Page::Settings;
            }
            if self.tooltip_revision != self.data.revision {
                self.tooltip_revision = self.data.revision;
                self.taskbar_dark = crate::tray::taskbar_is_dark();
                tray.set_tooltip(&self.tray_tooltip());
            }
            tray.set_color(self.settings.tray_icon_color(self.taskbar_dark));
        }
        let alerts = self.settings.alert_config();
        // A hidden window draws nothing, so a quiet couple of seconds means it is in the tray.
        let shown = self.last_ui.elapsed() < Duration::from_secs(2);
        if self.shown_sent != Some(shown) {
            let _ = self.worker.tx.send(Command::WindowShown(shown));
            self.shown_sent = Some(shown);
        }
        let plan = self.settings.compaction_plan();
        if self.compaction_sent != Some(plan) {
            let _ = self.worker.tx.send(Command::Compaction(plan));
            self.compaction_sent = Some(plan);
        }
        // Lifetime hosts are only read while the table showing them is on screen.
        let lifetime = self.hosts_lifetime
            && self.page == Page::Overview
            && self.settings.filter.source == Source::Transport;
        if lifetime != self.lifetime_sent {
            let _ = self.worker.tx.send(Command::LifetimeHosts(lifetime));
            self.lifetime_sent = lifetime;
        }
        if alerts != self.alerts_sent {
            let _ = self.worker.tx.send(Command::Alerts(alerts));
            self.alerts_sent = alerts;
        }
        if focus != self.focus_sent {
            let _ = self.worker.tx.send(Command::FocusApp(focus.clone()));
            self.focus_sent = focus;
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
        self.last_ui = Instant::now();
        // Pages are laid out for a minimum logical size; keep the window from going below it
        // at whatever interface size is chosen.
        if self.min_size_for != Some(self.settings.ui_scale) {
            root.ctx()
                .send_viewport_cmd(egui::ViewportCommand::MinInnerSize(
                    Vec2::from(MIN_LOGICAL_SIZE) * self.settings.ui_scale,
                ));
            self.min_size_for = Some(self.settings.ui_scale);
        }
        let tokens = self.settings.tokens(root.ctx());
        if self.applied != tokens {
            crate::ui::tokens::install(root.ctx(), tokens);
            self.applied = tokens;
            root.ctx().request_repaint();
        }
        crate::tray::set_caption_icon(tokens.is_dark());
        if self.titlebar != Some(tokens) {
            crate::tray_menu::set_palette(crate::tray_menu::Palette::from_tokens(&tokens));
            crate::titlebar::apply(&*frame, crate::titlebar::Colors::from_tokens(&tokens));
            self.titlebar = Some(tokens);
        }
        self.shortcuts(root.ctx());
        self.draw(root);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Remove the tray icon promptly while the worker saves its final data.
        drop(self.tray.take());
        self.worker.shutdown();
    }
    /// The saved file holds the settings and the window's place, not egui's own memory: that was
    /// 65 KB of scroll positions and open areas, rewritten every 30 seconds for nothing.
    fn persist_egui_memory(&self) -> bool {
        false
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, "settings", &self.settings);
        // Older versions saved egui's memory under this key; leave nothing of it behind.
        storage.remove_string("egui");
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
            selected.insert(UNASSIGNED_ADAPTER.into());
        }
    }
    selected.remove(NO_ADAPTERS);
    if enabled {
        selected.insert(id.into());
    } else {
        selected.remove(id);
    }
    if selected.is_empty() {
        selected.insert(NO_ADAPTERS.into());
    }
}
fn initial_wide_size(current: Vec2, monitor: Option<Vec2>) -> Vec2 {
    let width = current.x.max(DEFAULT_WINDOW_SIZE[0]);
    let width = monitor.map_or(width, |m| width.min((m.x - 32.0).max(MIN_WINDOW_SIZE[0])));
    Vec2::new(width, current.y)
}
/// New installs open a little smaller than 100% so more of each page is on screen.
const DEFAULT_UI_SCALE: f32 = 0.85;

fn valid_ui_scale(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(0.8, 1.1)
    } else {
        DEFAULT_UI_SCALE
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
/// An allowance as shown in its text box: whole numbers without a decimal point.
fn format_allowance(gib: f32) -> String {
    if gib.fract() == 0.0 {
        format!("{gib:.0}")
    } else {
        format!("{gib}")
    }
}
fn app_name(path: &str) -> String {
    executable_filename(path)
}
