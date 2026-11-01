use super::*;
use crate::widgets::Palette;

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
        custom_period: None,
        app_cache: Arc::default(),
        app_cache_key: None,
        history_totals: Default::default(),
        dialogs: Default::default(),
        connection_cache: Default::default(),
        breakdown_cache: Arc::default(),
        breakdown_cache_key: None,
        host_request: None,
        titlebar: None,
        window_record: None,
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
    Arc::make_mut(&mut app.data.snapshot).app_labels.insert(
        r"C:\Apps\Long application directory 0\plain0.exe".into(),
        "Friendly Browser".into(),
    );
    app
}
fn context(theme: Theme) -> egui::Context {
    let ctx = egui::Context::default();
    theme.apply(&ctx);
    install_fonts(&ctx);
    ctx
}
fn screen(size: Vec2, events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        events,
        ..Default::default()
    }
}
fn app_frame(
    app: &mut MonitorApp,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, f32, f32) {
    let mut bottom = 0.0;
    let mut limit = 0.0;
    let out = ctx.run_ui(screen(size, events), |root| {
        egui::Panel::left("test-navigation")
            .resizable(false)
            .exact_size(178.0)
            .show(root, |_| {});
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(16))
            .show(root, |ui| {
                app.page_header(ui);
                limit = ui.max_rect().bottom();
                app.overview(ui);
                bottom = ui.min_rect().bottom();
            });
    });
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
fn shell_frame(
    app: &mut MonitorApp,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(screen(size, events), |root| {
        app.draw(root);
    })
}
fn click_app_control(app: &mut MonitorApp, ctx: &egui::Context, label: &str) -> egui::FullOutput {
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
/// Settles layout, opens whatever `label` opens, and returns the next frame.
fn open_control(app: &mut MonitorApp, ctx: &egui::Context, label: &str) -> egui::FullOutput {
    let mut out = click_app_control(app, ctx, label);
    out.textures_delta.clear();
    let (out, _, _) = app_frame(app, ctx, Vec2::new(1200.0, 820.0), vec![]);
    out
}

#[test]
fn adapter_dashboard_fits_without_scrolling_at_supported_window_sizes() {
    for (width, height) in [(960.0, 680.0), (1200.0, 820.0), (1440.0, 900.0)] {
        let ctx = context(Theme::Dark);
        let mut app = fixture();
        let (mut out, bottom, limit) = app_frame(&mut app, &ctx, Vec2::new(width, height), vec![]);
        out.textures_delta.clear();
        assert!(
            bottom <= limit + 1.0,
            "Dashboard overflows at {width}x{height}: {bottom} > {limit}"
        );
    }
}
#[test]
fn app_icon_sits_before_the_name_and_remains_part_of_the_clickable_row() {
    fn find_icon(shape: &egui::Shape, texture: egui::TextureId) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find_icon(shape, texture)),
            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
            _ => None,
        }
    }
    let ctx = context(Theme::Dark);
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
        let ctx = context(Theme::Dark);
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
        let (apps, hosts, types) = (at("Apps"), at("Hosts"), at("Traffic type"));
        assert!(apps.x < hosts.x && hosts.x < types.x);
        // The three lists share the width equally, so their headings are evenly spaced.
        assert!(
            ((hosts.x - apps.x) - (types.x - hosts.x)).abs() <= 1.5,
            "Columns differ: {} then {}",
            hosts.x - apps.x,
            types.x - hosts.x
        );
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
    let ctx = context(Theme::Dark);
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
    let (mut out, _, _) = app_frame(&mut app, &ctx, size, vec![egui::Event::PointerMoved(pos)]);
    out.textures_delta.clear();
    let (mut out, _, _) = app_frame(&mut app, &ctx, size, click_events(pos));
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
    let (mut out, _, _) = app_frame(&mut app, &ctx, size, click_events(pos));
    out.textures_delta.clear();
    assert!(app.selected_app.is_none());
    assert_eq!(app.traffic_breakdown(&totals).hosts.len(), 80);
}
#[test]
fn every_page_uses_the_same_bounded_shell_in_all_themes() {
    for theme in Theme::ALL {
        for size in [Vec2::new(960.0, 680.0), Vec2::new(1440.0, 900.0)] {
            for (page, source) in [
                (Page::Overview, Source::Adapter),
                (Page::Overview, Source::Transport),
                (Page::Connections, Source::Transport),
                (Page::Adapters, Source::Adapter),
                (Page::Settings, Source::Adapter),
            ] {
                let ctx = context(theme);
                let mut app = app_fixture();
                app.page = page;
                app.settings.filter.source = source;
                app.settings.theme = theme;
                Arc::make_mut(&mut app.data.snapshot).etw_active = true;
                Arc::make_mut(&mut app.data.snapshot).connections = (0..65)
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
                Arc::make_mut(&mut app.data.snapshot).adapters = (0..8)
                    .map(|i| Adapter {
                        id: i.to_string(),
                        name: format!("Ethernet {i}"),
                        description: "Long network controller description".into(),
                        up: i == 0,
                        addresses: vec![
                            ("2001:db8:84a3:8d3:1319:8a2e:370:7344".parse().unwrap(), 64),
                            ("192.168.1.3".parse().unwrap(), 24),
                            ("192.168.1.4".parse().unwrap(), 24),
                        ],
                        ..Default::default()
                    })
                    .collect();
                let mut bounds = egui::Rect::NOTHING;
                let mut out = ctx.run_ui(screen(size, vec![]), |root| {
                    bounds = app.draw(root);
                });
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
        let cells = connections::connection_columns(rect);
        assert!(
            cells
                .windows(2)
                .all(|pair| pair[0].right() + 7.0 <= pair[1].left())
        );
        assert!(cells.last().unwrap().right() <= rect.right());
    }
}
#[test]
fn sidebar_and_adapter_shortcut_keep_app_traffic_in_overview() {
    let ctx = context(Theme::Dark);
    let size = Vec2::new(1200.0, 820.0);
    let mut app = app_fixture();
    Arc::make_mut(&mut app.data.snapshot).etw_active = true;
    Arc::make_mut(&mut app.data.snapshot).adapters = vec![Adapter {
        id: "eth".into(),
        name: "Ethernet".into(),
        ..Default::default()
    }];
    for _ in 0..2 {
        shell_frame(&mut app, &ctx, size, vec![])
            .textures_delta
            .clear();
    }
    for (title, expected) in [
        ("Connections", Page::Connections),
        ("Adapters", Page::Adapters),
        ("Settings", Page::Settings),
        ("Overview", Page::Overview),
        ("Adapters", Page::Adapters),
    ] {
        let mut out = shell_frame(&mut app, &ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, p)| s == title && p.x < widgets::NAV_WIDTH)
            .unwrap()
            .1
            + Vec2::new(6.0, 6.0);
        out.textures_delta.clear();
        shell_frame(&mut app, &ctx, size, click_events(pos))
            .textures_delta
            .clear();
        assert!(app.page == expected, "Sidebar {title} did not navigate");
    }
    let mut out = shell_frame(&mut app, &ctx, size, vec![]);
    let pos = text_positions(&out)
        .into_iter()
        .find(|(s, _)| s == "View traffic")
        .unwrap()
        .1
        + Vec2::new(5.0, 5.0);
    out.textures_delta.clear();
    shell_frame(&mut app, &ctx, size, click_events(pos))
        .textures_delta
        .clear();
    assert!(app.page == Page::Overview);
    assert_eq!(app.settings.filter.source, Source::Adapter);
    assert_eq!(app.settings.filter.adapters, HashSet::from(["eth".into()]));
}
#[test]
fn sidebar_collapses_to_an_icon_rail_when_the_window_is_narrow() {
    let nav_labels = |size: Vec2| {
        let ctx = context(Theme::Dark);
        let mut app = fixture();
        let mut out = shell_frame(&mut app, &ctx, size, vec![]);
        let text = text_positions(&out);
        out.textures_delta.clear();
        text.into_iter()
            .filter(|(s, p)| {
                p.x < widgets::NAV_RAIL_WIDTH
                    && ["Connections", "Adapters", "Settings"].contains(&s.as_str())
            })
            .count()
    };
    assert_eq!(nav_labels(Vec2::new(960.0, 680.0)), 0);
    assert_eq!(nav_labels(Vec2::new(1440.0, 820.0)), 3);
}
#[test]
fn the_source_switch_changes_between_overall_and_app_traffic() {
    let ctx = context(Theme::Dark);
    let size = Vec2::new(1200.0, 820.0);
    let mut app = fixture();
    for _ in 0..2 {
        shell_frame(&mut app, &ctx, size, vec![])
            .textures_delta
            .clear();
    }
    for (label, source) in [
        ("App traffic", Source::Transport),
        ("Overall Traffic", Source::Adapter),
    ] {
        let mut out = shell_frame(&mut app, &ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, _)| s == label)
            .unwrap()
            .1
            + Vec2::new(4.0, 4.0);
        out.textures_delta.clear();
        shell_frame(&mut app, &ctx, size, click_events(pos))
            .textures_delta
            .clear();
        assert_eq!(app.settings.filter.source, source);
    }
}
#[test]
fn every_palette_keeps_text_and_data_colours_readable_on_every_surface() {
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
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    for theme in Theme::ALL {
        let p: Palette = theme.palette();
        for (surface, background) in [
            ("canvas", p.canvas),
            ("card", p.card),
            ("raised", p.raised),
            ("selected", p.accent_soft),
        ] {
            for (label, color) in [
                ("text", p.text),
                ("dim", p.dim),
                ("download", p.down),
                ("upload", p.up),
                ("accent", p.accent),
                ("good", p.good),
                ("warn", p.warn),
                ("bad", p.bad),
            ] {
                let ratio = contrast(background, color);
                assert!(
                    ratio >= 4.5,
                    "{theme:?} {label} on {surface} has contrast {ratio:.2}"
                );
            }
        }
        let ratio = contrast(p.accent, p.accent_ink);
        assert!(
            ratio >= 4.5,
            "{theme:?} accent button text contrast {ratio:.2}"
        );
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
fn saved_theme_names_still_load_after_the_redesign() {
    for (name, theme) in [
        ("Dark", Theme::Dark),
        ("Light", Theme::Light),
        ("Nord", Theme::Nord),
        ("Mocha", Theme::Mocha),
    ] {
        let saved: Settings = serde_json::from_str(&format!(r#"{{"theme":"{name}"}}"#)).unwrap();
        assert_eq!(saved.theme, theme);
    }
}
#[test]
fn summary_keeps_usage_separate_from_captured_destinations() {
    for width in [1200.0, 720.0] {
        for unknown in [0, 500_000_000] {
            let ctx = context(Theme::Dark);
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
            let mut out = ctx.run_ui(screen(Vec2::new(width + 20.0, 260.0), vec![]), |root| {
                root.spacing_mut().item_spacing.y = 0.0;
                summary = root
                    .allocate_ui(Vec2::new(width, 220.0), |ui| app.summary_card(ui))
                    .response
                    .rect;
            });
            let labels = text_positions(&out);
            out.textures_delta.clear();
            assert!(
                summary.height() <= MonitorApp::SUMMARY_CONTENT_HEIGHT + 40.0
                    && summary.right() <= width + 1.0,
                "Summary grew to {summary:?}"
            );
            for required in [
                "Total usage",
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
                    "Missing {required} at width {width}"
                );
            }
            assert!(
                labels.iter().any(|(text, _)| text.ends_with(" captured")),
                "App-byte total needs its own label"
            );
            assert_eq!(labels.iter().any(|(text, _)| text == "Other"), unknown > 0);
            assert!(!labels.iter().any(|(text, _)| text == "app5.exe"));
            let at = |needle: &str| labels.iter().find(|(text, _)| text == needle).unwrap().1;
            let (usage, destinations, apps) =
                (at("Total usage"), at("LAN / WAN"), at("Top 5 apps"));
            assert!(usage.x < destinations.x && destinations.x < apps.x);
            // Each section is centred in its cell, so shorter ones sit lower than the full list.
            let totals_content = if width > 1000.0 { 84.0 } else { 138.0 };
            let offset = |content| MonitorApp::centred(content);
            assert!(
                (usage.y - apps.y - offset(totals_content)).abs() < 3.0,
                "Total usage sits {} below the apps heading",
                usage.y - apps.y
            );
            assert!(
                (destinations.y - apps.y - offset(crate::graph::DESTINATION_HEIGHT)).abs() < 3.0,
                "LAN / WAN sits {} below the apps heading",
                destinations.y - apps.y
            );
            let (down, up) = (at("Downloaded"), at("Uploaded"));
            if width > 1000.0 {
                assert!(down.y == up.y && down.x < up.x, "Wide totals share a row");
            } else {
                assert!(down.x == up.x && down.y < up.y, "Narrow totals stack");
            }
        }
    }
}
#[test]
fn the_chart_sits_above_the_summary_and_fills_the_remaining_height() {
    let ctx = context(Theme::Dark);
    let size = Vec2::new(1200.0, 820.0);
    let mut app = fixture();
    app.data.top_apps = vec![AppUsage {
        app: "app0.exe".into(),
        down: 1000,
        up: 10,
    }];
    let (mut out, bottom, limit) = app_frame(&mut app, &ctx, size, vec![]);
    let labels = text_positions(&out);
    out.textures_delta.clear();
    let at = |needle: &str| labels.iter().find(|(text, _)| text == needle).unwrap().1;
    // The legend heads the chart; the summary headings come after the whole plot.
    assert!(at("Download").y < at("Total usage").y);
    assert!(
        at("0 B/s").y < at("Total usage").y,
        "Axis labels end above the summary"
    );
    assert!(
        at("Total usage").y > size.y * 0.6,
        "The summary is the bottom row"
    );
    assert!(bottom <= limit + 1.0);
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
            let ctx = context(Theme::Dark);
            ctx.set_zoom_factor(scale);
            let size = Vec2::new(960.0, 680.0);
            ctx.run_ui(screen(size, vec![]), |_| {})
                .textures_delta
                .clear();
            let mut app = app_fixture();
            app.page = page;
            app.settings.filter.source = source;
            app.settings.ui_scale = scale;
            let mut bounds = egui::Rect::NOTHING;
            let mut out = ctx.run_ui(screen(size / scale, vec![]), |root| {
                bounds = app.draw(root);
            });
            assert!((out.pixels_per_point - scale).abs() < 0.001);
            out.textures_delta.clear();
            assert!(
                bounds.right() * scale <= size.x + 1.0 && bounds.bottom() * scale <= size.y + 1.0,
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
    assert_eq!(apps::traffic_bar_segments(rect, 0, 0), [None, None]);
    assert_eq!(apps::traffic_bar_segments(rect, 1, 0), [Some(rect), None]);
    assert_eq!(apps::traffic_bar_segments(rect, 0, 1), [None, Some(rect)]);
    for (down, up, expected_fraction) in [(3, 1, 0.75), (1, 3, 0.25), (u64::MAX, u64::MAX, 0.5)] {
        let [Some(download), Some(upload)] = apps::traffic_bar_segments(rect, down, up) else {
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
        let ctx = context(Theme::Dark);
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
    let ctx = context(Theme::Dark);
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
    let (period, adapters, network) = (at("Period"), at("Adapters"), at("Networks"));
    assert!(at("1h").x > period.x);
    assert!(period.x < adapters.x && adapters.x < network.x && network.x < search.x);
    assert!(
        [period, adapters, network]
            .iter()
            .all(|p| (p.y - search.y).abs() < 3.0)
    );
    let count = text
        .iter()
        .filter(|(s, p)| {
            (s == "Friendly Browser" || (s.starts_with("plain") && s.ends_with(".exe")))
                && p.y < size.y - 20.0
        })
        .count();
    assert!(count >= 9, "Only {count} apps fit in the dense list");
}
#[test]
fn period_menu_is_a_compact_grid_with_calendar_choices_below() {
    let ctx = context(Theme::Dark);
    let mut app = app_fixture();
    let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    warm.textures_delta.clear();
    let mut output = open_control(&mut app, &ctx, "Period");
    let positions = text_positions(&output);
    output.textures_delta.clear();
    let at = |label: &str| {
        positions
            .iter()
            .rfind(|(text, _)| text == label)
            .map(|(_, position)| *position)
            .unwrap_or_else(|| panic!("Missing period {label}"))
    };
    let (five, fifteen, three_hours) = (at("5m"), at("15m"), at("3h"));
    assert!(
        (five.y - fifteen.y).abs() < 1.0 && (fifteen.x - five.x) > 40.0,
        "Quick ranges share a row"
    );
    assert!(three_hours.y - five.y >= 30.0 && three_hours.y - five.y <= 40.0);
    assert!(at("Since beginning of month").y > three_hours.y);
    assert!(at("Custom range…").y > at("Since beginning of month").y);
}
#[test]
fn expanded_period_menu_supports_both_views_and_custom_cancel() {
    for source in [Source::Adapter, Source::Transport] {
        let ctx = context(Theme::Dark);
        let mut app = app_fixture();
        app.settings.filter.source = source;
        for (choice, seconds) in [
            ("15m", 900),
            ("30m", 1800),
            ("Since beginning of month", PERIOD_MONTH_TO_DATE),
            ("All time", 0),
        ] {
            click_app_control(&mut app, &ctx, "Period")
                .textures_delta
                .clear();
            click_app_control(&mut app, &ctx, choice)
                .textures_delta
                .clear();
            assert_eq!(app.settings.filter.seconds, seconds);
            assert!(app.settings.filter.time_range.is_none());
        }
        click_app_control(&mut app, &ctx, "Period")
            .textures_delta
            .clear();
        click_app_control(&mut app, &ctx, "Custom range…")
            .textures_delta
            .clear();
        assert!(app.custom_period.is_some());
        click_app_control(&mut app, &ctx, "Cancel")
            .textures_delta
            .clear();
        assert!(app.custom_period.is_none());
        assert!(app.settings.filter.all_time());
    }
}
#[test]
fn many_adapters_leave_wifi_choices_accessible() {
    let ctx = context(Theme::Dark);
    let mut app = app_fixture();
    Arc::make_mut(&mut app.data.snapshot).adapters = (0..40)
        .map(|i| Adapter {
            id: format!("adapter-{i}"),
            name: format!("Adapter {i}"),
            up: true,
            ..Default::default()
        })
        .collect();
    Arc::make_mut(&mut app.data.snapshot).wifi_networks = vec![WifiNetwork {
        id: "home-id".into(),
        name: "Home Wi-Fi".into(),
    }];
    let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    warm.textures_delta.clear();
    let mut opened = open_control(&mut app, &ctx, "Adapters");
    let position = text_positions(&opened)
        .into_iter()
        .find(|(text, _)| text == "Home Wi-Fi")
        .map(|(_, position)| position);
    opened.textures_delta.clear();
    let position =
        position.expect("Wi-Fi network should remain accessible below a long adapter list");
    assert!(position.y < 820.0, "Wi-Fi choices extend beyond the window");
    click_app_control(&mut app, &ctx, "Home Wi-Fi")
        .textures_delta
        .clear();
    assert_eq!(app.settings.filter.wifi_network.as_deref(), Some("home-id"));
}
#[test]
fn overview_and_app_filter_bars_have_the_same_height() {
    for width in [960.0, 1440.0] {
        for scale in [0.8, 0.9, 1.1] {
            let mut heights = Vec::new();
            for apps in [false, true] {
                let ctx = context(Theme::Dark);
                ctx.set_zoom_factor(scale);
                let mut app = app_fixture();
                app.settings.filter.source = if apps {
                    Source::Transport
                } else {
                    Source::Adapter
                };
                let mut height = 0.0;
                let mut out = ctx.run_ui(screen(Vec2::new(width, 680.0) / scale, vec![]), |ui| {
                    let top = ui.cursor().top();
                    app.toolbar(ui, apps);
                    height = ui.cursor().top() - top;
                });
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
fn three_adapters(app: &mut MonitorApp) {
    Arc::make_mut(&mut app.data.snapshot).adapters =
        [("eth", "Ethernet"), ("wifi", "Wi-Fi"), ("vpn", "VPN")]
            .into_iter()
            .map(|(id, name)| Adapter {
                id: id.into(),
                name: name.into(),
                up: true,
                ..Default::default()
            })
            .collect();
}
#[test]
fn overall_dropdowns_change_period_and_support_multiple_adapter_choices() {
    let ctx = context(Theme::Dark);
    let mut app = app_fixture();
    app.settings.filter.source = Source::Adapter;
    three_adapters(&mut app);
    let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    warm.textures_delta.clear();
    click_app_control(&mut app, &ctx, "Period")
        .textures_delta
        .clear();
    click_app_control(&mut app, &ctx, "30m")
        .textures_delta
        .clear();
    assert_eq!(app.settings.filter.seconds, 1800);
    click_app_control(&mut app, &ctx, "Adapters")
        .textures_delta
        .clear();
    let mut out = click_app_control(&mut app, &ctx, "Ethernet");
    assert_eq!(
        app.settings.filter.adapters,
        HashSet::from(["wifi".into(), "vpn".into()])
    );
    assert!(text_positions(&out).iter().any(|(s, _)| s == "Wi-Fi"));
    out.textures_delta.clear();
    click_app_control(&mut app, &ctx, "Wi-Fi")
        .textures_delta
        .clear();
    assert_eq!(app.settings.filter.adapters, HashSet::from(["vpn".into()]));
}
#[test]
fn wifi_connection_filter_is_independent_of_adapters_and_can_be_cleared() {
    for source in [Source::Adapter, Source::Transport] {
        let ctx = context(Theme::Dark);
        let mut app = app_fixture();
        app.settings.filter.source = source;
        app.settings.filter.adapters = HashSet::from(["wifi".into()]);
        if source == Source::Transport {
            app.settings.filter.scope = Scope::Wan;
        }
        three_adapters(&mut app);
        Arc::make_mut(&mut app.data.snapshot).wifi_networks = vec![
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
        let mut out = open_control(&mut app, &ctx, "Adapters");
        let present = text_positions(&out)
            .iter()
            .any(|(text, _)| text == "Office Wi-Fi");
        out.textures_delta.clear();
        assert!(present, "Remembered network is missing");
        click_app_control(&mut app, &ctx, "Office Wi-Fi")
            .textures_delta
            .clear();
        assert_eq!(
            app.settings.filter.wifi_network.as_deref(),
            Some("office-id")
        );
        let (label, value, _) = app.adapter_filter_caption();
        assert_eq!((label.as_str(), value.as_str()), ("Wi-Fi", "Office Wi-Fi"));
        assert_eq!(app.settings.filter.adapters, HashSet::from(["wifi".into()]));
        click_app_control(&mut app, &ctx, "Home Wi-Fi")
            .textures_delta
            .clear();
        assert_eq!(app.settings.filter.wifi_network.as_deref(), Some("home-id"));
        click_app_control(&mut app, &ctx, "Unidentified Wi-Fi")
            .textures_delta
            .clear();
        assert_eq!(
            app.settings.filter.wifi_network.as_deref(),
            Some(WIFI_UNKNOWN)
        );
        click_app_control(&mut app, &ctx, "All connections")
            .textures_delta
            .clear();
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
    let ctx = context(Theme::Dark);
    let mut app = app_fixture();
    Arc::make_mut(&mut app.data.snapshot).adapters = [("eth", "Ethernet"), ("wifi", "Wi-Fi")]
        .into_iter()
        .map(|(id, name)| Adapter {
            id: id.into(),
            name: name.into(),
            up: true,
            ..Default::default()
        })
        .collect();
    let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    warm.textures_delta.clear();
    click_app_control(&mut app, &ctx, "Period")
        .textures_delta
        .clear();
    click_app_control(&mut app, &ctx, "30m")
        .textures_delta
        .clear();
    assert_eq!(app.settings.filter.seconds, 1800);
    click_app_control(&mut app, &ctx, "Adapters")
        .textures_delta
        .clear();
    let mut out = click_app_control(&mut app, &ctx, "Ethernet");
    assert!(!app.settings.filter.adapters.contains("eth"));
    assert!(app.settings.filter.adapters.contains("wifi"));
    assert!(app.settings.filter.adapters.contains("unassigned"));
    assert!(
        text_positions(&out).iter().any(|(s, _)| s == "Wi-Fi"),
        "Adapter popup closed after a checkbox click"
    );
    out.textures_delta.clear();
    click_app_control(&mut app, &ctx, "Wi-Fi")
        .textures_delta
        .clear();
    assert_eq!(
        app.settings.filter.adapters,
        HashSet::from(["unassigned".into()])
    );
    click_app_control(&mut app, &ctx, "Networks")
        .textures_delta
        .clear();
    click_app_control(&mut app, &ctx, "WAN")
        .textures_delta
        .clear();
    assert_eq!(app.settings.filter.scope, Scope::Wan);
}
#[test]
fn selection_can_be_cleared_from_the_apps_heading() {
    let ctx = context(Theme::Dark);
    let mut app = app_fixture();
    app.selected_app = Some(r"C:\Apps\Long application directory 0\plain0.exe".into());
    let (mut warm, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    warm.textures_delta.clear();
    click_app_control(&mut app, &ctx, "All apps")
        .textures_delta
        .clear();
    assert!(app.selected_app.is_none());
}
#[test]
fn settings_controls_apply_theme_units_and_interface_size() {
    let ctx = context(Theme::Dark);
    let size = Vec2::new(1200.0, 900.0);
    let mut app = app_fixture();
    app.page = Page::Settings;
    for _ in 0..2 {
        shell_frame(&mut app, &ctx, size, vec![])
            .textures_delta
            .clear();
    }
    let click = |app: &mut MonitorApp, label: &str| {
        let mut out = shell_frame(app, &ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, p)| s == label && p.x > widgets::NAV_WIDTH)
            .unwrap_or_else(|| panic!("Missing {label}"))
            .1
            + Vec2::new(4.0, 5.0);
        out.textures_delta.clear();
        shell_frame(app, &ctx, size, click_events(pos))
            .textures_delta
            .clear();
    };
    click(&mut app, "Light");
    assert_eq!(app.settings.theme, Theme::Light);
    assert!(
        !widgets::Palette::of(&ctx).dark,
        "Theme change must restyle egui"
    );
    click(&mut app, "Bits/s");
    assert!(app.settings.bits);
    click(&mut app, "Bytes/s");
    assert!(!app.settings.bits);
    app.settings.ui_scale = 1.1;
    click(&mut app, "Reset");
    assert_eq!(app.settings.ui_scale, 0.9);
    // egui applies a new zoom factor at the start of the next pass.
    shell_frame(&mut app, &ctx, size, vec![])
        .textures_delta
        .clear();
    assert!((ctx.zoom_factor() - 0.9).abs() < 0.001);
}
#[test]
fn chart_legend_toggles_hide_each_direction() {
    let ctx = context(Theme::Dark);
    let size = Vec2::new(1200.0, 820.0);
    let mut app = fixture();
    for _ in 0..2 {
        shell_frame(&mut app, &ctx, size, vec![])
            .textures_delta
            .clear();
    }
    for (label, flag) in [("Download", 0), ("Upload", 1)] {
        let mut out = shell_frame(&mut app, &ctx, size, vec![]);
        let pos = text_positions(&out)
            .into_iter()
            .find(|(s, p)| s == label && p.x > widgets::NAV_WIDTH)
            .unwrap()
            .1
            + Vec2::new(4.0, 4.0);
        out.textures_delta.clear();
        shell_frame(&mut app, &ctx, size, click_events(pos))
            .textures_delta
            .clear();
        if flag == 0 {
            assert!(!app.show_down && app.show_up);
        } else {
            assert!(!app.show_down && !app.show_up);
        }
    }
}
#[test]
fn every_outline_is_a_whole_number_of_physical_pixels() {
    fn widths(shape: &egui::Shape, into: &mut Vec<f32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| widths(s, into)),
            egui::Shape::Rect(rect) => into.push(rect.stroke.width),
            egui::Shape::LineSegment { stroke, .. } => into.push(stroke.width),
            egui::Shape::Circle(circle) => into.push(circle.stroke.width),
            egui::Shape::Path(path) => into.push(path.stroke.width),
            _ => {}
        }
    }
    // Common display scalings combined with the 80-110% interface size.
    let input = |size: Vec2, native: f32, ppp: f32| {
        let mut input = screen(size / ppp, vec![]);
        input.viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(native),
                ..Default::default()
            },
        );
        input
    };
    for (native, zoom) in [(1.0, 0.9), (1.25, 0.9), (1.5, 1.0), (1.0, 1.1), (2.0, 0.8)] {
        let ppp = native * zoom;
        for (page, source) in [
            (Page::Overview, Source::Adapter),
            (Page::Overview, Source::Transport),
            (Page::Connections, Source::Transport),
            (Page::Adapters, Source::Adapter),
            (Page::Settings, Source::Adapter),
        ] {
            let ctx = context(Theme::Dark);
            ctx.set_zoom_factor(zoom);
            let mut app = app_fixture();
            app.page = page;
            app.settings.filter.source = source;
            Arc::make_mut(&mut app.data.snapshot).adapters = (0..3)
                .map(|i| Adapter {
                    id: i.to_string(),
                    name: format!("Ethernet {i}"),
                    up: true,
                    ..Default::default()
                })
                .collect();
            let mut found = Vec::new();
            // The first frames apply the zoom and rebuild egui's own outlines for it.
            for _ in 0..3 {
                let mut out = ctx.run_ui(input(Vec2::new(1200.0, 820.0), native, ppp), |root| {
                    app.draw(root);
                });
                assert!((out.pixels_per_point - ppp).abs() < 1e-3);
                found.clear();
                out.shapes.iter().for_each(|s| widths(&s.shape, &mut found));
                out.textures_delta.clear();
            }
            assert!(found.len() > 5, "{} drew no outlines", page.label());
            for width in found.iter().copied().filter(|w| *w > 0.0) {
                let pixels = width * ppp;
                assert!(
                    pixels >= 0.99 && (pixels - pixels.round()).abs() < 0.02,
                    "{} has a {pixels:.3}px outline at {ppp}x",
                    page.label()
                );
            }
        }
        let ctx = context(Theme::Dark);
        ctx.set_zoom_factor(zoom);
        for _ in 0..3 {
            ctx.run_ui(input(Vec2::new(400.0, 300.0), native, ppp), |root| {
                widgets::Palette::sync_scale(root.ctx());
            })
            .textures_delta
            .clear();
        }
        let popup_outline = ctx.global_style().visuals.window_stroke.width * ppp;
        assert!(
            (popup_outline - 1.0).abs() < 0.02,
            "Popup outline is {popup_outline:.3}px at {ppp}x"
        );
    }
}
#[test]
fn every_theme_colours_the_title_bar_like_the_app() {
    for theme in Theme::ALL {
        let palette = theme.palette();
        let colors = crate::titlebar::Colors::from_palette(&palette);
        let colorref =
            |c: Color32| u32::from(c.r()) | u32::from(c.g()) << 8 | u32::from(c.b()) << 16;
        assert_eq!(colors.caption, colorref(palette.canvas), "{theme:?}");
        assert_eq!(colors.text, colorref(palette.text), "{theme:?}");
        assert_eq!(colors.dark, palette.dark, "{theme:?}");
        assert_ne!(colors.border, colors.caption, "{theme:?} border is visible");
    }
}
fn history_with_samples() -> History {
    // (timestamp, down, up, observed): the window is 1000..=1100.
    let samples = [
        (999, 9_000_000, 9_000_000, true), // before the period
        (1000, 1_000_000, 100_000, true),
        (1020, 4_000_000, 700_000, true),
        (1040, 2_000_000, 300_000, true),
        (1060, 50_000_000, 50_000_000, false), // a gap marker, not a measurement
        (1100, 3_000_000, 200_000, true),
        (1101, 8_000_000, 8_000_000, true), // after the period
    ];
    History {
        window_start: 1000,
        window_end: 1100,
        points: samples
            .into_iter()
            .map(|(timestamp, down, up, observed)| Point {
                timestamp,
                down,
                up,
                observed,
            })
            .collect(),
        ..Default::default()
    }
}
#[test]
fn peak_speeds_cover_the_selected_period_and_ignore_gaps() {
    let mut app = fixture();
    assert_eq!(
        app.peak_rates(),
        (None, None),
        "An empty period has no peak"
    );
    app.data.history = Arc::new(history_with_samples());
    assert_eq!(
        app.peak_rates(),
        (Some(4_000_000.0), Some(700_000.0)),
        "Only observed samples inside the window count"
    );
    // A frozen chart keeps reporting the samples it froze.
    app.frozen = Some(FrozenChart {
        points: vec![Point {
            timestamp: 1010,
            down: 7,
            up: 5,
            observed: true,
        }],
        window: (1000, 1100),
    });
    assert_eq!(app.peak_rates(), (Some(7.0), Some(5.0)));
}
#[test]
fn the_live_speeds_are_shown_with_the_peak_for_the_period() {
    let ctx = context(Theme::Dark);
    let mut app = fixture();
    app.data.history = Arc::new(history_with_samples());
    let size = Vec2::new(1200.0, 820.0);
    let (mut out, bottom, limit) = app_frame(&mut app, &ctx, size, vec![]);
    let labels = text_positions(&out);
    out.textures_delta.clear();
    let find = |text: &str| -> Vec<egui::Pos2> {
        labels
            .iter()
            .filter(|(label, _)| label == text)
            .map(|(_, pos)| *pos)
            .collect()
    };
    let peaks = find("Peak");
    assert_eq!(peaks.len(), 2, "One peak per direction");
    let (download, upload) = (find("Download")[0], find("Upload")[0]);
    assert!(
        peaks[0].x > download.x && peaks[0].x < upload.x && peaks[1].x > upload.x,
        "Each peak sits beside its own speed"
    );
    let down_peak = find(&app.rate(4_000_000.0));
    let up_peak = find(&app.rate(700_000.0));
    assert!(
        down_peak.len() == 1 && up_peak.len() == 1,
        "Peaks are formatted like the live speeds"
    );
    assert!(down_peak[0].x < up_peak[0].x);
    assert!(bottom <= limit + 1.0);
}
#[test]
fn the_destination_breakdown_fills_exactly_its_declared_height() {
    let ctx = context(Theme::Dark);
    let totals = ScopeTotals {
        lan: 5,
        wan: 7,
        unknown: 3,
    };
    let mut height = 0.0;
    let mut out = ctx.run_ui(screen(Vec2::new(400.0, 300.0), vec![]), |root| {
        root.spacing_mut().item_spacing.y = 0.0;
        let top = root.cursor().top();
        crate::graph::destination_breakdown(root, &totals);
        height = root.cursor().top() - top;
    });
    out.textures_delta.clear();
    assert_eq!(height, crate::graph::DESTINATION_HEIGHT);
}
#[test]
fn the_peak_is_quieter_than_the_live_speed() {
    fn styled(shape: &egui::Shape, into: &mut Vec<(String, Color32, f32)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| styled(s, into)),
            egui::Shape::Text(text) => {
                if let Some(section) = text.galley.job.sections.first() {
                    into.push((
                        text.galley.job.text.clone(),
                        section.format.color,
                        section.format.font_id.size,
                    ));
                }
            }
            _ => {}
        }
    }
    let ctx = context(Theme::Dark);
    let palette = Theme::Dark.palette();
    let mut app = fixture();
    app.data.history = Arc::new(history_with_samples());
    let (mut out, _, _) = app_frame(&mut app, &ctx, Vec2::new(1200.0, 820.0), vec![]);
    let mut texts = Vec::new();
    out.shapes.iter().for_each(|s| styled(&s.shape, &mut texts));
    out.textures_delta.clear();
    let one = |text: &str, min: f32, max: f32| {
        texts
            .iter()
            .find(|(t, _, size)| t == text && (min..=max).contains(size))
            .cloned()
            .unwrap_or_else(|| panic!("Missing {text} between {min} and {max}pt"))
    };
    // Empty live rows make both live speeds read zero; they are the large numbers.
    let (_, live_color, live_size) = one("0 B/s", 20.0, 40.0);
    let (_, peak_color, peak_size) = one(&app.rate(4_000_000.0), 8.0, 20.0);
    let (_, label_color, label_size) = one("Peak", 8.0, 20.0);
    assert_eq!(live_color, palette.text);
    assert_eq!(peak_color, palette.dim, "The peak uses the secondary grey");
    assert_eq!(label_color, palette.dim);
    assert!(
        peak_size <= 13.0 && label_size < peak_size && live_size >= 24.0,
        "The peak stays small next to the {live_size}pt live speed"
    );
}
#[derive(Default)]
struct MemoryStorage(std::collections::HashMap<String, String>);
impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.to_owned(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}
#[test]
fn a_minimized_window_never_overwrites_the_last_real_size_and_position() {
    use eframe::{App as _, Storage as _};
    let real = "(inner_position_pixels:Some((x:294.0,y:317.0)),outer_position_pixels:Some((x:286.0,y:286.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:1600.0,y:911.0)))";
    let collapsed = "(inner_position_pixels:Some((x:-32000.0,y:-32000.0)),outer_position_pixels:Some((x:-32000.0,y:-32000.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:0.0,y:0.0)))";
    let mut app = fixture();
    let mut storage = MemoryStorage::default();
    // eframe stores the window's geometry first, then asks the app to save.
    storage.set_string("window", real.to_owned());
    app.save(&mut storage);
    assert_eq!(app.window_record.as_deref(), Some(real));
    assert_eq!(storage.get_string("window").as_deref(), Some(real));
    storage.set_string("window", collapsed.to_owned());
    app.save(&mut storage);
    assert_eq!(
        storage.get_string("window").as_deref(),
        Some(real),
        "The collapsed record is replaced by the last real one"
    );
    // With nothing real remembered, the app opens at its default size instead.
    let mut fresh = fixture();
    storage.set_string("window", collapsed.to_owned());
    fresh.save(&mut storage);
    assert_eq!(
        storage.get_string("window").as_deref(),
        Some(crate::window_state::DEFAULT_RECORD)
    );
    assert!(
        storage.get_string("settings").is_some(),
        "Settings still save"
    );
}
