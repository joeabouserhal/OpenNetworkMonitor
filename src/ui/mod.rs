//! The interface, in the style of shadcn/ui: zinc neutrals, a single border width,
//! restrained colour and Inter type. Everything here is drawn with egui's painter.
pub mod a11y;
pub mod button;
pub mod chart;
pub mod controls;
pub mod icon;
pub mod layout;
pub mod text;
pub mod tokens;

#[cfg(any(test, feature = "ui-preview"))]
pub mod gallery;
#[cfg(any(test, feature = "ui-preview"))]
pub mod scene;

use eframe::egui::{CornerRadius, Painter, Rect, Stroke, StrokeKind};
use tokens::Tokens;

/// The three-pixel translucent ring around a keyboard-focused control.
pub fn focus_ring(
    painter: &Painter,
    ctx: &eframe::egui::Context,
    rect: Rect,
    radius: CornerRadius,
    t: &Tokens,
) {
    let grown = CornerRadius::same(radius.nw.saturating_add(2));
    let _ = ctx;
    painter.rect_stroke(
        rect.expand(1.5),
        grown,
        Stroke::new(3.0, t.ring.gamma_multiply(0.5)),
        StrokeKind::Outside,
    );
}
