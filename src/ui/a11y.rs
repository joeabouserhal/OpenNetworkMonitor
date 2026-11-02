//! Labels for text that is painted straight onto the canvas, which screen readers
//! otherwise cannot see.
use eframe::egui::{Id, Rect, Sense, Ui, WidgetInfo, WidgetType};

/// Registers a labelled node over `rect`. It only listens for hovering, so it never takes a
/// click from the widget beneath it.
pub fn label(ui: &Ui, rect: Rect, text: &str) {
    let id = Id::new((
        "onm-described",
        ui.id(),
        text,
        rect.min.x.round() as i32,
        rect.min.y.round() as i32,
    ));
    ui.interact(rect, id, Sense::hover())
        .widget_info(|| WidgetInfo::labeled(WidgetType::Label, ui.is_enabled(), text));
}
