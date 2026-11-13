use super::*;
use eframe::egui::Color32;

const PREVIEW_PATHS: [&str; 5] = [
    r"C:\Program Files\Helium\Application\chrome.exe",
    r"C:\Program Files (x86)\Steam\steam.exe",
    r"C:\Apps\Editor\Code.exe",
    r"C:\Program Files\Music\player.exe",
    r"C:\Tools\plain.exe",
];

impl MonitorApp {
    /// Isolated visual-development harness: no tray, capture, user settings or history writes.
    #[cfg(any(test, feature = "ui-preview"))]
    pub fn preview(ctx: &egui::Context, page: &str, light: bool) -> Self {
        crate::ui::text::install_fonts(ctx);
        let settings = Settings {
            appearance: Some(if light {
                Appearance::Light
            } else {
                Appearance::Dark
            }),
            close_to_tray: false,
            ..Default::default()
        };
        let tokens = settings.tokens(ctx);
        crate::ui::tokens::install(ctx, tokens);
        ctx.set_zoom_factor(settings.ui_scale);
        let mut app = Self {
            worker: Worker::for_ui_test(),
            icons: IconCache::disabled(),
            startup: crate::startup::Controller::preview(),
            tray: None,
            data: Shared::default(),
            settings,
            page: match page {
                "connections" => Page::Connections,
                "adapters" => Page::Adapters,
                "settings" => Page::Settings,
                _ => Page::Overview,
            },
            show_down: true,
            show_up: true,
            log: false,
            paused: false,
            frozen: None,
            filter_revision: 0,
            selected_app: None,
            connection_search: String::new(),
            app_search: String::new(),
            storage: Some((
                std::time::Instant::now(),
                super::logic::StorageUsage {
                    history: 134_300_000,
                    settings: 100_000,
                    other: 0,
                },
            )),
            data_dir: PathBuf::from(r"C:\Users\Example\AppData\Local\OpenNetworkMonitor"),
            view_warning: String::new(),
            custom_period: None,
            app_cache: Arc::default(),
            app_cache_key: None,
            history_totals: Default::default(),
            dialogs: Default::default(),
            connection_cache: Default::default(),
            connection_sort: None,
            protocol_filter: Default::default(),
            status_filter: Default::default(),
            pending_end: None,
            timeline_mode: Default::default(),
            focus_sent: None,
            alerts_sent: Default::default(),
            notice_seen: None,
            top_apps_view: TopAppsView::Top,
            min_size_for: None,
            pause_sent: false,
            tooltip_revision: 0,
            compaction_sent: None,
            last_ui: Instant::now(),
            shown_sent: None,
            hosts_lifetime: false,
            lifetime_sent: false,
            taskbar_dark: true,
            cap_input: "100".into(),
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
            titlebar: None,
            applied: tokens,
            window_record: None,
        };
        if page == "apps" {
            app.settings.filter.source = Source::Transport;
            // Generated sample icons keep previews independent of installed apps.
            for (i, path) in PREVIEW_PATHS.iter().enumerate() {
                let colors = [
                    Color32::from_rgb(68, 137, 117),
                    Color32::from_rgb(76, 105, 143),
                    Color32::from_rgb(145, 109, 158),
                    Color32::from_rgb(183, 119, 77),
                    Color32::from_rgb(105, 112, 125),
                ];
                let mut pixels = vec![Color32::TRANSPARENT; 32 * 32];
                for y in 3..29 {
                    for x in 3..29 {
                        pixels[y * 32 + x] = if (9..23).contains(&x) && (9..23).contains(&y) {
                            Color32::from_rgb(235, 239, 239)
                        } else {
                            colors[i]
                        };
                    }
                }
                app.icons
                    .insert_preview(ctx, path, egui::ColorImage::new([32, 32], pixels));
            }
        }
        app.refresh_preview();
        app
    }
    #[cfg(any(test, feature = "ui-preview"))]
    pub fn refresh_preview(&mut self) {
        use std::net::IpAddr;
        if self.data.history_revision == self.filter_revision && !self.data.history.rows.is_empty()
        {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        self.data.lifetime_hosts = Arc::new(
            [
                ("203.0.113.1", 612_000_000_000_u64, 9_100_000_000_u64, 3),
                ("203.0.113.2", 48_000_000_000, 320_000_000, 12),
                ("198.51.100.7", 6_300_000_000, 88_000_000, 31),
                ("198.51.100.9", 910_000_000, 14_000_000, 60),
            ]
            .map(|(remote, down, up, days_ago)| crate::model::HostTotal {
                remote: remote.into(),
                country: String::new(),
                down,
                up,
                first_seen: now - 90 * 86_400,
                last_seen: now - days_ago * 86_400,
            })
            .to_vec(),
        );
        let paths = PREVIEW_PATHS;
        let names = [
            "Helium",
            "Steam",
            "Visual Studio Code",
            "Music Player",
            "plain.exe",
        ];
        let mut rows = Vec::new();
        for i in 0..35 {
            let path = paths[i % paths.len()].to_owned();
            Arc::make_mut(&mut self.data.snapshot)
                .app_labels
                .insert(path.clone(), names[i % names.len()].into());
            let remote = format!("203.0.113.{}", i + 1);
            if i < 4 {
                Arc::make_mut(&mut self.data.snapshot).host_labels.insert(
                    remote.clone(),
                    [
                        "media.example.net",
                        "downloads.example.org",
                        "api.example.com",
                        "updates.example.net",
                    ][i]
                        .into(),
                );
            }
            rows.push(UsageRow {
                app: path,
                adapter_id: "ethernet".into(),
                adapter: "Ethernet".into(),
                source: self.settings.filter.source.key().into(),
                remote: remote.clone(),
                pid: 4200 + i as u32,
                traffic_type: ["HTTPS / TLS", "HTTP", "DNS", "QUIC", "Other TCP"][i % 5].into(),
                protocol: "TCP".into(),
                scope: "WAN".into(),
                down: ((35 - i) as u64).pow(3) * 335_601,
                up: (i + 1) as u64 * 543_210,
                ..Default::default()
            });
        }
        let mut totals = BTreeMap::<String, AppUsage>::new();
        for r in &rows {
            let a = totals.entry(r.app.clone()).or_insert_with(|| AppUsage {
                app: r.app.clone(),
                ..Default::default()
            });
            a.down += r.down;
            a.up += r.up;
        }
        self.data.top_apps = totals.into_values().collect();
        self.data
            .top_apps
            .sort_by_key(|a| std::cmp::Reverse(a.down + a.up));
        self.data.top_apps.truncate(5);
        self.data.recent_apps = PREVIEW_PATHS
            .iter()
            .zip([40, 400, 5_000, 90_000, 700_000])
            .map(|(path, age)| RecentApp {
                app: (*path).to_owned(),
                last_seen: now - age,
            })
            .collect();
        Arc::make_mut(&mut self.data.snapshot).connections = (0..85)
            .map(|i| Connection {
                pid: 4200 + i as u32,
                app: paths[i % paths.len()].into(),
                local: if i % 2 == 0 {
                    "192.168.1.3:52744".into()
                } else {
                    "[2001:db8:84a3:8d3::7344]:52712".into()
                },
                remote: if i % 2 == 0 {
                    format!("203.0.113.{}:443", i + 1)
                } else {
                    "[2001:db8:84a3:8d3::1121]:443".into()
                },
                protocol: if i % 3 == 0 { "UDP" } else { "TCP" }.into(),
                country: if i % 3 == 0 { "Unknown" } else { "Germany" }.into(),
                state: if i % 3 == 0 { "Bound" } else { "Established" }.into(),
            })
            .collect();
        Arc::make_mut(&mut self.data.snapshot).adapters = [
            (
                "ethernet",
                "Ethernet",
                "Intel Ethernet Controller",
                true,
                true,
            ),
            (
                "wifi",
                "Wi-Fi",
                "Intel Wi-Fi 6 Wireless Adapter",
                false,
                true,
            ),
            (
                "vpn",
                "VPN",
                "Virtual private network adapter",
                false,
                false,
            ),
            (
                "bt",
                "Bluetooth Network Connection",
                "Bluetooth Device (Personal Area Network)",
                false,
                false,
            ),
            (
                "usb",
                "USB Ethernet",
                "Realtek USB GbE Family Controller",
                true,
                true,
            ),
        ]
        .into_iter()
        .map(|(id, name, description, up, physical)| Adapter {
            id: id.into(),
            name: name.into(),
            description: description.into(),
            up,
            physical,
            addresses: if up {
                vec![
                    ("192.168.1.3".parse::<IpAddr>().unwrap(), 24),
                    ("2001:db8:84a3:8d3::7344".parse::<IpAddr>().unwrap(), 64),
                ]
            } else {
                vec![]
            },
            ..Default::default()
        })
        .collect();
        Arc::make_mut(&mut self.data.snapshot).wifi_networks = vec![
            WifiNetwork {
                id: "sample-home".into(),
                name: "Home Wi-Fi".into(),
            },
            WifiNetwork {
                id: "sample-office".into(),
                name: "Office Wi-Fi".into(),
            },
        ];
        // Live speeds, from both capture sources as the collector reports them.
        let live: Vec<UsageRow> = rows
            .iter()
            .cloned()
            .map(|r| UsageRow {
                down: r.down / 2000,
                up: r.up / 50,
                ..r
            })
            .collect();
        let other_source = if self.settings.filter.source == Source::Transport {
            Source::Adapter
        } else {
            Source::Transport
        };
        Arc::make_mut(&mut self.data.snapshot).live_rows = live
            .iter()
            .cloned()
            .chain(live.iter().map(|r| UsageRow {
                source: other_source.key().into(),
                ..r.clone()
            }))
            .collect();
        Arc::make_mut(&mut self.data.snapshot).interval = 1.0;
        Arc::make_mut(&mut self.data.snapshot).etw_active = true;
        Arc::make_mut(&mut self.data.snapshot).capture_status =
            "Detailed monitoring is active".into();
        Arc::make_mut(&mut self.data.snapshot).parsed_events = 1_482_071;
        let points = (0..300)
            .map(|i| Point {
                timestamp: now - 300 + i,
                down: ((i as f64 * 0.11).sin().abs() * 8_000_000.0) as u64 + 160_000,
                up: ((i as f64 * 0.07).sin().abs() * 950_000.0) as u64,
                observed: true,
            })
            .collect();
        self.data.classified = ScopeTotals {
            lan: 68_000_000,
            wan: self
                .data
                .top_apps
                .iter()
                .map(|a| a.down + a.up)
                .sum::<u64>()
                .saturating_sub(68_000_000),
            unknown: 0,
        };
        let (start, end) = if self.settings.filter.all_time() {
            (now - 3600, now)
        } else {
            self.settings.filter.time_bounds(now)
        };
        self.data.history = Arc::new(History {
            window_start: start,
            window_end: end,
            down: rows.iter().map(|r| r.down).sum(),
            up: rows.iter().map(|r| r.up).sum(),
            rows,
            points,
        });
        self.data.history_revision = self.filter_revision;
        self.data.history_generation += 1;
        self.data.revision += 1;
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::ui::scene::{self, Step};

    type Setup = fn(&mut MonitorApp);

    #[test]
    #[ignore = "Exports the app's own headless egui geometry for offline visual review"]
    #[allow(clippy::type_complexity)]
    fn export_ui_review() {
        let wide = DEFAULT_WINDOW_SIZE;
        let small = MIN_WINDOW_SIZE;
        let scenes: Vec<(&str, &str, bool, [f32; 2], Vec<Step>, Setup)> = vec![
            ("overview", "overview", false, wide, vec![], |_| {}),
            ("overview-light", "overview", true, wide, vec![], |_| {}),
            ("overview-small", "overview", false, small, vec![], |_| {}),
            ("overview-violet", "overview", false, wide, vec![], |app| {
                app.settings.base_color = BaseColor::Violet;
            }),
            (
                "overview-green-light",
                "overview",
                true,
                wide,
                vec![],
                |app| {
                    app.settings.base_color = BaseColor::Green;
                },
            ),
            ("overview-recent", "overview", false, wide, vec![], |app| {
                app.top_apps_view = TopAppsView::Recent;
            }),
            (
                "overview-small-light",
                "overview",
                true,
                small,
                vec![],
                |_| {},
            ),
            ("apps", "apps", false, wide, vec![], |_| {}),
            ("apps-light", "apps", true, wide, vec![], |_| {}),
            ("apps-small", "apps", false, small, vec![], |_| {}),
            ("apps-lifetime", "apps", false, wide, vec![], |app| {
                app.hosts_lifetime = true;
            }),
            ("apps-timeline", "apps", false, wide, vec![], |app| {
                select_timeline(app, false);
            }),
            ("apps-timeline-daily", "apps", true, wide, vec![], |app| {
                select_timeline(app, true);
            }),
            ("connections", "connections", false, wide, vec![], |_| {}),
            (
                "connections-light",
                "connections",
                true,
                small,
                vec![],
                |_| {},
            ),
            ("adapters", "adapters", false, wide, vec![], |_| {}),
            ("adapters-small", "adapters", true, small, vec![], |_| {}),
            (
                "settings",
                "settings",
                false,
                [1200.0, 1900.0],
                vec![],
                |app| {
                    app.settings.storage_open = true;
                    app.settings.cap_enabled = true;
                    app.settings.alert_new_apps = true;
                    app.data.cap = Some(crate::worker::CapStatus {
                        used: 86 * 1_073_741_824,
                        limit: 100 * 1_073_741_824,
                        cycle_start: chrono::Utc::now().timestamp() - 20 * 86_400,
                    });
                },
            ),
            (
                "settings-light",
                "settings",
                true,
                [1200.0, 1900.0],
                vec![],
                |_| {},
            ),
            ("intro", "overview", false, wide, vec![], |app| {
                app.settings.intro_seen = Some(false);
            }),
            (
                "menu-period",
                "overview",
                false,
                wide,
                vec![Step::Click("Period")],
                |_| {},
            ),
            (
                "menu-adapters-light",
                "apps",
                true,
                wide,
                vec![Step::Click("Adapters")],
                |_| {},
            ),
            ("period-dialog", "overview", false, small, vec![], |app| {
                app.custom_period = Some(CustomPeriod::from_filter(&app.settings.filter));
            }),
            (
                "period-dialog-light",
                "overview",
                true,
                small,
                vec![],
                |app| {
                    let mut period = CustomPeriod::from_filter(&app.settings.filter);
                    period.mode = PeriodMode::Dates;
                    app.custom_period = Some(period);
                },
            ),
            ("end-process", "connections", false, small, vec![], |app| {
                app.pending_end = Some((4242, "Steam".into()));
            }),
        ];
        for (name, page, light, size, steps, setup) in scenes {
            let ctx = egui::Context::default();
            let mut app = MonitorApp::preview(&ctx, page, light);
            setup(&mut app);
            // A scene may change the accent colour, so apply the settings' tokens again.
            crate::ui::tokens::install(&ctx, app.settings.tokens(&ctx));
            let scale = app.settings.ui_scale;
            scene::export_in(&ctx, name, size, scale, &steps, |ui| {
                app.refresh_preview();
                app.draw(ui);
            });
        }
    }

    /// Selects the first sample app and fills its timeline with sample bars.
    fn select_timeline(app: &mut MonitorApp, daily: bool) {
        let now = chrono::Utc::now().timestamp() / 60 * 60;
        app.selected_app = Some(PREVIEW_PATHS[1].to_owned());
        let (view, step, span) = if daily {
            app.timeline_mode = super::app_timeline::TimelineMode::Daily;
            (crate::model::SeriesView::Daily, 3600, 30 * 86_400)
        } else {
            (crate::model::SeriesView::Period, 300, 86_400)
        };
        app.data.app_series = Some(Arc::new(crate::model::AppSeries {
            app: PREVIEW_PATHS[1].to_owned(),
            view,
            read_at: 0,
            revision: app.filter_revision,
            step,
            window: (now - span, now),
            points: (0..span / step)
                .filter(|i| i % 7 != 3)
                .map(|i| {
                    let wave = ((i as f64 / 14.0).sin().abs() * 9_000_000.0) as u64;
                    (now - span + i * step, wave + 400_000, wave / 6 + 50_000)
                })
                .collect(),
        }));
    }
}
