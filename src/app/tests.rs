use super::*;
use crate::ui::scene::texts;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, Rect};

const PAGES: [&str; 5] = ["overview", "apps", "connections", "adapters", "settings"];

/// Window sizes in physical pixels with the interface size they are used at. Every one is at
/// least the minimum logical size (1120 x 780 points) once the interface size is applied.
const SIZES: [(Vec2, f32); 5] = [
    (Vec2::new(952.0, 663.0), 0.85),
    (Vec2::new(1120.0, 780.0), 1.0),
    (Vec2::new(1440.0, 820.0), 0.85),
    (Vec2::new(1232.0, 858.0), 1.1),
    (Vec2::new(1920.0, 1080.0), 1.0),
];

fn screen(size: Vec2, scale: f32, events: Vec<Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size / scale)),
        events,
        ..Default::default()
    }
}

fn frame(
    app: &mut MonitorApp,
    ctx: &egui::Context,
    size: Vec2,
    events: Vec<Event>,
) -> egui::FullOutput {
    let scale = app.settings.ui_scale;
    ctx.set_zoom_factor(scale);
    let mut out = ctx.run_ui(screen(size, scale, events), |ui| {
        app.draw(ui);
    });
    out.textures_delta.clear();
    out
}

fn press(pos: Pos2) -> Vec<Event> {
    let button = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    vec![Event::PointerMoved(pos), button(true), button(false)]
}

/// Clicks the first piece of text that reads `label`.
fn click(app: &mut MonitorApp, ctx: &egui::Context, size: Vec2, label: &str) {
    let out = frame(app, ctx, size, vec![]);
    let at = texts(&out)
        .into_iter()
        .find(|(text, _)| text == label)
        .unwrap_or_else(|| panic!("No text reads {label:?}"))
        .1;
    frame(app, ctx, size, press(at + egui::vec2(4.0, 5.0)));
    frame(app, ctx, size, vec![]);
}

fn settle(app: &mut MonitorApp, ctx: &egui::Context, size: Vec2) -> egui::FullOutput {
    let mut out = frame(app, ctx, size, vec![]);
    for _ in 0..2 {
        out = frame(app, ctx, size, vec![]);
    }
    out
}

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
    assert!(selected.contains(NO_ADAPTERS));
}

#[test]
fn every_page_draws_in_both_schemes_at_every_supported_size_and_scale() {
    for light in [false, true] {
        for page in PAGES {
            for (size, scale) in SIZES {
                let ctx = egui::Context::default();
                let mut app = MonitorApp::preview(&ctx, page, light);
                app.settings.ui_scale = scale;
                let out = settle(&mut app, &ctx, size);
                assert!(!out.shapes.is_empty(), "{page} {light} {size:?}");
                // The sidebar never collapses, so its live speed is always there.
                assert!(
                    texts(&out).iter().any(|(text, _)| text == "Live speed"),
                    "The sidebar is missing on {page} at {size:?}"
                );
            }
        }
    }
}

#[test]
fn every_outline_is_a_whole_number_of_physical_pixels() {
    fn collect(shape: &egui::Shape, into: &mut Vec<f32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, into)),
            egui::Shape::Rect(rect) if !rect.stroke.is_empty() => into.push(rect.stroke.width),
            _ => {}
        }
    }
    for page in PAGES {
        for scale in [0.8, 0.9, 1.0, 1.1] {
            let ctx = egui::Context::default();
            let mut app = MonitorApp::preview(&ctx, page, false);
            app.settings.ui_scale = scale;
            let out = settle(&mut app, &ctx, Vec2::new(1440.0, 900.0));
            let mut widths = Vec::new();
            out.shapes
                .iter()
                .for_each(|s| collect(&s.shape, &mut widths));
            assert!(!widths.is_empty(), "{page} draws no outlines");
            let ppp = out.pixels_per_point;
            for width in widths {
                let pixels = width * ppp;
                assert!(
                    (pixels - pixels.round()).abs() < 0.02 && pixels.round() >= 1.0,
                    "{page} at {scale}: an outline is {pixels} physical pixels wide"
                );
            }
        }
    }
}

#[test]
fn the_sidebar_never_collapses_and_the_minimum_size_is_what_pages_are_built_for() {
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    for (size, scale) in SIZES {
        app.settings.ui_scale = scale;
        let labels = texts(&settle(&mut app, &ctx, size));
        assert!(labels.iter().any(|(text, _)| text == "Ctrl+2"), "{size:?}");
    }
    // The smallest window at the default interface size is exactly the minimum logical size.
    assert!((MIN_WINDOW_SIZE[0] / DEFAULT_UI_SCALE - MIN_LOGICAL_SIZE[0]).abs() < 2.0);
    assert!((MIN_WINDOW_SIZE[1] / DEFAULT_UI_SCALE - MIN_LOGICAL_SIZE[1]).abs() < 2.0);
}

