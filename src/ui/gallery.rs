//! Every component on one page, for visual review in both themes.
use super::{
    button::{Button, Size, Variant},
    chart::{self, AreaChart, BarValue},
    controls::{self, AlertVariant, BadgeVariant},
    icon::Icon,
    layout::{self, Col},
    text,
    tokens::tk,
};
use crate::model::Point;
use eframe::egui::{self, Align, Layout, Margin, Sense, Vec2};

pub struct GalleryState {
    pub tab: u8,
    pub on: bool,
    pub checked: bool,
    pub slider: f32,
    pub input: String,
    pub search: String,
}
impl Default for GalleryState {
    fn default() -> Self {
        Self {
            tab: 0,
            on: true,
            checked: true,
            slider: 0.9,
            input: "100".into(),
            search: String::new(),
        }
    }
}

fn heading(ui: &mut egui::Ui, label: &str) {
    let t = tk(ui.ctx());
    ui.add_space(14.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 20.0), Sense::hover());
    text::paint(
        ui.painter(),
        rect,
        egui::Align2::LEFT_CENTER,
        label,
        text::semibold(13.0),
        t.muted_foreground,
    );
    ui.add_space(4.0);
}

pub fn show(ui: &mut egui::Ui, state: &mut GalleryState) {
    let t = tk(ui.ctx());
    egui::Frame::new()
        .fill(t.background)
        .inner_margin(Margin::same(24))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
            heading(ui, "Buttons");
            ui.horizontal_wrapped(|ui| {
                for (label, variant) in [
                    ("Default", Variant::Default),
                    ("Secondary", Variant::Secondary),
                    ("Outline", Variant::Outline),
                    ("Ghost", Variant::Ghost),
                    ("Destructive", Variant::Destructive),
                    ("Link", Variant::Link),
                ] {
                    Button::new(label).variant(variant).show(ui);
                }
            });
            ui.horizontal(|ui| {
                Button::new("Export")
                    .icon(Icon::Download)
                    .variant(Variant::Outline)
                    .show(ui);
                Button::new("Small")
                    .size(Size::Sm)
                    .variant(Variant::Secondary)
                    .show(ui);
                Button::new("Large").size(Size::Lg).show(ui);
                Button::icon_only(Icon::More, "More")
                    .variant(Variant::Ghost)
                    .show(ui);
                Button::icon_only(Icon::Pause, "Pause")
                    .variant(Variant::Outline)
                    .show(ui);
                Button::new("Disabled").enabled(false).show(ui);
            });
            heading(ui, "Badges");
            ui.horizontal(|ui| {
                for (label, variant) in [
                    ("Default", BadgeVariant::Default),
                    ("Secondary", BadgeVariant::Secondary),
                    ("Outline", BadgeVariant::Outline),
                    ("Destructive", BadgeVariant::Destructive),
                    ("Connected", BadgeVariant::Success),
                ] {
                    controls::badge(ui, variant, label);
                }
                controls::kbd(ui, "Ctrl+1");
            });
            heading(ui, "Controls");
            ui.horizontal(|ui| {
                let mut tab = state.tab;
                layout::tabs(
                    ui,
                    "gallery-tabs",
                    &mut tab,
                    &[(0, "Overall traffic"), (1, "App traffic")],
                );
                state.tab = tab;
                layout::select(
                    ui,
                    "gallery-select",
                    "Period",
                    "1 hour",
                    None,
                    egui::PopupCloseBehavior::CloseOnClick,
                    200.0,
                    |ui| {
                        layout::menu_item(ui, true, "1 hour");
                        layout::menu_item(ui, false, "24 hours");
                    },
                );
                controls::switch(ui, &mut state.on, "Match Windows");
                controls::checkbox(ui, &mut state.checked, "Check");
                controls::slider(ui, &mut state.slider, 0.8..=1.1, 0.05, 140.0, "Size");
            });
            ui.horizontal(|ui| {
                controls::input(ui, &mut state.input, "Allowance", 120.0);
                controls::search(ui, &mut state.search, "Search apps", 260.0);
            });
            let mut ignored = false;
            let _ = &mut ignored;
            controls::progress(ui, 0.62, None);
            controls::alert(
                ui,
                AlertVariant::Default,
                Icon::Info,
                "Welcome",
                "Overall Traffic is everything each adapter carries.",
                true,
            );
            controls::alert(
                ui,
                AlertVariant::Destructive,
                Icon::Alert,
                "History needs attention",
                "Saved history failed its integrity check.",
                false,
            );
            heading(ui, "Card, table and charts");
            layout::card(ui, 20, |ui| {
                layout::card_header(
                    ui,
                    "Top apps",
                    "By total usage in the selected period",
                    |ui| {
                        Button::new("View all")
                            .variant(Variant::Outline)
                            .size(Size::Sm)
                            .show(ui);
                    },
                );
                ui.add_space(12.0);
                let cols = [Col::Flex(2.0), Col::Fixed(90.0), Col::Fixed(90.0)];
                let (header, _) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width(), layout::TABLE_HEADER_HEIGHT),
                    Sense::hover(),
                );
                let cells = layout::cells(header, &cols, 8.0, 0.0);
                for (cell, (label, align)) in cells.iter().zip([
                    ("App", egui::Align2::LEFT_CENTER),
                    ("Down", egui::Align2::RIGHT_CENTER),
                    ("Up", egui::Align2::RIGHT_CENTER),
                ]) {
                    layout::header_cell(
                        ui,
                        label,
                        *cell,
                        label,
                        align,
                        true,
                        (label == "Down").then_some(false),
                    );
                }
                layout::row_border(ui, header);
                for (name, down, up) in [
                    ("Helium", "30.6 GiB", "58.0 MiB"),
                    ("Steam", "27.5 GiB", "61.6 MiB"),
                ] {
                    let (row, response) = ui
                        .allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
                    layout::row_hover(ui, row, &response, false);
                    let cells = layout::cells(row, &cols, 8.0, 0.0);
                    controls::avatar(
                        ui.painter(),
                        &t,
                        egui::Rect::from_center_size(
                            egui::pos2(cells[0].left() + 16.0, row.center().y),
                            Vec2::splat(32.0),
                        ),
                        name,
                        None,
                    );
                    text::paint(
                        ui.painter(),
                        egui::Rect::from_min_max(
                            egui::pos2(cells[0].left() + 44.0, row.top()),
                            cells[0].right_bottom(),
                        ),
                        egui::Align2::LEFT_CENTER,
                        name,
                        text::medium(14.0),
                        t.foreground,
                    );
                    text::paint(
                        ui.painter(),
                        cells[1],
                        egui::Align2::RIGHT_CENTER,
                        down,
                        text::regular(14.0),
                        t.foreground,
                    );
                    text::paint(
                        ui.painter(),
                        cells[2],
                        egui::Align2::RIGHT_CENTER,
                        up,
                        text::regular(14.0),
                        t.muted_foreground,
                    );
                    layout::row_border(ui, row);
                }
            });
            ui.add_space(8.0);
            layout::card(ui, 20, |ui| {
                layout::card_header(ui, "Network speed", "Download and upload", |_| {});
                ui.add_space(12.0);
                let now = 1_800_000_000;
                let points: Vec<Point> = (0..120)
                    .map(|i| Point {
                        observed: i % 40 != 39,
                        timestamp: now - 3600 + i * 30,
                        down: ((i as f64 / 9.0).sin().abs() * 4_000_000.0) as u64 + 200_000,
                        up: ((i as f64 / 13.0).cos().abs() * 400_000.0) as u64 + 20_000,
                    })
                    .collect();
                AreaChart {
                    points: &points,
                    window: (now - 3600, now),
                    height: 180.0,
                    show_down: true,
                    show_up: true,
                    bits: false,
                    log_scale: false,
                }
                .show(ui);
                ui.add_space(12.0);
                let labels: Vec<String> = (1..=14).map(|d| format!("{d} Nov")).collect();
                let values: Vec<BarValue> = labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| BarValue {
                        label,
                        down: (i as u64 % 5 + 1) * 40_000_000,
                        up: (i as u64 % 3 + 1) * 3_000_000,
                    })
                    .collect();
                chart::bars(ui, &values, 130.0, "No traffic");
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |_| {});
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        scene,
        tokens::{BaseColor, Scheme, Tokens},
    };

    #[test]
    #[ignore = "Exports the component gallery's geometry for offline visual review"]
    fn export_gallery() {
        for (name, scheme, base) in [
            ("gallery-light", Scheme::Light, BaseColor::Zinc),
            ("gallery-dark", Scheme::Dark, BaseColor::Zinc),
            ("gallery-dark-violet", Scheme::Dark, BaseColor::Violet),
            ("gallery-light-green", Scheme::Light, BaseColor::Green),
        ] {
            let mut state = GalleryState::default();
            scene::export(
                name,
                [900.0, 1300.0],
                1.0,
                Tokens::new(scheme, base),
                &[],
                |ui| show(ui, &mut state),
            );
        }
    }
}
