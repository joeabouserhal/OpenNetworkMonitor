//! Form controls and small display pieces: badge, switch, checkbox, slider, input,
//! progress, separator, kbd, alert, skeleton and avatar.
use super::{
    icon::{self, Icon},
    text,
    tokens::{RADIUS, Tokens, hairline, tk},
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Layout, Margin, Pos2, Rect, Response,
    Sense, Stroke, StrokeKind, TextEdit, TextureId, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType,
};
use std::ops::RangeInclusive;

fn corner(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

// ---------------------------------------------------------------------------- badge

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadgeVariant {
    Default,
    Secondary,
    Outline,
    Destructive,
    Success,
}

/// A small pill of text. Returns its rectangle.
pub fn badge(ui: &mut Ui, variant: BadgeVariant, label: &str) -> Rect {
    let t = tk(ui.ctx());
    let font = text::medium(12.0);
    let width = text::text_width(ui.painter(), label, font.clone()) + 20.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 22.0), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, label));
    let (fill, ink, border) = match variant {
        BadgeVariant::Default => (t.primary, t.primary_foreground, None),
        BadgeVariant::Secondary => (t.secondary, t.foreground, None),
        BadgeVariant::Outline => (Color32::TRANSPARENT, t.foreground, Some(t.border)),
        BadgeVariant::Destructive => (t.destructive, Color32::WHITE, None),
        BadgeVariant::Success => (
            t.success.gamma_multiply(0.14),
            t.success,
            Some(t.success.gamma_multiply(0.35)),
        ),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, corner(11.0), fill);
    if let Some(border) = border {
        painter.rect_stroke(
            rect,
            corner(11.0),
            hairline(ui.ctx(), border),
            StrokeKind::Inside,
        );
    }
    text::paint(painter, rect, Align2::CENTER_CENTER, label, font, ink);
    rect
}

// ------------------------------------------------------------------------ separator

pub fn separator(ui: &mut Ui) {
    let t = tk(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        hairline(ui.ctx(), t.border),
    );
}

// -------------------------------------------------------------------------- kbd

pub fn kbd(ui: &mut Ui, keys: &str) {
    let t = tk(ui.ctx());
    let font = text::medium(11.5);
    let width = text::text_width(ui.painter(), keys, font.clone()) + 12.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width.max(22.0), 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, corner(4.0), t.muted);
    text::paint(
        painter,
        rect,
        Align2::CENTER_CENTER,
        keys,
        font,
        t.muted_foreground,
    );
}

// ------------------------------------------------------------------------- switch

