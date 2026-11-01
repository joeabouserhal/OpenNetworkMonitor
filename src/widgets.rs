//! Design system: palette tokens, typography and the shared controls.
//!
//! The interface is monochrome chrome with two data colours (download and
//! upload). Everything is drawn from `Palette` tokens so every theme stays in
//! step, and controls are painted directly rather than styled from egui defaults.
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Id, Layout, Margin, Painter,
    PopupCloseBehavior, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType,
    text::{LayoutJob, TextWrapping},
};
use std::ops::RangeInclusive;

pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [1440.0, 820.0];
pub const MIN_WINDOW_SIZE: [f32; 2] = [960.0, 680.0];
pub const NAV_WIDTH: f32 = 208.0;
pub const NAV_RAIL_WIDTH: f32 = 68.0;
/// Below this logical window width the sidebar collapses to an icon rail.
pub const NAV_RAIL_BELOW: f32 = 1100.0;
pub const PAGE_MARGIN: i8 = 28;
pub const ROW_HEIGHT: f32 = 54.0;
pub const CONTROL_HEIGHT: f32 = 34.0;
const CARD_RADIUS: u8 = 14;

/// Colour tokens shared by every control. `hairline` is translucent so one value
/// works on the canvas, on cards and inside popups.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub canvas: Color32,
    pub card: Color32,
    pub raised: Color32,
    pub sunken: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub accent: Color32,
    pub accent_ink: Color32,
    pub accent_soft: Color32,
    pub down: Color32,
    pub up: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
}

const PALETTE_ID: &str = "onm-palette";
const PIXELS_ID: &str = "onm-pixels";

impl Palette {
    /// Stores the tokens and restyles egui's own widgets to match.
    pub fn install(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Id::new(PALETTE_ID), self));
        // Visuals are stored per light/dark slot. Pin the slot being written so a
        // change of the Windows theme cannot swap in egui's own default colours.
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        install_fonts(ctx);
        self.restyle(ctx);
    }
    /// egui's own outlines (popups, tooltips, focus) are sized in points, so they
    /// are rebuilt whenever the pixel density changes, e.g. when the interface
    /// size is adjusted or the window moves to another display.
    pub fn sync_scale(ctx: &egui::Context) {
        let current = ctx.pixels_per_point();
        let known = ctx.data(|d| d.get_temp::<f32>(Id::new(PIXELS_ID)));
        if known.is_some_and(|known| (known - current).abs() > 1e-4) {
            Self::of(ctx).restyle(ctx);
            ctx.request_repaint();
        }
    }
    fn restyle(self, ctx: &egui::Context) {
        let ppp = ctx.pixels_per_point();
        ctx.data_mut(|d| d.insert_temp(Id::new(PIXELS_ID), ppp));
        let px = 1.0 / ppp;
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = None;
        v.weak_text_color = Some(self.dim);
        v.panel_fill = self.canvas;
        v.window_fill = self.card;
        v.faint_bg_color = self.raised;
        v.extreme_bg_color = self.sunken;
        v.code_bg_color = self.sunken;
        v.text_edit_bg_color = Some(self.sunken);
        v.window_stroke = Stroke::new(px, self.hairline);
        v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
        v.menu_corner_radius = CornerRadius::same(12);
        let shadow = egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(if self.dark { 120 } else { 36 }),
        };
        v.window_shadow = shadow;
        v.popup_shadow = shadow;
        v.selection.bg_fill = self.card.lerp_to_gamma(self.accent, 0.38);
        v.selection.stroke = Stroke::new(px, self.accent);
        v.hyperlink_color = self.accent;
        v.error_fg_color = self.bad;
        v.warn_fg_color = self.warn;
        v.text_cursor.stroke = Stroke::new(2.0 * px, self.accent);
        v.striped = false;
        v.slider_trailing_fill = true;
        v.indent_has_left_vline = false;
        v.collapsing_header_frame = false;
        let hover = self.raised.lerp_to_gamma(self.text, 0.07);
        let ink = Stroke::new(px, self.text);
        let widgets = &mut v.widgets;
        widgets.noninteractive.bg_fill = self.card;
        widgets.noninteractive.weak_bg_fill = self.card;
        widgets.noninteractive.bg_stroke = Stroke::new(px, self.hairline);
        widgets.noninteractive.fg_stroke = ink;
        for (state, fill) in [
            (&mut widgets.inactive, self.raised),
            (&mut widgets.hovered, hover),
            (&mut widgets.active, hover),
            (&mut widgets.open, hover),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.fg_stroke = ink;
            state.corner_radius = CornerRadius::same(10);
            state.expansion = 0.0;
        }
        widgets.inactive.bg_stroke = Stroke::NONE;
        widgets.hovered.bg_stroke = Stroke::new(px, self.hairline);
        widgets.active.bg_stroke = Stroke::new(px, self.accent);
        widgets.open.bg_stroke = Stroke::new(px, self.accent);
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 8.0);
            s.spacing.button_padding = Vec2::new(14.0, 7.0);
            s.spacing.interact_size = Vec2::new(40.0, 32.0);
            s.spacing.menu_margin = Margin::same(8);
            s.spacing.scroll = egui::style::ScrollStyle::floating();
            for (style, font) in [
                (egui::TextStyle::Body, sans(15.0)),
                (egui::TextStyle::Button, sans(15.0)),
                (egui::TextStyle::Small, sans(12.5)),
                (egui::TextStyle::Heading, bold(22.0)),
                (egui::TextStyle::Monospace, mono(13.5)),
            ] {
                s.text_styles.insert(style, font);
            }
        });
    }
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(Id::new(PALETTE_ID)))
            .unwrap_or(Self::FALLBACK)
    }
    const FALLBACK: Self = Self {
        dark: true,
        canvas: Color32::from_rgb(13, 14, 16),
        card: Color32::from_rgb(21, 23, 25),
        raised: Color32::from_rgb(32, 35, 39),
        sunken: Color32::from_rgb(15, 16, 18),
        hairline: Color32::from_rgba_premultiplied(14, 14, 14, 14),
        text: Color32::from_rgb(236, 238, 240),
        dim: Color32::from_rgb(154, 160, 168),
        accent: Color32::from_rgb(96, 165, 250),
        accent_ink: Color32::from_rgb(8, 14, 24),
        accent_soft: Color32::from_rgb(28, 43, 66),
        down: Color32::from_rgb(96, 165, 250),
        up: Color32::from_rgb(255, 176, 84),
        good: Color32::from_rgb(74, 222, 128),
        warn: Color32::from_rgb(245, 176, 65),
        bad: Color32::from_rgb(248, 113, 113),
    };
}

