//! Buttons, in shadcn's variants and sizes.
use super::{
    icon::{self, Icon},
    text,
    tokens::{RADIUS, Tokens, hairline, tk},
};
use eframe::egui::{
    Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, WidgetInfo, WidgetType,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    Default,
    Secondary,
    Outline,
    Ghost,
    Destructive,
    Link,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Sm,
    Md,
    Lg,
    Icon,
}
impl Size {
    pub fn height(self) -> f32 {
        match self {
            Self::Sm => 32.0,
            Self::Md | Self::Icon => 36.0,
            Self::Lg => 40.0,
        }
    }
    fn padding(self) -> f32 {
        match self {
            Self::Sm => 12.0,
            Self::Md | Self::Icon => 16.0,
            Self::Lg => 24.0,
        }
    }
}

pub struct Button<'a> {
    label: &'a str,
    variant: Variant,
    size: Size,
    icon: Option<Icon>,
    trailing: Option<Icon>,
    width: Option<f32>,
    enabled: bool,
    selected: bool,
}
impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            variant: Variant::Default,
            size: Size::Md,
            icon: None,
            trailing: None,
            width: None,
            enabled: true,
            selected: false,
        }
    }
    /// A square button holding only an icon; `label` is what screen readers say.
    pub fn icon_only(icon: Icon, label: &'a str) -> Self {
        Self {
            icon: Some(icon),
            size: Size::Icon,
            ..Self::new(label)
        }
    }
    pub fn variant(mut self, variant: Variant) -> Self {
        self.variant = variant;
        self
    }
    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn trailing(mut self, icon: Icon) -> Self {
        self.trailing = Some(icon);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Shows the button as pressed, for toggles.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let t = tk(ui.ctx());
        let font = text::medium(14.0);
        let icon_only = self.size == Size::Icon;
        let label_width = if icon_only {
            0.0
        } else {
            text::text_width(ui.painter(), self.label, font.clone())
        };
        let glyph = 16.0;
        let gap = 8.0;
        let mut content = label_width;
        if self.icon.is_some() && !icon_only {
            content += glyph + gap;
        }
        if self.trailing.is_some() {
            content += glyph + gap;
        }
        let width = if icon_only {
            self.size.height()
        } else {
            self.width
                .unwrap_or(content + self.size.padding() * 2.0)
                .max(self.size.height())
        };
        let size = Vec2::new(width, self.size.height());
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(size, sense);
        let label = self.label;
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, self.enabled, label));
        let (fill, ink, border) = colors(&t, self.variant, response.hovered(), self.selected);
        let fade = if self.enabled { 1.0 } else { 0.5 };
        let painter = ui.painter();
        let radius = CornerRadius::same(RADIUS as u8);
        painter.rect_filled(rect, radius, fill.gamma_multiply(fade));
        if let Some(border) = border {
            painter.rect_stroke(
                rect,
                radius,
                hairline(ui.ctx(), border.gamma_multiply(fade)),
                StrokeKind::Inside,
            );
        }
        if self.enabled && response.has_focus() {
            super::focus_ring(painter, ui.ctx(), rect, radius, &t);
        }
        let ink = ink.gamma_multiply(fade);
        let mid = rect.center().y;
        if icon_only {
            if let Some(glyph_icon) = self.icon {
                icon::paint(painter, glyph_icon, rect.center(), glyph, ink);
            }
        } else {
            let mut x = rect.center().x - content / 2.0;
            if let Some(glyph_icon) = self.icon {
                icon::paint(
                    painter,
                    glyph_icon,
                    Pos2::new(x + glyph / 2.0, mid),
                    glyph,
                    ink,
                );
                x += glyph + gap;
            }
            let text_rect = Rect::from_min_size(
                Pos2::new(x, rect.top()),
                Vec2::new(label_width + 1.0, rect.height()),
            );
            let placed = text::paint(
                painter,
                text_rect,
                Align2::LEFT_CENTER,
                self.label,
                font,
                ink,
            );
            if self.variant == Variant::Link && response.hovered() {
                painter.hline(
                    placed.x_range(),
                    placed.bottom() + 1.0,
                    Stroke::new(1.0, ink),
                );
            }
            x += label_width + gap;
            if let Some(trailing) = self.trailing {
                icon::paint(
                    painter,
                    trailing,
                    Pos2::new(x + glyph / 2.0, mid),
                    glyph,
                    ink,
                );
            }
        }
        if self.enabled {
            response.on_hover_cursor(CursorIcon::PointingHand)
        } else {
            response
        }
    }
}

/// `(fill, text, border)` for a variant in its resting or hovered state.
fn colors(
    t: &Tokens,
    variant: Variant,
    hovered: bool,
    selected: bool,
) -> (Color32, Color32, Option<Color32>) {
    let strength = if hovered || selected { 1.0 } else { 0.0 };
    let toward =
        |from: Color32, to: Color32, amount: f32| from.lerp_to_gamma(to, amount * strength);
    match variant {
        Variant::Default => (
            toward(t.primary, t.background, 0.12),
            t.primary_foreground,
            None,
        ),
        Variant::Secondary => (toward(t.secondary, t.foreground, 0.06), t.foreground, None),
        Variant::Outline => (
            if hovered || selected {
                t.accent
            } else {
                t.background
            },
            t.foreground,
            Some(t.input),
        ),
        Variant::Ghost => (
            if hovered || selected {
                t.accent
            } else {
                Color32::TRANSPARENT
            },
            t.foreground,
            None,
        ),
        Variant::Destructive => (
            toward(t.destructive, t.background, 0.12),
            Color32::WHITE,
            None,
        ),
        Variant::Link => (Color32::TRANSPARENT, t.primary, None),
    }
}