pub fn switch(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = tk(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(36.0, 20.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let value = *on;
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, value, label));
    let eased = ui.ctx().animate_bool_with_time(response.id, *on, 0.12);
    let track = t.input.lerp_to_gamma(t.primary, eased);
    let track = if t.is_dark() && !*on {
        super::tokens::over(t.input, t.background)
    } else {
        track
    };
    let painter = ui.painter();
    painter.rect_filled(rect, corner(10.0), track);
    let knob_x = rect.left() + 10.0 + 16.0 * eased;
    let knob = if *on {
        t.primary_foreground
    } else {
        t.background
    };
    let knob = if t.is_dark() && !*on {
        t.foreground
    } else {
        knob
    };
    painter.circle_filled(Pos2::new(knob_x, rect.center().y), 8.0, knob);
    if response.has_focus() {
        super::focus_ring(painter, ui.ctx(), rect, corner(10.0), &t);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

// ----------------------------------------------------------------------- checkbox

pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> Response {
    let t = tk(ui.ctx());
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let value = *checked;
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, value, label));
    let painter = ui.painter();
    if *checked {
        painter.rect_filled(rect, corner(4.0), t.primary);
        icon::paint(
            painter,
            Icon::Check,
            rect.center(),
            12.0,
            t.primary_foreground,
        );
    } else {
        painter.rect_stroke(
            rect,
            corner(4.0),
            hairline(ui.ctx(), t.input.gamma_multiply(1.4)),
            StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        super::focus_ring(painter, ui.ctx(), rect, corner(4.0), &t);
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

// ------------------------------------------------------------------------- slider

/// A horizontal slider that also moves with the arrow keys when focused.
pub fn slider(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step: f32,
    width: f32,
    label: &str,
) -> Response {
    let t = tk(ui.ctx());
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let snapped = |raw: f32| {
        let stepped = lo + ((raw - lo) / step).round() * step;
        (stepped.clamp(lo, hi) * 100.0).round() / 100.0
    };
    if let Some(pointer) = response
        .interact_pointer_pos()
        .filter(|_| response.dragged() || response.clicked())
    {
        let fraction = ((pointer.x - rect.left() - 8.0) / (rect.width() - 16.0)).clamp(0.0, 1.0);
        let next = snapped(lo + fraction * (hi - lo));
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
            let next = snapped(*value + nudge * step);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                response.mark_changed();
            }
        }
    }
    let shown = f64::from(*value);
    response.widget_info(|| WidgetInfo::slider(true, shown, label));
    let fraction = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let rail = Rect::from_center_size(rect.center(), Vec2::new(rect.width() - 16.0, 6.0));
    let knob = Pos2::new(rail.left() + rail.width() * fraction, rect.center().y);
    let painter = ui.painter();
    painter.rect_filled(rail, corner(3.0), t.muted);
    painter.rect_filled(
        Rect::from_min_max(rail.min, Pos2::new(knob.x, rail.bottom())),
        corner(3.0),
        t.primary,
    );
    painter.circle_filled(knob, 8.0, t.background);
    painter.circle_stroke(knob, 8.0, Stroke::new(1.5, t.primary));
    if response.has_focus() || response.dragged() {
        painter.circle_stroke(knob, 10.5, Stroke::new(3.0, t.ring.gamma_multiply(0.5)));
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

// -------------------------------------------------------------------------- input

/// A bordered single-line text field. Returns the editor's response.
pub fn input(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    field(ui, value, hint, width, None, text::regular(14.0))
}

/// The same field with a leading magnifier and a clear button.
pub fn search(ui: &mut Ui, value: &mut String, hint: &str, width: f32) -> Response {
    field(
        ui,
        value,
        hint,
        width,
        Some(Icon::Search),
        text::regular(14.0),
    )
}

/// The id of the search box on screen, which Ctrl+F focuses.
pub const SEARCH_ID: &str = "onm-ui-search";

fn field(
    ui: &mut Ui,
    value: &mut String,
    hint: &str,
    width: f32,
    leading: Option<Icon>,
    font: egui::FontId,
) -> Response {
    let t = tk(ui.ctx());
    let height = 36.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    let left = if leading.is_some() { 34.0 } else { 12.0 };
    let clearable = leading.is_some() && !value.is_empty();
    let right = if clearable { 32.0 } else { 12.0 };
    let edit_rect = Rect::from_min_max(
        Pos2::new(rect.left() + left, rect.top()),
        Pos2::new(rect.right() - right, rect.bottom()),
    );
    let background = ui.painter().add(egui::Shape::Noop);
    let mut inner = ui.new_child(
        UiBuilder::new()
            .max_rect(edit_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let mut editor = TextEdit::singleline(value)
        .font(font)
        .hint_text(hint)
        .frame(egui::Frame::NONE)
        .margin(Margin::ZERO)
        .vertical_align(Align::Center)
        .desired_width(edit_rect.width());
    if leading.is_some() {
        editor = editor.id(egui::Id::new(SEARCH_ID));
    }
    let response = inner.add(editor);
    if let Some(glyph) = leading {
        icon::paint(
            ui.painter(),
            glyph,
            Pos2::new(rect.left() + 17.0, rect.center().y),
            15.0,
            t.muted_foreground,
        );
    }
    if clearable {
        let spot = Rect::from_center_size(
            Pos2::new(rect.right() - 18.0, rect.center().y),
            Vec2::splat(22.0),
        );
        let clear = ui.interact(spot, response.id.with("clear"), Sense::click());
        clear.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Clear search"));
        icon::paint(
            ui.painter(),
            Icon::Close,
            spot.center(),
            14.0,
            if clear.hovered() {
                t.foreground
            } else {
                t.muted_foreground
            },
        );
        if clear.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            value.clear();
        }
    }
    let focused = response.has_focus();
    let painter = ui.painter();
    painter.set(
        background,
        egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, corner(RADIUS), t.background),
            egui::Shape::rect_stroke(
                rect,
                corner(RADIUS),
                hairline(ui.ctx(), if focused { t.ring } else { t.input }),
                StrokeKind::Inside,
            ),
        ]),
    );
    if focused {
        super::focus_ring(ui.painter(), ui.ctx(), rect, corner(RADIUS), &t);
    }
    response
}

// ----------------------------------------------------------------------- progress

pub fn progress(ui: &mut Ui, fraction: f32, tone: Option<Color32>) {
    let t = tk(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
    let percent = (fraction.clamp(0.0, 1.0) * 100.0).round();
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::ProgressIndicator,
            true,
            format!("{percent:.0}%"),
        )
    });
    let painter = ui.painter();
    painter.rect_filled(rect, corner(4.0), t.muted);
    let width = rect.width() * fraction.clamp(0.0, 1.0);
    if width > 0.5 {
        painter.rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(width, rect.height())),
            corner(4.0),
            tone.unwrap_or(t.primary),
        );
    }
}

