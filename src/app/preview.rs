use super::*;

const PREVIEW_PATHS: [&str; 5] = [
    r"C:\Program Files\Helium\Application\chrome.exe",
    r"C:\Program Files (x86)\Steam\steam.exe",
    r"C:\Apps\Editor\Code.exe",
    r"C:\Program Files\Music\player.exe",
    r"C:\Tools\plain.exe",
];

impl MonitorApp {
    /// Isolated visual-development harness: no tray, capture, user settings or history writes.
    #[cfg(feature = "ui-preview")]
    pub fn preview(ctx: &egui::Context, page: &str, light: bool) -> Self {
        install_fonts(ctx);
        let settings = Settings {
            theme: if light { Theme::Light } else { Theme::Dark },
            close_to_tray: false,
            ..Default::default()
        };
        settings.theme.apply(ctx);
        ctx.set_zoom_factor(settings.ui_scale);
        let mut app = Self {
            worker: Worker::for_ui_test(),
            icons: IconCache::disabled(),
            tray: None,
            data: Shared::default(),
            settings,
            page: match page {
                "connections" => Page::Connections,
                "adapters" => Page::Adapters,
                "settings" => Page::Settings,
                _ => Page::Overview,
            },
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
            data_dir: PathBuf::from(r"C:\Users\Example\AppData\Local\OpenNetworkMonitor"),
            view_warning: String::new(),
            show_graph_options: false,
            app_cache: Arc::default(),
            app_cache_key: None,
            breakdown_cache: Arc::default(),
            breakdown_cache_key: None,
            host_request: None,
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
    #[cfg(feature = "ui-preview")]
    pub fn refresh_preview(&mut self) {
        use std::net::IpAddr;
        if self.data.history_revision == self.filter_revision && !self.data.history.rows.is_empty()
        {
            return;
        }
        let now = chrono::Utc::now().timestamp();
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
            self.data
                .snapshot
                .app_labels
                .insert(path.clone(), names[i % names.len()].into());
            let remote = format!("203.0.113.{}", i + 1);
            if i < 4 {
                self.data.snapshot.host_labels.insert(
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
        self.data.snapshot.connections = (0..85)
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
        self.data.snapshot.adapters = [
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
        self.data.snapshot.wifi_networks = vec![
            WifiNetwork {
                id: "sample-home".into(),
                name: "Home Wi-Fi".into(),
            },
            WifiNetwork {
                id: "sample-office".into(),
                name: "Office Wi-Fi".into(),
            },
        ];
        self.data.snapshot.live_rows = rows
            .iter()
            .cloned()
            .map(|r| UsageRow {
                down: r.down / 2000,
                up: r.up / 50,
                ..r
            })
            .collect();
        self.data.snapshot.interval = 1.0;
        self.data.snapshot.etw_active = true;
        self.data.snapshot.capture_status = "Detailed monitoring is active".into();
        self.data.snapshot.parsed_events = 1_482_071;
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
        self.data.history = Arc::new(History {
            window_start: now - self.settings.filter.seconds,
            window_end: now,
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
    #[test]
    #[ignore = "Exports the app's own headless egui geometry for offline visual review"]
    fn export_ui_review() {
        use std::collections::HashMap;
        let output_dir = PathBuf::from("target/ui-review");
        std::fs::create_dir_all(&output_dir).unwrap();
        for (name, page, light, size) in [
            ("apps", "apps", false, [1200.0, 820.0]),
            ("apps-small", "apps", false, [960.0, 680.0]),
            ("adapter-small", "overview", false, [960.0, 680.0]),
            ("overall-other", "overview", false, [960.0, 680.0]),
            (
                "overall-smooth",
                "overview",
                false,
                widgets::DEFAULT_WINDOW_SIZE,
            ),
            (
                "overall-wide",
                "overview",
                false,
                widgets::DEFAULT_WINDOW_SIZE,
            ),
            ("connections", "connections", false, [1200.0, 820.0]),
            ("adapters", "adapters", false, [960.0, 680.0]),
            ("settings", "settings", false, [1200.0, 820.0]),
            ("apps-light", "apps", true, [1200.0, 820.0]),
        ] {
            let ctx = egui::Context::default();
            let mut app = MonitorApp::preview(&ctx, page, light);
            if name == "overall-smooth" {
                app.settings.filter.seconds = 300;
                let history = Arc::make_mut(&mut app.data.history);
                history.window_start = history.window_end - 300;
            }
            if name == "overall-other" {
                let other = app.data.classified.wan / 10;
                app.data.classified.wan -= other;
                app.data.classified.unknown = other;
            }
            let mut textures = HashMap::<String, ([usize; 2], Vec<[u8; 4]>)>::new();
            let mut geometry = Vec::new();
            for _ in 0..3 {
                let mut out = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::from(size) / app.settings.ui_scale,
                        )),
                        ..Default::default()
                    },
                    |root| {
                        app.draw(root);
                    },
                );
                for (id, deltas) in &out.textures_delta.set {
                    for delta in deltas {
                        let egui::ImageData::Color(image) = &delta.image;
                        let key = format!("{id:?}");
                        if let Some(offset) = delta.pos {
                            let (target_size, target) = textures.get_mut(&key).unwrap();
                            for y in 0..image.size[1] {
                                for x in 0..image.size[0] {
                                    target[(offset[1] + y) * target_size[0] + offset[0] + x] =
                                        image.pixels[y * image.size[0] + x].to_array();
                                }
                            }
                        } else {
                            textures.insert(
                                key,
                                (
                                    image.size,
                                    image.pixels.iter().map(|c| c.to_array()).collect(),
                                ),
                            );
                        }
                    }
                }
                let primitives = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
                geometry=primitives.into_iter().filter_map(|p|match p.primitive {
                    egui::epaint::Primitive::Mesh(mesh)=>Some(serde_json::json!({"clip":[p.clip_rect.min.x*out.pixels_per_point,p.clip_rect.min.y*out.pixels_per_point,p.clip_rect.max.x*out.pixels_per_point,p.clip_rect.max.y*out.pixels_per_point],"texture":format!("{:?}",mesh.texture_id),"vertices":mesh.vertices.iter().map(|v|(v.pos.x*out.pixels_per_point,v.pos.y*out.pixels_per_point,v.uv.x,v.uv.y,v.color.to_array())).collect::<Vec<_>>(),"indices":mesh.indices})),
                    _=>None,
                }).collect();
                out.textures_delta.clear();
            }
            let texture_json: Vec<_> = textures
                .into_iter()
                .enumerate()
                .map(|(n, (id, (size, pixels)))| {
                    let file = format!("{name}-atlas-{n}.rgba");
                    std::fs::write(
                        output_dir.join(&file),
                        pixels.into_iter().flatten().collect::<Vec<_>>(),
                    )
                    .unwrap();
                    serde_json::json!({"id":id,"size":size,"file":file})
                })
                .collect();
            std::fs::write(
                output_dir.join(format!("{name}.json")),
                serde_json::to_vec(
                    &serde_json::json!({"size":size,"textures":texture_json,"meshes":geometry}),
                )
                .unwrap(),
            )
            .unwrap();
        }
    }
}