pub fn pal(ui: &Ui) -> Palette {
    Palette::of(ui.ctx())
}

/// A stroke exactly `pixels` physical pixels wide. Whole pixels keep rules and
/// outlines crisp: egui snaps rectangle edges to the pixel grid but not stroke
/// widths, so a width of 1 point is soft and uneven at most interface scales.
pub fn px_stroke(ctx: &egui::Context, pixels: f32, color: Color32) -> Stroke {
    Stroke::new(pixels / ctx.pixels_per_point(), color)
}

/// `points` rounded to a whole number of physical pixels, at least one.
pub fn snapped(ctx: &egui::Context, points: f32, color: Color32) -> Stroke {
    let ppp = ctx.pixels_per_point();
    Stroke::new((points * ppp).round().max(1.0) / ppp, color)
}

/// The standard one-pixel outline.
pub fn hairline(ui: &Ui) -> Stroke {
    px_stroke(ui.ctx(), 1.0, pal(ui).hairline)
}

/// The two-pixel ring drawn around keyboard-focused controls.
pub fn focus_ring(ui: &Ui) -> Stroke {
    px_stroke(ui.ctx(), 2.0, pal(ui).accent)
}

// ---------------------------------------------------------------- typography

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}
pub fn light() -> FontFamily {
    FontFamily::Name("light".into())
}
pub fn sans(size: f32) -> FontId {
    FontId::proportional(size)
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, semibold())
}
pub fn thin(size: f32) -> FontId {
    FontId::new(size, light())
}
pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}
pub fn strong_text(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(bold(size))
}

/// Loads Segoe UI weights and Cascadia Mono when present; every family falls
/// back to egui's bundled fonts so `semibold()` and `light()` always resolve.
pub fn install_fonts(ctx: &egui::Context) {
    let flag = Id::new("onm-fonts");
    if ctx.data(|d| d.get_temp::<bool>(flag)).unwrap_or(false) {
        return;
    }
    ctx.data_mut(|d| d.insert_temp(flag, true));
    fn load(fonts: &mut egui::FontDefinitions, key: &str, file: &str) -> bool {
        match std::fs::read(format!(r"C:\Windows\Fonts\{file}")) {
            Ok(bytes) => {
                fonts
                    .font_data
                    .insert(key.into(), egui::FontData::from_owned(bytes).into());
                true
            }
            Err(_) => false,
        }
    }
    let mut fonts = egui::FontDefinitions::default();
    let regular = load(&mut fonts, "Segoe UI", "segoeui.ttf");
    let semi = load(&mut fonts, "Segoe UI Semibold", "seguisb.ttf");
    let slim = load(&mut fonts, "Segoe UI Light", "segoeuil.ttf");
    let code = load(&mut fonts, "Cascadia Mono", "CascadiaMono.ttf");
    let mut proportional = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    if regular {
        proportional.insert(0, "Segoe UI".into());
    }
    let with = |first: bool, name: &str| {
        let mut stack = proportional.clone();
        if first {
            stack.insert(0, name.into());
        }
        stack
    };
    let weighted = [
        (semibold(), with(semi, "Segoe UI Semibold")),
        (light(), with(slim, "Segoe UI Light")),
    ];
    fonts
        .families
        .insert(FontFamily::Proportional, proportional.clone());
    fonts.families.extend(weighted);
    if code {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "Cascadia Mono".into());
    }
    ctx.set_fonts(fonts);
}

