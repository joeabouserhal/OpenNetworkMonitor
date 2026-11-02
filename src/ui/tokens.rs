//! Colour tokens, modelled on shadcn/ui's CSS variables.
//!
//! The neutrals are shadcn's zinc scale. A base colour only changes the primary
//! colour, the focus ring and the download series, as in shadcn's theme picker.
use eframe::egui::{self, Color32, Id, Stroke};
use serde::{Deserialize, Serialize};

pub const RADIUS: f32 = 8.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scheme {
    #[default]
    Light,
    Dark,
}

/// shadcn's base colours. Variant names are saved in preferences; keep them stable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaseColor {
    #[default]
    Zinc,
    Slate,
    Stone,
    Red,
    Rose,
    Orange,
    Green,
    Blue,
    Yellow,
    Violet,
}
impl BaseColor {
    pub const ALL: [Self; 10] = [
        Self::Zinc,
        Self::Slate,
        Self::Stone,
        Self::Red,
        Self::Rose,
        Self::Orange,
        Self::Green,
        Self::Blue,
        Self::Yellow,
        Self::Violet,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Zinc => "Zinc",
            Self::Slate => "Slate",
            Self::Stone => "Stone",
            Self::Red => "Red",
            Self::Rose => "Rose",
            Self::Orange => "Orange",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Yellow => "Yellow",
            Self::Violet => "Violet",
        }
    }
    /// The primary colour for a scheme.
    pub fn primary(self, scheme: Scheme) -> Color32 {
        let (light, dark) = match self {
            Self::Zinc => (0x18181b, 0xe4e4e7),
            Self::Slate => (0x0f172a, 0xe2e8f0),
            Self::Stone => (0x1c1917, 0xe7e5e4),
            Self::Red => (0xdc2626, 0xef4444),
            Self::Rose => (0xe11d48, 0xf43f5e),
            Self::Orange => (0xea580c, 0xf97316),
            Self::Green => (0x16a34a, 0x22c55e),
            Self::Blue => (0x2563eb, 0x3b82f6),
            Self::Yellow => (0xeab308, 0xfacc15),
            Self::Violet => (0x7c3aed, 0xa78bfa),
        };
        rgb(if scheme == Scheme::Light { light } else { dark })
    }
    /// Text on top of the primary colour.
    fn primary_foreground(self, scheme: Scheme) -> Color32 {
        match (self, scheme) {
            (Self::Zinc | Self::Slate | Self::Stone, Scheme::Dark) => rgb(0x18181b),
            (Self::Zinc | Self::Slate | Self::Stone, Scheme::Light) => rgb(0xfafafa),
            // Coloured bases take whichever of white and near-black reads better.
            _ => {
                let fill = self.primary(scheme);
                let (white, ink) = (rgb(0xffffff), rgb(0x18181b));
                if contrast(white, fill) >= contrast(ink, fill) {
                    white
                } else {
                    ink
                }
            }
        }
    }
}

pub const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}
fn alpha(hex: u32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, a)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub scheme: Scheme,
    pub base: BaseColor,
    pub background: Color32,
    pub foreground: Color32,
    pub card: Color32,
    pub popover: Color32,
    pub primary: Color32,
    pub primary_foreground: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub muted_foreground: Color32,
    pub accent: Color32,
    pub destructive: Color32,
    pub border: Color32,
    pub input: Color32,
    pub ring: Color32,
    /// Green for "connected" and "active" states.
    pub success: Color32,
    pub warning: Color32,
    /// Download and upload series.
    pub chart_down: Color32,
    pub chart_up: Color32,
}

impl Tokens {
    pub fn new(scheme: Scheme, base: BaseColor) -> Self {
        let primary = base.primary(scheme);
        let chart_down = match base {
            BaseColor::Zinc | BaseColor::Slate | BaseColor::Stone => match scheme {
                Scheme::Light => rgb(0x2563eb),
                Scheme::Dark => rgb(0x3b82f6),
            },
            BaseColor::Yellow if scheme == Scheme::Light => rgb(0xa16207),
            _ => primary,
        };
        // Upload is amber, unless the base colour is already close to it.
        let chart_up = match (base, scheme) {
            (BaseColor::Orange | BaseColor::Yellow, Scheme::Light) => rgb(0x0d9488),
            (BaseColor::Orange | BaseColor::Yellow, Scheme::Dark) => rgb(0x2dd4bf),
            (_, Scheme::Light) => rgb(0xea580c),
            (_, Scheme::Dark) => rgb(0xfb923c),
        };
        let common = |ring: Color32| (primary, base.primary_foreground(scheme), ring);
        match scheme {
            Scheme::Light => {
                let (primary, primary_foreground, ring) = common(primary.gamma_multiply(0.45));
                Self {
                    scheme,
                    base,
                    background: rgb(0xffffff),
                    foreground: rgb(0x09090b),
                    card: rgb(0xffffff),
                    popover: rgb(0xffffff),
                    primary,
                    primary_foreground,
                    secondary: rgb(0xf4f4f5),
                    muted: rgb(0xf4f4f5),
                    muted_foreground: rgb(0x71717a),
                    accent: rgb(0xf4f4f5),
                    destructive: rgb(0xdc2626),
                    border: rgb(0xe4e4e7),
                    input: rgb(0xe4e4e7),
                    ring,
                    success: rgb(0x16a34a),
                    warning: rgb(0xd97706),
                    chart_down,
                    chart_up,
                }
            }
            Scheme::Dark => {
                let (primary, primary_foreground, ring) = common(primary.gamma_multiply(0.5));
                Self {
                    scheme,
                    base,
                    background: rgb(0x09090b),
                    foreground: rgb(0xfafafa),
                    card: rgb(0x18181b),
                    popover: rgb(0x18181b),
                    primary,
                    primary_foreground,
                    secondary: rgb(0x27272a),
                    muted: rgb(0x27272a),
                    muted_foreground: rgb(0xa1a1aa),
                    accent: rgb(0x27272a),
                    destructive: rgb(0xf87171),
                    border: alpha(0xffffff, 26),
                    input: alpha(0xffffff, 38),
                    ring,
                    success: rgb(0x4ade80),
                    warning: rgb(0xfbbf24),
                    chart_down,
                    chart_up,
                }
            }
        }
    }
    pub fn is_dark(&self) -> bool {
        self.scheme == Scheme::Dark
    }
}