// -------------------------------------------------------------------------- alert

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertVariant {
    Default,
    Destructive,
}

/// A bordered message with an icon, a title and a description. Returns true when its
/// close button is pressed.
pub fn alert(
    ui: &mut Ui,
    variant: AlertVariant,
    icon_kind: Icon,
    title: &str,
    description: &str,
    dismissible: bool,
) -> bool {
    let t = tk(ui.ctx());
    let tone = match variant {
        AlertVariant::Default => t.foreground,
        AlertVariant::Destructive => t.destructive,
    };
    let mut dismissed = false;
    let width = ui.available_width();
    let inner_width = width - 16.0 * 2.0 - 28.0 - if dismissible { 30.0 } else { 0.0 };
    let painter = ui.painter().clone();
    let title_galley = painter.layout(
        title.to_owned(),
        text::medium(14.0),
        tone,
        inner_width.max(60.0),
    );
    let body_galley = (!description.is_empty()).then(|| {
        painter.layout(
            description.to_owned(),
            text::regular(13.5),
            t.muted_foreground,
            inner_width.max(60.0),
        )
    });
    let height =
        16.0 * 2.0 + title_galley.size().y + body_galley.as_ref().map_or(0.0, |g| g.size().y + 4.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Label, true, format!("{title}. {description}"))
    });
    painter.rect_filled(rect, corner(RADIUS), t.card);
    painter.rect_stroke(
        rect,
        corner(RADIUS),
        hairline(
            ui.ctx(),
            if variant == AlertVariant::Destructive {
                tone.gamma_multiply(0.5)
            } else {
                t.border
            },
        ),
        StrokeKind::Inside,
    );
    icon::paint(
        &painter,
        icon_kind,
        Pos2::new(rect.left() + 16.0 + 8.0, rect.top() + 16.0 + 9.0),
        16.0,
        tone,
    );
    let left = rect.left() + 16.0 + 28.0;
    painter.galley(
        Pos2::new(left, rect.top() + 16.0),
        title_galley.clone(),
        tone,
    );
    if let Some(body) = body_galley {
        painter.galley(
            Pos2::new(left, rect.top() + 16.0 + title_galley.size().y + 4.0),
            body,
            t.muted_foreground,
        );
    }
    if dismissible {
        let spot = Rect::from_center_size(
            Pos2::new(rect.right() - 24.0, rect.top() + 26.0),
            Vec2::splat(26.0),
        );
        let close = ui.interact(spot, response.id.with("close"), Sense::click());
        close.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Dismiss"));
        if close.hovered() {
            painter.rect_filled(spot, corner(6.0), t.accent);
        }
        icon::paint(
            &painter,
            Icon::Close,
            spot.center(),
            14.0,
            t.muted_foreground,
        );
        dismissed = close.on_hover_cursor(CursorIcon::PointingHand).clicked();
    }
    ui.add_space(8.0);
    dismissed
}

// ------------------------------------------------------------------------ skeleton

pub fn skeleton(ui: &mut Ui, size: Vec2) {
    let t = tk(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, corner(6.0), t.muted);
}

// -------------------------------------------------------------------------- avatar

/// A rounded tile for an app: its executable icon when available, else its initial.
pub fn avatar(
    painter: &egui::Painter,
    t: &Tokens,
    rect: Rect,
    name: &str,
    texture: Option<TextureId>,
) {
    painter.rect_filled(rect, corner(6.0), t.muted);
    if let Some(texture) = texture {
        let inner = Rect::from_center_size(rect.center(), rect.size() * 0.66);
        painter.image(
            texture,
            inner,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }
    let initial = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    text::paint(
        painter,
        rect,
        Align2::CENTER_CENTER,
        &initial,
        text::semibold(rect.height() * 0.42),
        t.muted_foreground,
    );
}