pub fn radius(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

pub fn text_width(painter: &Painter, text: &str, font: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
}

/// Paints one line of text anchored inside `rect`, truncating with an ellipsis.
/// Nothing is allocated, so painted text never steals clicks from its row.
pub fn paint_text(
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

/// Lays out `63.51 MiB/s` as a large number followed by a quieter unit.
pub fn value_galley(
    painter: &Painter,
    text: &str,
    number: FontId,
    unit: FontId,
    ink: Color32,
    dim: Color32,
) -> std::sync::Arc<egui::Galley> {
    let (head, tail) = text.split_once(' ').unwrap_or((text, ""));
    let mut job = LayoutJob::default();
    job.append(head, 0.0, egui::text::TextFormat::simple(number, ink));
    if !tail.is_empty() {
        job.append(
            &format!(" {tail}"),
            0.0,
            egui::text::TextFormat::simple(unit, dim),
        );
    }
    painter.layout_job(job)
}

pub fn paint_value(
    painter: &Painter,
    rect: Rect,
    align: Align2,
    text: &str,
    fonts: (FontId, FontId),
    ink: Color32,
    dim: Color32,
) -> Rect {
    let galley = value_galley(painter, text, fonts.0, fonts.1, ink, dim);
    let placed = align.align_size_within_rect(galley.size(), rect);
    painter.galley(placed.min, galley, ink);
    placed
}

// --------------------------------------------------------------------- icons

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Overview,
    Connections,
    Adapters,
    Settings,
    Chevron,
    Search,
    Check,
    Close,
    Export,
    More,
    Down,
    Up,
}

/// Line icons on a 24-unit grid, drawn with the painter so they scale with the
/// interface and take any colour.
pub fn paint_icon(painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let k = size / 24.0;
    let stroke = snapped(painter.ctx(), (1.7 * k).max(1.2), color);
    let pt = |x: f32, y: f32| center + Vec2::new(x, y) * k;
    match icon {
        Icon::Overview => {
            for (x, h) in [(-6.5, 8.0), (0.0, 15.0), (6.5, 11.0)] {
                painter.rect_filled(
                    Rect::from_min_max(pt(x - 2.1, 8.0 - h), pt(x + 2.1, 8.0)),
                    radius(2.0 * k),
                    color,
                );
            }
        }
        Icon::Connections => {
            for (x, y) in [(-6.5, -5.5), (6.5, 5.5)] {
                painter.circle_stroke(pt(x, y), 3.4 * k, stroke);
            }
            painter.line_segment([pt(-4.2, -3.2), pt(4.2, 3.2)], stroke);
        }
        Icon::Adapters => {
            painter.rect_stroke(
                Rect::from_min_max(pt(-9.0, -8.5), pt(9.0, 3.5)),
                radius(2.5 * k),
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment([pt(0.0, 3.5), pt(0.0, 8.0)], stroke);
            painter.line_segment([pt(-4.5, 8.0), pt(4.5, 8.0)], stroke);
        }
        Icon::Settings => {
            for (y, x) in [(-6.0, -3.5), (0.0, 4.0), (6.0, -1.0)] {
                painter.line_segment([pt(-9.0, y), pt(9.0, y)], stroke);
                painter.circle_filled(pt(x, y), 2.8 * k, color);
            }
        }
        Icon::Chevron => {
            painter.add(Shape::line(
                vec![pt(-4.5, -2.0), pt(0.0, 2.5), pt(4.5, -2.0)],
                stroke,
            ));
        }
        Icon::Search => {
            painter.circle_stroke(pt(-1.5, -1.5), 5.8 * k, stroke);
            painter.line_segment([pt(2.8, 2.8), pt(8.0, 8.0)], stroke);
        }
        Icon::Check => {
            painter.add(Shape::line(
                vec![pt(-5.5, 0.5), pt(-1.8, 4.2), pt(5.8, -4.0)],
                stroke,
            ));
        }
        Icon::Close => {
            painter.line_segment([pt(-5.0, -5.0), pt(5.0, 5.0)], stroke);
            painter.line_segment([pt(-5.0, 5.0), pt(5.0, -5.0)], stroke);
        }
        Icon::Export => {
            painter.line_segment([pt(0.0, -8.0), pt(0.0, 3.5)], stroke);
            painter.add(Shape::line(
                vec![pt(-4.0, 0.0), pt(0.0, 4.0), pt(4.0, 0.0)],
                stroke,
            ));
            painter.add(Shape::line(
                vec![pt(-8.0, 3.0), pt(-8.0, 8.0), pt(8.0, 8.0), pt(8.0, 3.0)],
                stroke,
            ));
        }
        Icon::More => {
            for x in [-6.0, 0.0, 6.0] {
                painter.circle_filled(pt(x, 0.0), 1.9 * k, color);
            }
        }
        Icon::Down | Icon::Up => {
            let s = if icon == Icon::Down { 1.0 } else { -1.0 };
            painter.line_segment([pt(0.0, -7.0 * s), pt(0.0, 7.0 * s)], stroke);
            painter.add(Shape::line(
                vec![pt(-4.5, 2.5 * s), pt(0.0, 7.0 * s), pt(4.5, 2.5 * s)],
                stroke,
            ));
        }
    }
}

// ------------------------------------------------------------------ surfaces

pub fn surface(ui: &Ui) -> egui::Frame {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(hairline(ui))
        .corner_radius(CARD_RADIUS)
        .inner_margin(18)
}

/// A card that stretches to the bottom of the space it was given.
pub fn surface_fill<R>(
    ui: &mut Ui,
    frame: egui::Frame,
    contents: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    let outer = ui.available_size();
    let margin = frame.total_margin().sum();
    frame.show(ui, |ui| {
        ui.set_min_size((outer - margin).max(Vec2::ZERO));
        contents(ui)
    })
}

pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, hairline(ui));
}

