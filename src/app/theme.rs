use crate::widgets::Palette;
use eframe::egui::{self, Color32};
use serde::{Deserialize, Serialize};

/// Variant names are persisted in user preferences; keep them stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(super) enum Theme {
    #[default]
    Dark,
    Light,
    Nord,
    Mocha,
}
impl Theme {
    pub(super) const ALL: [Self; 4] = [Self::Dark, Self::Light, Self::Nord, Self::Mocha];
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Dark => "Slate",
            Self::Light => "Light",
            Self::Nord => "Nord",
            Self::Mocha => "Catppuccin Mocha",
        }
    }
    /// The theme to show: with "match Windows" on, Light or Slate to follow the system's
    /// choice, otherwise the one picked. An unknown system theme keeps the pick.
    pub(super) fn resolve(follow_system: bool, system: Option<egui::Theme>, chosen: Self) -> Self {
        match (follow_system, system) {
            (true, Some(egui::Theme::Light)) => Self::Light,
            (true, Some(egui::Theme::Dark)) => Self::Dark,
            _ => chosen,
        }
    }
    pub(super) fn apply(self, ctx: &egui::Context) {
        self.palette().install(ctx);
    }
    pub(super) fn palette(self) -> Palette {
        let rgb = |r, g, b| Color32::from_rgb(r, g, b);
        let hairline = |dark: bool| {
            if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 15)
            } else {
                Color32::from_rgba_unmultiplied(15, 23, 42, 22)
            }
        };
        let soft = |card: Color32, accent: Color32, amount: f32| card.lerp_to_gamma(accent, amount);
        match self {
            Self::Dark => Palette {
                dark: true,
                canvas: rgb(11, 12, 14),
                card: rgb(21, 23, 26),
                raised: rgb(32, 35, 40),
                sunken: rgb(15, 16, 18),
                hairline: hairline(true),
                text: rgb(236, 238, 240),
                dim: rgb(154, 160, 168),
                accent: rgb(96, 165, 250),
                accent_ink: rgb(8, 14, 24),
                accent_soft: soft(rgb(21, 23, 26), rgb(96, 165, 250), 0.18),
                down: rgb(96, 165, 250),
                up: rgb(255, 176, 84),
                good: rgb(74, 222, 128),
                warn: rgb(245, 176, 65),
                bad: rgb(248, 113, 113),
            },
            Self::Light => Palette {
                dark: false,
                canvas: rgb(244, 245, 247),
                card: rgb(255, 255, 255),
                raised: rgb(240, 241, 244),
                sunken: rgb(238, 240, 243),
                hairline: hairline(false),
                text: rgb(20, 22, 26),
                dim: rgb(91, 98, 112),
                accent: rgb(30, 86, 210),
                accent_ink: rgb(255, 255, 255),
                accent_soft: soft(rgb(255, 255, 255), rgb(30, 86, 210), 0.09),
                down: rgb(30, 86, 210),
                up: rgb(150, 66, 4),
                good: rgb(16, 112, 52),
                warn: rgb(140, 80, 4),
                bad: rgb(190, 28, 28),
            },
            Self::Nord => Palette {
                dark: true,
                canvas: rgb(36, 41, 51),
                card: rgb(46, 52, 64),
                raised: rgb(59, 66, 82),
                sunken: rgb(36, 41, 51),
                hairline: hairline(true),
                text: rgb(236, 239, 244),
                dim: rgb(180, 191, 208),
                accent: rgb(136, 192, 208),
                accent_ink: rgb(24, 30, 40),
                accent_soft: soft(rgb(46, 52, 64), rgb(136, 192, 208), 0.12),
                down: rgb(136, 192, 208),
                up: rgb(232, 170, 125),
                good: rgb(163, 190, 140),
                warn: rgb(235, 203, 139),
                bad: rgb(242, 160, 168),
            },
            Self::Mocha => Palette {
                dark: true,
                canvas: rgb(17, 17, 27),
                card: rgb(30, 30, 46),
                raised: rgb(49, 50, 68),
                sunken: rgb(17, 17, 27),
                hairline: hairline(true),
                text: rgb(205, 214, 244),
                dim: rgb(166, 173, 200),
                accent: rgb(203, 166, 247),
                accent_ink: rgb(26, 18, 40),
                accent_soft: soft(rgb(30, 30, 46), rgb(203, 166, 247), 0.18),
                down: rgb(137, 180, 250),
                up: rgb(250, 179, 135),
                good: rgb(166, 227, 161),
                warn: rgb(249, 226, 175),
                bad: rgb(243, 139, 168),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn following_windows_picks_slate_or_light_and_otherwise_keeps_the_choice() {
        let dark = Some(egui::Theme::Dark);
        let light = Some(egui::Theme::Light);
        assert_eq!(Theme::resolve(true, dark, Theme::Nord), Theme::Dark);
        assert_eq!(Theme::resolve(true, light, Theme::Mocha), Theme::Light);
        assert_eq!(Theme::resolve(true, None, Theme::Nord), Theme::Nord);
        assert_eq!(Theme::resolve(false, light, Theme::Nord), Theme::Nord);
        assert_eq!(Theme::resolve(false, dark, Theme::Light), Theme::Light);
    }
}