const TOKENS_ID: &str = "onm-ui-tokens";

/// Stores the tokens for every control and restyles egui's own widgets to match.
pub fn install(ctx: &egui::Context, tokens: Tokens) {
    ctx.data_mut(|d| d.insert_temp(Id::new(TOKENS_ID), tokens));
    ctx.set_theme(if tokens.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let mut v = if tokens.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.override_text_color = None;
    v.weak_text_color = Some(tokens.muted_foreground);
    v.panel_fill = tokens.background;
    v.window_fill = tokens.popover;
    v.faint_bg_color = tokens.muted;
    v.extreme_bg_color = tokens.background;
    v.window_stroke = Stroke::new(1.0 / ctx.pixels_per_point(), tokens.border);
    v.window_corner_radius = egui::CornerRadius::same(10);
    v.menu_corner_radius = egui::CornerRadius::same(8);
    v.selection.bg_fill = tokens.primary.gamma_multiply(0.25);
    v.selection.stroke = Stroke::new(1.0, tokens.primary);
    v.text_cursor.stroke = Stroke::new(1.5, tokens.foreground);
    v.window_shadow = egui::Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(if tokens.is_dark() { 110 } else { 28 }),
    };
    v.popup_shadow = v.window_shadow;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.scroll = egui::style::ScrollStyle::floating();
    });
}

pub fn tk(ctx: &egui::Context) -> Tokens {
    ctx.data(|d| d.get_temp::<Tokens>(Id::new(TOKENS_ID)))
        .unwrap_or_else(|| Tokens::new(Scheme::Light, BaseColor::Zinc))
}

/// A one-physical-pixel stroke, so borders stay crisp at any display scaling.
pub fn hairline(ctx: &egui::Context, color: Color32) -> Stroke {
    Stroke::new(1.0 / ctx.pixels_per_point(), color)
}

/// Rounds a length in points to a whole number of physical pixels.
pub fn snap(ctx: &egui::Context, points: f32) -> f32 {
    let ppp = ctx.pixels_per_point();
    (points * ppp).round() / ppp
}

/// WCAG contrast ratio between two opaque colours.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    fn luminance(c: Color32) -> f32 {
        let channel = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
    }
    let (hi, lo) = {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y), x.min(y))
    };
    (hi + 0.05) / (lo + 0.05)
}

/// `top` laid over `bottom`, as the screen would show it.
pub fn over(top: Color32, bottom: Color32) -> Color32 {
    let a = f32::from(top.a()) / 255.0;
    let mix = |t: u8, b: u8| (f32::from(t) * a + f32::from(b) * (1.0 - a)).round() as u8;
    Color32::from_rgb(
        mix(top.r(), bottom.r()),
        mix(top.g(), bottom.g()),
        mix(top.b(), bottom.b()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_pairs_meet_wcag_aa_in_every_scheme_and_base_colour() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            for base in BaseColor::ALL {
                let t = Tokens::new(scheme, base);
                let card = over(t.card, t.background);
                let border = over(t.border, t.background);
                let checks = [
                    ("foreground on background", t.foreground, t.background, 7.0),
                    ("foreground on card", t.foreground, card, 7.0),
                    (
                        "muted text on background",
                        t.muted_foreground,
                        t.background,
                        4.5,
                    ),
                    ("muted text on card", t.muted_foreground, card, 4.5),
                    ("muted text on muted", t.muted_foreground, t.muted, 4.0),
                    (
                        "primary text on primary",
                        t.primary_foreground,
                        t.primary,
                        4.5,
                    ),
                    ("download on card", t.chart_down, card, 3.0),
                    ("upload on card", t.chart_up, card, 3.0),
                ];
                for (what, fg, bg, want) in checks {
                    let got = contrast(fg, bg);
                    assert!(got >= want, "{scheme:?} {base:?} {what}: {got:.2} < {want}");
                }
                assert!(border != t.background, "Borders must be visible");
            }
        }
    }

    #[test]
    fn the_two_series_are_distinguishable_in_every_theme() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            for base in BaseColor::ALL {
                let t = Tokens::new(scheme, base);
                assert_ne!(t.chart_down, t.chart_up, "{scheme:?} {base:?}");
            }
        }
    }
}