pub fn empty_state(ui: &mut Ui, text: &str) {
    ui.add_space(12.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).size(13.5).weak());
    });
}

/// Small rounded label. Pass `pal(ui).dim` for a neutral count.
pub fn badge(ui: &mut Ui, label: &str, color: Color32) {
    let p = pal(ui);
    let font = bold(12.5);
    let width = text_width(ui.painter(), label, font.clone()) + 20.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        radius(11.0),
        p.card
            .lerp_to_gamma(color, if p.dark { 0.16 } else { 0.11 }),
    );
    paint_text(painter, rect, Align2::CENTER_CENTER, label, font, color);
}

pub fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Compact status message. Returns true when Dismiss was selected.
pub fn message(ui: &mut Ui, text: &str, warning: bool, dismissible: bool) -> bool {
    let p = pal(ui);
    let tone = if warning { p.warn } else { p.accent };
    let mut dismissed = false;
    egui::Frame::new()
        .fill(p.card.lerp_to_gamma(tone, if p.dark { 0.13 } else { 0.10 }))
        .stroke(px_stroke(ui.ctx(), 1.0, tone.gamma_multiply(0.35)))
        .corner_radius(12)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let (dot, _) = ui.allocate_exact_size(Vec2::new(8.0, 20.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.0, tone);
                let width = (ui.available_width() - if dismissible { 84.0 } else { 0.0 }).max(50.0);
                ui.add_sized(
                    [width, 0.0],
                    egui::Label::new(RichText::new(text).size(14.0)).wrap(),
                );
                if dismissible {
                    dismissed = button(ui, ButtonKind::Ghost, "Dismiss").clicked();
                }
            });
        });
    ui.add_space(8.0);
    dismissed
}

// ------------------------------------------------------------------- buttons

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

pub fn button(ui: &mut Ui, kind: ButtonKind, label: &str) -> Response {
    button_with(ui, kind, None, label, None)
}

pub fn button_with(
    ui: &mut Ui,
    kind: ButtonKind,
    icon: Option<Icon>,
    label: &str,
    width: Option<f32>,
) -> Response {
    let p = pal(ui);
    let font = bold(14.0);
    let label_width = text_width(ui.painter(), label, font.clone());
    let icon_space = if icon.is_some() { 24.0 } else { 0.0 };
    let natural = label_width + icon_space + 28.0;
    let size = Vec2::new(width.map_or(natural, |w| w.max(natural)), 32.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), label));
    let hover = ui.ctx().animate_bool_with_time(
        response.id,
        response.hovered() || response.has_focus(),
        0.1,
    );
    let (fill, ink, edge) = match kind {
        ButtonKind::Primary => (
            p.accent.lerp_to_gamma(p.text, 0.14 * hover),
            p.accent_ink,
            Stroke::NONE,
        ),
        ButtonKind::Secondary => (
            p.raised.lerp_to_gamma(p.text, 0.07 * hover),
            p.text,
            hairline(ui),
        ),
        ButtonKind::Ghost => (p.raised.gamma_multiply(hover), p.text, Stroke::NONE),
    };
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let painter = ui.painter();
    painter.rect_filled(rect, radius(10.0), fill.gamma_multiply(fade));
    if edge != Stroke::NONE {
        painter.rect_stroke(rect, radius(10.0), edge, StrokeKind::Inside);
    }
    if response.has_focus() {
        painter.rect_stroke(rect, radius(10.0), focus_ring(ui), StrokeKind::Outside);
    }
    let inner = rect.shrink2(Vec2::new(14.0, 0.0));
    let content = label_width + icon_space;
    let mut x = inner.center().x - content / 2.0;
    if let Some(icon) = icon {
        paint_icon(
            painter,
            icon,
            Pos2::new(x + 8.0, rect.center().y),
            16.0,
            ink.gamma_multiply(fade),
        );
        x += icon_space;
    }
    paint_text(
        painter,
        Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(rect.right() - 6.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        font,
        ink.gamma_multiply(fade),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), tip));
    let hover = ui.ctx().animate_bool_with_time(
        response.id,
        response.hovered() || response.has_focus(),
        0.1,
    );
    ui.painter()
        .rect_filled(rect, radius(10.0), p.raised.gamma_multiply(hover));
    paint_icon(
        ui.painter(),
        icon,
        rect.center(),
        18.0,
        p.dim.lerp_to_gamma(p.text, hover),
    );
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ controls

