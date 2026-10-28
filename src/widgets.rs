use eframe::egui::{self, Color32, FontId, RichText, Stroke, Vec2};

pub const DEFAULT_WINDOW_SIZE: [f32; 2] = [1440.0, 820.0];
pub const MIN_WINDOW_SIZE: [f32; 2] = [960.0, 680.0];
pub const NAV_WIDTH: f32 = 180.0;
pub const PAGE_MARGIN: i8 = 18;
pub const ROW_HEIGHT: f32 = 56.0;

pub fn border(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(58, 63, 70)
    } else {
        Color32::from_rgb(213, 218, 224)
    }
}
pub fn surface(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(Stroke::new(1.0, border(ui)))
        .corner_radius(8)
        .inner_margin(12)
}
pub fn text_at(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    text: impl Into<egui::WidgetText>,
    align: egui::Align,
) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .sense(egui::Sense::empty())
            .layout(egui::Layout::top_down(align)),
    );
    child.add(
        egui::Label::new(text)
            .truncate()
            .selectable(false)
            .sense(egui::Sense::empty()),
    );
}
pub fn badge(ui: &mut egui::Ui, label: &str, color: Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(if ui.visuals().dark_mode { 0.13 } else { 0.09 }))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(label).size(12.5).color(color)).selectable(false),
            );
        });
}
pub fn row_hover(ui: &egui::Ui) -> Color32 {
    let c = ui.visuals().window_fill;
    if ui.visuals().dark_mode {
        Color32::from_rgb(
            c.r().saturating_add(3),
            c.g().saturating_add(3),
            c.b().saturating_add(3),
        )
    } else {
        Color32::from_rgb(
            c.r().saturating_sub(7),
            c.g().saturating_sub(7),
            c.b().saturating_sub(7),
        )
    }
}
pub fn section_title(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.label(RichText::new(title).size(17.0).strong());
    if !description.is_empty() {
        ui.add(egui::Label::new(RichText::new(description).size(13.5).weak()).wrap());
    }
    ui.add_space(4.0);
}
pub fn navigation(ui: &mut egui::Ui, title: &str, kind: usize, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            title,
        )
    });
    if selected || response.hovered() {
        ui.painter().rect_filled(
            rect,
            6,
            if selected {
                ui.visuals().selection.bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            },
        );
    }
    let ink = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let p = ui.painter();
    if response.has_focus() {
        p.rect_stroke(
            rect.shrink(1.0),
            6,
            Stroke::new(1.0, ui.visuals().selection.stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    if selected {
        p.vline(
            rect.left() + 1.5,
            rect.top() + 10.0..=rect.bottom() - 10.0,
            Stroke::new(3.0, ink),
        );
    }
    let c = rect.left_center() + Vec2::new(23.0, 0.0);
    let stroke = Stroke::new(1.5, ink);
    match kind {
        0 => {
            for (x, h) in [(-6.0, 5.0), (0.0, 12.0), (6.0, 8.0)] {
                p.line_segment([c + Vec2::new(x, 7.0), c + Vec2::new(x, 7.0 - h)], stroke);
            }
        }
        1 => {
            for (x, y) in [(-6.0, -5.0), (6.0, 5.0)] {
                p.circle_stroke(c + Vec2::new(x, y), 3.0, stroke);
            }
            p.line_segment([c + Vec2::new(-4.0, -3.0), c + Vec2::new(4.0, 3.0)], stroke);
        }
        2 => {
            p.rect_stroke(
                egui::Rect::from_center_size(c, Vec2::new(16.0, 11.0)),
                2,
                stroke,
                egui::StrokeKind::Inside,
            );
            p.line_segment([c + Vec2::new(0.0, 6.0), c + Vec2::new(0.0, 9.0)], stroke);
            p.hline(c.x - 5.0..=c.x + 5.0, c.y + 9.0, stroke);
        }
        _ => {
            for (y, x) in [(-6.0, -3.0), (0.0, 4.0), (6.0, -1.0)] {
                p.hline(c.x - 8.0..=c.x + 8.0, c.y + y, stroke);
                p.circle_filled(c + Vec2::new(x, y), 2.5, ink);
            }
        }
    }
    p.text(
        rect.left_center() + Vec2::new(42.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        FontId::proportional(15.0),
        ink,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Compact, wrapping status message. Returns true when Dismiss was selected.
pub fn message(ui: &mut egui::Ui, text: &str, warning: bool, dismissible: bool) -> bool {
    let color = if warning {
        if ui.visuals().dark_mode {
            Color32::from_rgb(225, 177, 100)
        } else {
            Color32::from_rgb(145, 93, 15)
        }
    } else {
        ui.visuals().selection.stroke.color
    };
    let mut dismissed = false;
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(Stroke::new(1.0, color))
        .corner_radius(6)
        .inner_margin(9)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let width = (ui.available_width() - if dismissible { 82.0 } else { 0.0 }).max(50.0);
                ui.add_sized(
                    [width, 0.0],
                    egui::Label::new(RichText::new(text).size(14.0)).wrap(),
                );
                if dismissible {
                    dismissed = ui.small_button("Dismiss").clicked();
                }
            });
        });
    dismissed
}

/// Integer column origins keep panel borders aligned at fractional desktop scaling.
pub fn three_columns<R>(ui: &mut egui::Ui, contents: impl FnOnce(&mut [egui::Ui; 3]) -> R) -> R {
    let bounds = ui.available_rect_before_wrap();
    let gap = ui.spacing().item_spacing.x;
    let column = ((bounds.width() - 2.0 * gap) / 3.0).floor();
    let mut cols = std::array::from_fn(|i| {
        let left = bounds.left() + i as f32 * (column + gap);
        let right = if i == 2 {
            bounds.right()
        } else {
            left + column
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(egui::Rect::from_min_max(
                    egui::pos2(left, bounds.top()),
                    egui::pos2(right, bounds.bottom()),
                ))
                .layout(egui::Layout::top_down_justified(egui::Align::Min)),
        );
        child.set_width(right - left);
        child
    });
    let out = contents(&mut cols);
    let height = cols
        .iter()
        .map(|col| col.min_size().y)
        .fold(0.0f32, f32::max);
    ui.advance_cursor_after_rect(egui::Rect::from_min_size(
        bounds.min,
        Vec2::new(bounds.width(), height),
    ));
    out
}
