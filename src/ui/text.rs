//! Typography: Inter in three weights, a monospace face, and text painting helpers.
use eframe::egui::{
    self, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Id, Painter, Rect, Vec2,
    text::{LayoutJob, TextFormat, TextWrapping},
};
use std::sync::Arc;

const REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
}

pub fn font(weight: Weight, size: f32) -> FontId {
    FontId::new(
        size,
        match weight {
            Weight::Regular => FontFamily::Proportional,
            Weight::Medium => FontFamily::Name("medium".into()),
            Weight::Semibold => FontFamily::Name("semibold".into()),
        },
    )
}
pub fn regular(size: f32) -> FontId {
    font(Weight::Regular, size)
}
pub fn medium(size: f32) -> FontId {
    font(Weight::Medium, size)
}
pub fn semibold(size: f32) -> FontId {
    font(Weight::Semibold, size)
}
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Registers Inter for all three weights, with egui's bundled fonts behind it for
/// symbols and scripts Inter lacks, and Cascadia Mono for code when installed.
pub fn install_fonts(ctx: &egui::Context) {
    let flag = Id::new("onm-ui-fonts");
    if ctx.data(|d| d.get_temp::<bool>(flag)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(flag, true));
    let mut fonts = FontDefinitions::default();
    for (key, bytes) in [
        ("Inter", REGULAR),
        ("Inter Medium", MEDIUM),
        ("Inter Semibold", SEMIBOLD),
    ] {
        fonts
            .font_data
            .insert(key.into(), Arc::new(FontData::from_static(bytes)));
    }
    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let stack = |first: &str| {
        let mut list = vec![first.to_owned()];
        if first != "Inter" {
            list.push("Inter".into());
        }
        list.extend(fallback.iter().cloned());
        list
    };
    fonts
        .families
        .insert(FontFamily::Proportional, stack("Inter"));
    fonts
        .families
        .insert(FontFamily::Name("medium".into()), stack("Inter Medium"));
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), stack("Inter Semibold"));
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\CascadiaMono.ttf") {
        fonts.font_data.insert(
            "Cascadia Mono".into(),
            Arc::new(FontData::from_owned(bytes)),
        );
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "Cascadia Mono".into());
    }
    ctx.set_fonts(fonts);
}

pub fn text_width(painter: &Painter, text: &str, font: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
}

/// Paints one line of text inside `rect`, cut with an ellipsis if too long. Nothing is
/// allocated, so painted text never takes clicks from the widget under it.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    align: Align2,
    text: &str,
    font: FontId,
    color: Color32,
) -> Rect {
    if text.is_empty() || rect.width() < 1.0 {
        return Rect::from_min_size(rect.min, Vec2::ZERO);
    }
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(rect.width());
    let galley = painter.layout_job(job);
    let placed = align.align_size_within_rect(galley.size(), rect);
    painter.galley(placed.min, galley, color);
    placed
}

/// A big number followed by a quieter unit, such as `63.51` and `MiB/s`.
pub fn number_with_unit(
    painter: &Painter,
    text: &str,
    number: FontId,
    unit: FontId,
    ink: Color32,
    quiet: Color32,
) -> Arc<egui::Galley> {
    let (head, tail) = text.split_once(' ').unwrap_or((text, ""));
    let mut job = LayoutJob::default();
    job.append(head, 0.0, TextFormat::simple(number, ink));
    if !tail.is_empty() {
        job.append(&format!(" {tail}"), 0.0, TextFormat::simple(unit, quiet));
    }
    painter.layout_job(job)
}