/// Pill-shaped choice between a few values. The selection slides between segments.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let p = pal(ui);
    let font = bold(14.0);
    let segment = options
        .iter()
        .map(|(_, label)| text_width(ui.painter(), label, font.clone()))
        .fold(0.0, f32::max)
        + 34.0;
    let inset = 3.0;
    let size = Vec2::new(segment * options.len() as f32 + inset * 2.0, 36.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let base = ui.make_persistent_id(id);
    let (track, pill) = if p.dark {
        (p.card, p.raised)
    } else {
        (p.raised, p.card)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, radius(12.0), track);
    painter.rect_stroke(rect, radius(12.0), hairline(ui), StrokeKind::Inside);
    let selected = options.iter().position(|(v, _)| v == value).unwrap_or(0);
    let slide = ui
        .ctx()
        .animate_value_with_time(base.with("slide"), selected as f32, 0.16);
    let pill_rect = Rect::from_min_size(
        rect.min + Vec2::new(inset + slide * segment, inset),
        Vec2::new(segment, size.y - inset * 2.0),
    );
    painter.rect_filled(pill_rect, radius(9.0), pill);
    if p.dark {
        painter.rect_stroke(pill_rect, radius(9.0), hairline(ui), StrokeKind::Inside);
    } else {
        painter.rect_stroke(
            pill_rect,
            radius(9.0),
            px_stroke(ui.ctx(), 1.0, Color32::from_black_alpha(18)),
            StrokeKind::Inside,
        );
    }
    let mut changed = false;
    for (i, (option, label)) in options.iter().enumerate() {
        let cell = Rect::from_min_size(
            rect.min + Vec2::new(inset + i as f32 * segment, inset),
            Vec2::new(segment, size.y - inset * 2.0),
        );
        let response = ui.interact(cell, base.with(i), Sense::click());
        let on = i == selected;
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::SelectableLabel, ui.is_enabled(), on, *label)
        });
        let hover =
            ui.ctx()
                .animate_bool_with_time(base.with(("hover", i)), response.hovered(), 0.1);
        let ink = if on {
            p.text
        } else {
            p.dim.lerp_to_gamma(p.text, hover)
        };
        paint_text(
            ui.painter(),
            cell,
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            ink,
        );
        if response.has_focus() {
            ui.painter()
                .rect_stroke(cell, radius(9.0), focus_ring(ui), StrokeKind::Inside);
        }
        if response
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
            && !on
        {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// Filter button that anchors a popup. Painted as `label value ⌄`.
pub fn chip(ui: &mut Ui, id: &str, label: &str, value: &str) -> Response {
    let p = pal(ui);
    let (label_font, value_font) = (sans(13.5), bold(14.0));
    let label_width = text_width(ui.painter(), label, label_font.clone());
    let value_width = text_width(ui.painter(), value, value_font.clone()).min(170.0);
    let width = 14.0 + label_width + 8.0 + value_width + 8.0 + 12.0 + 12.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, CONTROL_HEIGHT), Sense::hover());
    let response = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Button,
            ui.is_enabled(),
            format!("{label} {value}"),
        )
    });
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id, response.hovered() || open, 0.1);
    let painter = ui.painter();
    painter.rect_filled(rect, radius(17.0), p.card.lerp_to_gamma(p.raised, hover));
    painter.rect_stroke(
        rect,
        radius(17.0),
        px_stroke(
            ui.ctx(),
            1.0,
            if open || response.has_focus() {
                p.accent
            } else {
                p.hairline
            },
        ),
        StrokeKind::Inside,
    );
    let mid = rect.center().y;
    let mut x = rect.left() + 14.0;
    paint_text(
        painter,
        Rect::from_min_size(
            Pos2::new(x, rect.top()),
            Vec2::new(label_width + 2.0, rect.height()),
        ),
        Align2::LEFT_CENTER,
        label,
        label_font,
        p.dim,
    );
    x += label_width + 8.0;
    paint_text(
        painter,
        Rect::from_min_size(
            Pos2::new(x, rect.top()),
            Vec2::new(value_width + 2.0, rect.height()),
        ),
        Align2::LEFT_CENTER,
        value,
        value_font,
        p.text,
    );
    paint_icon(
        painter,
        Icon::Chevron,
        Pos2::new(rect.right() - 17.0, mid),
        14.0,
        p.dim,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A `chip` that opens `contents` as a popup beneath it.
pub fn dropdown(
    ui: &mut Ui,
    id: &str,
    label: &str,
    value: &str,
    close: PopupCloseBehavior,
    width: f32,
    contents: impl FnOnce(&mut Ui),
) -> Response {
    let response = chip(ui, id, label, value);
    egui::Popup::menu(&response)
        .close_behavior(close)
        .gap(6.0)
        .show(|ui| {
            // Rows fill the popup, so the width must be fixed rather than a minimum.
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 2.0;
            contents(ui);
        });
    response
}

fn menu_row(ui: &mut Ui) -> (Rect, Response) {
    ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(120.0), 32.0),
        Sense::click(),
    )
}