#[test]
fn clicking_navigation_tabs_and_headers_changes_what_is_shown() {
    let size = Vec2::new(1440.0, 820.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    settle(&mut app, &ctx, size);

    click(&mut app, &ctx, size, "Connections");
    assert!(app.page == Page::Connections);
    click(&mut app, &ctx, size, "Remote endpoint");
    assert_eq!(
        app.connection_sort.map(|s| (s.column, s.ascending)),
        Some((connection_tools::Column::Remote, true))
    );
    click(&mut app, &ctx, size, "Remote endpoint");
    assert_eq!(app.connection_sort.map(|s| s.ascending), Some(false));
    click(&mut app, &ctx, size, "Overview");
    assert!(app.page == Page::Overview);
    assert_eq!(app.settings.filter.source, Source::Adapter, "start");
    click(&mut app, &ctx, size, "App traffic");
    assert_eq!(app.settings.filter.source, Source::Transport);
    click(&mut app, &ctx, size, "Overall traffic");
    assert_eq!(app.settings.filter.source, Source::Adapter, "back");
}

#[test]
fn the_period_menu_opens_and_changes_the_period() {
    let size = Vec2::new(1440.0, 820.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    settle(&mut app, &ctx, size);
    click(&mut app, &ctx, size, "Period");
    click(&mut app, &ctx, size, "24h");
    assert_eq!(app.settings.filter.seconds, 86_400);
    assert_eq!(period_caption(&app.settings.filter), "24h");
}

#[test]
fn settings_controls_change_appearance_units_and_size() {
    let size = Vec2::new(1440.0, 900.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "settings", false);
    settle(&mut app, &ctx, size);
    click(&mut app, &ctx, size, "Light");
    assert_eq!(app.settings.appearance(), Appearance::Light);
    click(&mut app, &ctx, size, "Bits/s");
    assert!(app.settings.bits);
    click(&mut app, &ctx, size, "Bytes/s");
    assert!(!app.settings.bits);
    app.settings.ui_scale = 1.1;
    click(&mut app, &ctx, size, "Reset");
    assert_eq!(app.settings.ui_scale, DEFAULT_UI_SCALE);
}

#[test]
fn saved_settings_from_earlier_releases_keep_their_light_or_dark_choice() {
    let old = |theme: &str, follow: bool| -> Settings {
        serde_json::from_value(serde_json::json!({
            "theme": theme, "follow_system": follow, "close_to_tray": true, "bits": false, "ui_scale": 0.9
        }))
        .unwrap()
    };
    assert_eq!(old("Light", false).appearance(), Appearance::Light);
    assert_eq!(old("Dark", false).appearance(), Appearance::Dark);
    assert_eq!(old("Nord", false).appearance(), Appearance::Dark);
    assert_eq!(old("Mocha", false).appearance(), Appearance::Dark);
    assert_eq!(old("Dark", true).appearance(), Appearance::System);
    let mut current = old("Light", false);
    current.appearance = Some(Appearance::Dark);
    assert_eq!(
        current.appearance(),
        Appearance::Dark,
        "The new setting wins"
    );
    assert_eq!(Settings::default().base_color, BaseColor::Zinc);
}

#[test]
fn the_accent_colour_changes_the_installed_tokens() {
    let ctx = egui::Context::default();
    let mut settings = Settings {
        appearance: Some(Appearance::Light),
        ..Default::default()
    };
    let zinc = settings.tokens(&ctx);
    settings.base_color = BaseColor::Violet;
    let violet = settings.tokens(&ctx);
    assert_ne!(zinc.primary, violet.primary);
    assert_eq!(zinc.background, violet.background, "Neutrals stay the same");
    assert!(!violet.is_dark());
}

#[test]
fn control_number_keys_switch_pages_and_escape_clears_the_selection() {
    let size = Vec2::new(900.0, 600.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    let send = |app: &mut MonitorApp, key, modifiers| {
        let key_event = vec![Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        }];
        ctx.run_ui(screen(size, 1.0, key_event), |root| {
            app.shortcuts(root.ctx())
        })
        .textures_delta
        .clear();
    };
    app.selected_app = Some("C:/Apps/x.exe".into());
    send(&mut app, Key::Num2, Modifiers::COMMAND);
    assert!(app.page == Page::Connections);
    assert!(
        app.selected_app.is_some(),
        "The selection is kept for when the user comes back"
    );
    send(&mut app, Key::Num4, Modifiers::COMMAND);
    assert!(app.page == Page::Settings);
    send(&mut app, Key::Num1, Modifiers::COMMAND);
    assert!(app.page == Page::Overview);
    send(&mut app, Key::Num3, Modifiers::NONE);
    assert!(app.page == Page::Overview, "A bare digit is not a shortcut");
    send(&mut app, Key::Escape, Modifiers::NONE);
    assert!(app.selected_app.is_none());
}

#[test]
fn the_tray_tooltip_shows_live_speeds_and_when_recording_is_paused() {
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    let snapshot = Arc::make_mut(&mut app.data.snapshot);
    snapshot.interval = 1.0;
    snapshot.live_rows = vec![UsageRow {
        source: "adapter".into(),
        adapter_id: "eth".into(),
        down: 2_097_152,
        up: 1_024,
        ..Default::default()
    }];
    snapshot.recording_paused = false;
    app.settings.filter.source = Source::Adapter;
    let text = app.tray_tooltip();
    assert!(text.starts_with("OpenNetworkMonitor"));
    assert!(text.contains("↓ 2.00 MiB/s"), "{text}");
    assert!(text.contains("↑ 1.00 KiB/s"), "{text}");
    Arc::make_mut(&mut app.data.snapshot).recording_paused = true;
    assert!(app.tray_tooltip().ends_with("Recording paused"));
}

#[test]
fn painted_figures_are_readable_by_screen_readers() {
    let size = Vec2::new(1440.0, 900.0);
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let labels_of = |app: &mut MonitorApp| -> Vec<String> {
        let mut labels = Vec::new();
        for _ in 0..2 {
            if let Some(update) = frame(app, &ctx, size, vec![])
                .platform_output
                .accesskit_update
            {
                labels.extend(
                    update
                        .nodes
                        .iter()
                        .flat_map(|(_, node)| [node.label(), node.value()])
                        .flatten()
                        .map(str::to_owned),
                );
            }
        }
        labels
    };
    let has = |labels: &[String], part: &str| labels.iter().any(|label| label.contains(part));
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    let overview = labels_of(&mut app);
    for part in [
        "Download:",
        "Upload:",
        "Total usage:",
        "Chart of download and upload",
        "Network speed",
    ] {
        assert!(has(&overview, part), "{part}: {overview:?}");
    }
    app.page = Page::Connections;
    let connections = labels_of(&mut app);
    assert!(has(&connections, "PID 4200"), "{connections:?}");
    assert!(has(&connections, "203.0.113.1:443"));
    app.page = Page::Adapters;
    let adapters = labels_of(&mut app);
    assert!(has(&adapters, "Download:"), "{adapters:?}");
}

#[test]
fn the_welcome_card_shows_only_until_dismissed() {
    let size = Vec2::new(1440.0, 820.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "overview", false);
    app.settings.intro_seen = Some(false);
    let shown = texts(&settle(&mut app, &ctx, size));
    assert!(
        shown
            .iter()
            .any(|(text, _)| text == "Welcome to OpenNetworkMonitor")
    );
    click(&mut app, &ctx, size, "Got it");
    assert_eq!(app.settings.intro_seen, Some(true));
    let gone = texts(&settle(&mut app, &ctx, size));
    assert!(
        !gone
            .iter()
            .any(|(text, _)| text == "Welcome to OpenNetworkMonitor")
    );
}

#[test]
fn the_end_process_dialog_asks_before_anything_happens() {
    let size = Vec2::new(1200.0, 800.0);
    let ctx = egui::Context::default();
    let mut app = MonitorApp::preview(&ctx, "connections", false);
    app.pending_end = Some((u32::MAX - 5, "Example".into()));
    let shown = texts(&settle(&mut app, &ctx, size));
    assert!(shown.iter().any(|(text, _)| text == "End this process?"));
    click(&mut app, &ctx, size, "Cancel");
    assert!(
        app.pending_end.is_none(),
        "Cancel closes the dialog without ending anything"
    );
}

/// The first text that reads `label`, with its position.
fn find_text(out: &egui::FullOutput, label: &str) -> Pos2 {
    texts(out)
        .into_iter()
        // The sidebar's live speed repeats the first two figures, so look in the page.
        .find(|(text, pos)| text.starts_with(label) && pos.x > 250.0)
        .unwrap_or_else(|| panic!("No text starts with {label:?}"))
        .1
}

#[test]
fn four_stat_tiles_share_a_row_and_the_chart_follows_at_every_usable_size() {
    for (size, scale) in SIZES {
        let ctx = egui::Context::default();
        let mut app = MonitorApp::preview(&ctx, "overview", false);
        app.settings.ui_scale = scale;
        let out = settle(&mut app, &ctx, size);
        let tiles = ["63.51 MiB/s", "6.53 MiB/s", "124.37 GiB", "85 open"]
            .map(|label| find_text(&out, label));
        assert!(
            tiles.iter().all(|p| (p.y - tiles[0].y).abs() < 1.0),
            "{size:?} at {scale}: the tiles are not in one row: {tiles:?}"
        );
        assert!(tiles.windows(2).all(|w| w[1].x > w[0].x));
        let chart = find_text(&out, "Network speed");
        assert!(
            chart.y > tiles[0].y + 60.0,
            "The chart sits below the tiles"
        );
        // The chart is on screen without scrolling at the default window size.
        if size.x == 1440.0 {
            assert!(
                chart.y < size.y / scale,
                "{size:?}: the chart starts off screen"
            );
        }
    }
}

#[test]
fn app_traffic_is_always_three_tables_side_by_side() {
    for (size, scale) in SIZES {
        let ctx = egui::Context::default();
        let mut app = MonitorApp::preview(&ctx, "apps", false);
        app.settings.ui_scale = scale;
        let out = settle(&mut app, &ctx, size);
        let tables = ["Apps  ·", "Hosts  ·", "Traffic type  ·"].map(|label| find_text(&out, label));
        assert!(
            tables.iter().all(|p| (p.y - tables[0].y).abs() < 1.0),
            "{size:?} at {scale}: the tables are stacked: {tables:?}"
        );
        assert!(tables.windows(2).all(|w| w[1].x > w[0].x));
    }
}

#[test]
fn pages_fill_the_window_without_scrolling_and_keep_one_arrangement() {
    for page in ["overview", "apps", "connections"] {
        let mut arrangements = Vec::new();
        for (size, scale) in SIZES {
            let ctx = egui::Context::default();
            let mut app = MonitorApp::preview(&ctx, page, false);
            app.settings.ui_scale = scale;
            let out = settle(&mut app, &ctx, size);
            let (width, height) = (size.x / scale, size.y / scale);
            // Cards and panels end inside the window too, so no page needs scrolling.
            for clipped in &out.shapes {
                if let egui::Shape::Rect(rect) = &clipped.shape
                    && rect.rect.height() > 60.0
                    && rect.rect.width() > 200.0
                {
                    // Rows inside a scrolling list are clipped to it and may run on; a card on
                    // the page itself is clipped only by the window.
                    if clipped.clip_rect.max.y >= height - 1.0 {
                        assert!(
                            rect.rect.max.y <= height + 0.5,
                            "{page} at {size:?}: a card ends at {}, below the {height} point window",
                            rect.rect.max.y
                        );
                    }
                }
            }
            for (text, at) in visible_texts(&out) {
                assert!(
                    at.x < width && at.y < height,
                    "{page} at {size:?}: {text:?} is at {at:?}, outside the {width}x{height} window"
                );
            }
            // What sits in a row with what, from the left, is the same at every size.
            let mut rows: Vec<(i32, Vec<String>)> = Vec::new();
            for (text, at) in visible_texts(&out) {
                if ![
                    "Download",
                    "Upload",
                    "Total usage",
                    "Network speed",
                    "Destinations",
                    "Apps  ·",
                    "Hosts  ·",
                    "Traffic type  ·",
                    "All apps",
                ]
                .iter()
                .any(|key| text.starts_with(key))
                    || at.x < 240.0
                {
                    continue;
                }
                let key = (at.y / 4.0).round() as i32;
                match rows.iter_mut().find(|(row, _)| (*row - key).abs() <= 1) {
                    Some((_, names)) => names.push(text),
                    None => rows.push((key, vec![text])),
                }
            }
            rows.sort_by_key(|(row, _)| *row);
            arrangements.push(
                rows.into_iter()
                    .map(|(_, names)| names.len())
                    .collect::<Vec<_>>(),
            );
        }
        assert!(
            arrangements.windows(2).all(|pair| pair[0] == pair[1]),
            "{page} is arranged differently at different sizes: {arrangements:?}"
        );
    }
}

/// Texts that are actually on screen: not scrolled out of their list's clip rectangle.
fn visible_texts(out: &egui::FullOutput) -> Vec<(String, Pos2)> {
    fn collect(shape: &egui::Shape, clip: Rect, into: &mut Vec<(String, Pos2)>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, clip, into)),
            egui::Shape::Text(t) if clip.contains(t.pos) => {
                into.push((t.galley.job.text.clone(), t.pos));
            }
            _ => {}
        }
    }
    let mut all = Vec::new();
    for clipped in &out.shapes {
        collect(&clipped.shape, clipped.clip_rect, &mut all);
    }
    all
}