fn menu_hover(ui: &Ui, rect: Rect, response: &Response, selected: bool) {
    let p = pal(ui);
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(
            rect,
            radius(8.0),
            if response.hovered() {
                p.raised
            } else {
                p.accent_soft
            },
        );
    }
    if response.has_focus() {
        ui.painter()
            .rect_stroke(rect, radius(8.0), focus_ring(ui), StrokeKind::Inside);
    }
}

/// Gives text that is painted straight onto the canvas a node for screen readers, which
/// otherwise cannot see it. The node covers `rect` and reads out `label`.
pub fn describe(ui: &Ui, rect: Rect, label: &str) {
    let id = ui.id().with((
        "described",
        label,
        rect.min.x.round() as i32,
        rect.min.y.round() as i32,
    ));
    ui.interact(rect, id, Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), label));
}

/// Single-choice row with a check on the right of the chosen option.
pub fn menu_item(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let p = pal(ui);
    let (rect, response) = menu_row(ui);
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    menu_hover(ui, rect, &response, false);
    let inner = rect.shrink2(Vec2::new(10.0, 0.0));
    paint_text(
        ui.painter(),
        Rect::from_min_max(inner.min, Pos2::new(inner.right() - 22.0, inner.bottom())),
        Align2::LEFT_CENTER,
        label,
        if selected { bold(14.0) } else { sans(14.0) },
        p.text,
    );
    if selected {
        paint_icon(
            ui.painter(),
            Icon::Check,
            Pos2::new(inner.right() - 8.0, rect.center().y),
            16.0,
            p.accent,
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Multi-choice row with a checkbox on the left. Returns true when toggled.
pub fn check_item(ui: &mut Ui, checked: &mut bool, label: &str) -> bool {
    let p = pal(ui);
    let (rect, response) = menu_row(ui);
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *checked, label)
    });
    menu_hover(ui, rect, &response, false);
    let toggled = response.clicked();
    if toggled {
        *checked = !*checked;
    }
    let boxed = Rect::from_center_size(
        Pos2::new(rect.left() + 20.0, rect.center().y),
        Vec2::splat(18.0),
    );
    let painter = ui.painter();
    if *checked {
        painter.rect_filled(boxed, radius(5.0), p.accent);
        paint_icon(painter, Icon::Check, boxed.center(), 14.0, p.accent_ink);
    } else {
        painter.rect_stroke(
            boxed,
            radius(5.0),
            px_stroke(ui.ctx(), 1.0, p.dim.gamma_multiply(0.8)),
            StrokeKind::Inside,
        );
    }
    paint_text(
        painter,
        Rect::from_min_max(
            Pos2::new(rect.left() + 38.0, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        sans(14.0),
        p.text,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
    toggled
}

/// Wrapped caption for a popup. Popup menus use a justified layout that would
/// stretch a normal label's word spacing, so the text is painted left-aligned.
pub fn menu_note(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    let width = (ui.available_width() - 20.0).max(60.0);
    let galley = ui
        .painter()
        .layout(text.to_owned(), sans(12.5), p.dim, width);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), galley.size().y + 8.0),
        Sense::hover(),
    );
    ui.painter()
        .galley(rect.min + Vec2::new(10.0, 4.0), galley, p.dim);
}

pub fn menu_heading(ui: &mut Ui, text: &str) -> Response {
    let p = pal(ui);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(120.0), 26.0),
        Sense::hover(),
    );
    paint_text(
        ui.painter(),
        rect.shrink2(Vec2::new(10.0, 0.0)),
        Align2::LEFT_CENTER,
        text,
        bold(12.5),
        p.dim,
    );
    response
}

pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let p = pal(ui);
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(42.0, 24.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *on, label));
    let t = ui.ctx().animate_bool_with_time(response.id, *on, 0.14);
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let painter = ui.painter();
    let off = if p.dark {
        p.raised.lerp_to_gamma(p.text, 0.08)
    } else {
        p.dim.gamma_multiply(0.35)
    };
    painter.rect_filled(
        rect,
        radius(12.0),
        off.lerp_to_gamma(p.accent, t).gamma_multiply(fade),
    );
    let knob = Pos2::new(rect.left() + 12.0 + t * 18.0, rect.center().y);
    painter.circle_filled(
        knob + Vec2::new(0.0, 1.0),
        9.0,
        Color32::from_black_alpha(40),
    );
    painter.circle_filled(
        knob,
        8.5,
        if p.dark {
            Color32::from_gray(245)
        } else {
            Color32::WHITE
        }
        .gamma_multiply(fade),
    );
    if response.has_focus() {
        painter.rect_stroke(rect, radius(12.0), focus_ring(ui), StrokeKind::Outside);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    width: f32,
    label: &str,
) -> Response {
    let p = pal(ui);
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    if let Some(pointer) = response
        .interact_pointer_pos()
        .filter(|_| response.dragged() || response.clicked())
    {
        let t = ((pointer.x - rect.left() - 10.0) / (rect.width() - 20.0)).clamp(0.0, 1.0);
        let raw = lo + t * (hi - lo);
        let snapped = (lo + ((raw - lo) / step).round() * step).clamp(lo, hi);
        let next = (snapped * 100.0).round() / 100.0;
        if (next - *value).abs() > f32::EPSILON {
            *value = next;
            response.mark_changed();
        }
    }
    if response.has_focus() {
        let nudge = ui.input_mut(|input| {
            let none = egui::Modifiers::NONE;
            let up = input.consume_key(none, egui::Key::ArrowRight)
                || input.consume_key(none, egui::Key::ArrowUp);
            let down = input.consume_key(none, egui::Key::ArrowLeft)
                || input.consume_key(none, egui::Key::ArrowDown);
            f32::from(i8::from(up) - i8::from(down))
        });
        if nudge != 0.0 {
            let next = ((*value + nudge * step).clamp(lo, hi) * 100.0).round() / 100.0;
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                response.mark_changed();
            }
        }
    }
    response.widget_info(|| WidgetInfo::slider(ui.is_enabled(), f64::from(*value), label));
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let rail = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 20.0, 4.0));
    let knob = Pos2::new(rail.left() + rail.width() * t, rect.center().y);
    let painter = ui.painter();
    painter.rect_filled(rail, radius(2.0), p.raised.lerp_to_gamma(p.text, 0.1));
    painter.rect_filled(
        Rect::from_min_max(rail.min, Pos2::new(knob.x, rail.bottom())),
        radius(2.0),
        p.accent,
    );
    painter.circle_filled(
        knob + Vec2::new(0.0, 1.0),
        10.0,
        Color32::from_black_alpha(40),
    );
    painter.circle_filled(
        knob,
        9.0,
        if p.dark {
            Color32::from_gray(245)
        } else {
            Color32::WHITE
        },
    );
    if response.has_focus() || response.dragged() {
        painter.circle_stroke(knob, 11.5, focus_ring(ui));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Pill-shaped text input with a leading magnifier.
/// The one search box on screen; Ctrl+F focuses it.
pub const SEARCH_ID: &str = "onm-search";

pub fn search_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let p = pal(ui);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, CONTROL_HEIGHT), Sense::hover());
    let background = ui.painter().add(Shape::Noop);
    paint_icon(
        ui.painter(),
        Icon::Search,
        Pos2::new(rect.left() + 17.0, rect.center().y),
        16.0,
        p.dim,
    );
    let clear_width = if text.is_empty() { 0.0 } else { 28.0 };
    let edit = Rect::from_min_max(
        Pos2::new(rect.left() + 34.0, rect.top()),
        Pos2::new(rect.right() - 12.0 - clear_width, rect.bottom()),
    );
    // A child ui keeps the parent's cursor at the end of the pill.
    let mut field = ui.new_child(
        UiBuilder::new()
            .max_rect(edit)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let response = field.add(
        egui::TextEdit::singleline(text)
            .id(Id::new(SEARCH_ID))
            .hint_text(hint)
            .frame(egui::Frame::NONE)
            .margin(Margin::ZERO)
            .vertical_align(Align::Center)
            .desired_width(edit.width()),
    );
    if clear_width > 0.0 {
        let spot = Rect::from_center_size(
            Pos2::new(rect.right() - 20.0, rect.center().y),
            Vec2::splat(24.0),
        );
        let clear = ui.interact(spot, response.id.with("clear"), Sense::click());
        paint_icon(
            ui.painter(),
            Icon::Close,
            spot.center(),
            14.0,
            if clear.hovered() { p.text } else { p.dim },
        );
        if clear
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            text.clear();
        }
    }
    ui.painter().set(
        background,
        Shape::Vec(vec![
            Shape::rect_filled(rect, radius(17.0), p.card),
            Shape::rect_stroke(
                rect,
                radius(17.0),
                px_stroke(
                    ui.ctx(),
                    1.0,
                    if response.has_focus() {
                        p.accent
                    } else {
                        p.hairline
                    },
                ),
                StrokeKind::Inside,
            ),
        ]),
    );
    response
}

/// Title and description on the left, `control` on the right. `control_width` is
/// reserved for the control so the text wraps in the remaining space.
pub fn setting_row(
    ui: &mut Ui,
    title: &str,
    description: &str,
    control_width: f32,
    control: impl FnOnce(&mut Ui),
) {
    let p = pal(ui);
    let width = ui.available_width();
    let text_width = (width - control_width - 24.0).max(120.0);
    let painter = ui.painter().clone();
    let title_galley = painter.layout(title.to_owned(), bold(15.0), p.text, text_width);
    let description_galley = (!description.is_empty())
        .then(|| painter.layout(description.to_owned(), sans(13.5), p.dim, text_width));
    let text_height = title_galley.size().y
        + description_galley
            .as_ref()
            .map_or(0.0, |g| g.size().y + 3.0);
    let height = (text_height + 24.0).max(52.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let top = rect.center().y - text_height / 2.0;
    painter.galley(Pos2::new(rect.left(), top), title_galley.clone(), p.text);
    if let Some(galley) = description_galley {
        painter.galley(
            Pos2::new(rect.left(), top + title_galley.size().y + 3.0),
            galley,
            p.dim,
        );
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(
                Pos2::new(rect.right() - control_width, rect.top()),
                rect.max,
            ))
            .layout(Layout::right_to_left(Align::Center)),
    );
    control(&mut child);
}

pub fn navigation(ui: &mut Ui, title: &str, icon: Icon, selected: bool, rail: bool) -> Response {
    let p = pal(ui);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            title,
        )
    });
    let hover = ui
        .ctx()
        .animate_bool_with_time(response.id.with("hover"), response.hovered(), 0.1);
    let chosen = ui
        .ctx()
        .animate_bool_with_time(response.id.with("chosen"), selected, 0.16);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        radius(11.0),
        p.raised.gamma_multiply(chosen.max(hover * 0.55)),
    );
    if response.has_focus() {
        painter.rect_stroke(rect, radius(11.0), focus_ring(ui), StrokeKind::Inside);
    }
    let ink = p.dim.lerp_to_gamma(p.text, chosen.max(hover));
    let icon_ink = p
        .dim
        .lerp_to_gamma(p.accent, chosen)
        .lerp_to_gamma(p.text, hover * (1.0 - chosen));
    let icon_center = if rail {
        rect.center()
    } else {
        Pos2::new(rect.left() + 24.0, rect.center().y)
    };
    paint_icon(painter, icon, icon_center, 20.0, icon_ink);
    if !rail {
        paint_text(
            painter,
            Rect::from_min_max(
                Pos2::new(rect.left() + 46.0, rect.top()),
                Pos2::new(rect.right() - 8.0, rect.bottom()),
            ),
            Align2::LEFT_CENTER,
            title,
            if selected { bold(14.5) } else { sans(14.5) },
            ink,
        );
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if rail {
        response.on_hover_text(title)
    } else {
        response
    }
}

// --------------------------------------------------------------------- bars

/// A rounded bar split into proportional segments with a small gap between them.
pub fn split_bar(painter: &Painter, rect: Rect, segments: &[(f32, Color32)], track: Color32) {
    let r = radius(rect.height() / 2.0);
    painter.rect_filled(rect, r, track);
    let visible: Vec<_> = segments.iter().filter(|(f, _)| *f > 0.0).collect();
    let mut x = rect.left();
    for (i, (fraction, color)) in visible.iter().enumerate() {
        let right = if i + 1 == visible.len() {
            rect.right()
        } else {
            (x + rect.width() * fraction).min(rect.right())
        };
        let gap = if visible.len() > 1 { 1.0 } else { 0.0 };
        let cell = Rect::from_min_max(
            Pos2::new(if i == 0 { x } else { x + gap }, rect.top()),
            Pos2::new(
                if i + 1 == visible.len() {
                    right
                } else {
                    right - gap
                },
                rect.bottom(),
            ),
        );
        if cell.width() > 0.0 {
            painter.rect_filled(cell, r, *color);
        }
        x = right;
    }
}

// ------------------------------------------------------------------ columns

#[derive(Clone, Copy)]
pub enum Col {
    Flex(f32),
    Fixed(f32),
}

/// Lays out `N` side-by-side columns that reach the bottom of the available
/// space. Whole-number origins keep card edges crisp at fractional scaling.
pub fn columns<const N: usize, R>(
    ui: &mut Ui,
    spec: [Col; N],
    gap: f32,
    contents: impl FnOnce(&mut [Ui; N]) -> R,
) -> R {
    let bounds = ui.available_rect_before_wrap();
    let room = (bounds.width() - gap * (N as f32 - 1.0)).max(0.0);
    let fixed: f32 = spec
        .iter()
        .map(|c| if let Col::Fixed(w) = c { *w } else { 0.0 })
        .sum();
    let flex: f32 = spec
        .iter()
        .map(|c| if let Col::Flex(w) = c { *w } else { 0.0 })
        .sum();
    let spare = (room - fixed).max(0.0);
    let mut left = bounds.left();
    let mut cols = std::array::from_fn(|i| {
        let right = if i == N - 1 {
            bounds.right()
        } else {
            left + match spec[i] {
                Col::Fixed(w) => w,
                Col::Flex(w) => (spare * w / flex.max(f32::EPSILON)).floor(),
            }
        };
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    Pos2::new(left, bounds.top()),
                    Pos2::new(right, bounds.bottom()),
                ))
                .layout(Layout::top_down_justified(Align::Min)),
        );
        child.set_width(right - left);
        left = right + gap;
        child
    });
    let out = contents(&mut cols);
    let height = cols.iter().map(|c| c.min_size().y).fold(0.0f32, f32::max);
    ui.advance_cursor_after_rect(Rect::from_min_size(
        bounds.min,
        Vec2::new(bounds.width(), height),
    ));
    out
}
