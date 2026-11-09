//! Containers and composite controls: card, tabs, select and menus, table helpers, dialog.
use super::{
    icon::{self, Icon},
    text,
    tokens::{RADIUS, hairline, tk},
};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, CursorIcon, Layout, Margin, Pos2, Rect, Response,
    Sense, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType,
};

fn corner(r: f32) -> CornerRadius {
    CornerRadius::same(r.round().clamp(0.0, 255.0) as u8)
}

// --------------------------------------------------------------------------- card

/// A bordered surface, shadcn's `Card`. `padding` is the inside margin in points.
pub fn card<R>(ui: &mut Ui, padding: i8, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let t = tk(ui.ctx());
    egui::Frame::new()
        .fill(t.card)
        .stroke(hairline(ui.ctx(), t.border))
        .corner_radius(corner(12.0))
        .inner_margin(Margin::same(padding))
        .shadow(egui::Shadow {
            offset: [0, 1],
            blur: 2,
            spread: 0,
            color: Color32::from_black_alpha(if t.is_dark() { 0 } else { 12 }),
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
}

/// A card of exactly `height` points. What is inside can use `ui.available_height()` to fill
/// the rest, which is how lists and charts take the room a window gives them.
pub fn card_fixed<R>(ui: &mut Ui, padding: i8, height: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), height),
        Layout::top_down(Align::Min),
        |ui| {
            card(ui, padding, |ui| {
                // The frame's own outline takes a point on each side. Content spaces itself with
                // explicit gaps, so the height it needs is the same at every window size.
                ui.set_min_height(height - 2.0 * f32::from(padding) - 2.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                add(ui)
            })
            .inner
        },
    )
    .inner
}

/// A card title with an optional description beneath and actions on the right.
pub fn card_header(ui: &mut Ui, title: &str, description: &str, actions: impl FnOnce(&mut Ui)) {
    let t = tk(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 2.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (rect, response) =
                ui.allocate_exact_size(Vec2::new(ui.available_width() - 8.0, 20.0), Sense::hover());
            response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, title));
            text::paint(
                ui.painter(),
                rect,
                Align2::LEFT_CENTER,
                title,
                text::semibold(15.0),
                t.foreground,
            );
            if !description.is_empty() {
                let (rect, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width() - 8.0, 18.0),
                    Sense::hover(),
                );
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, description));
                text::paint(
                    ui.painter(),
                    rect,
                    Align2::LEFT_CENTER,
                    description,
                    text::regular(13.5),
                    t.muted_foreground,
                );
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
}

// -------------------------------------------------------------------------- tabs

/// shadcn's `Tabs` list: a muted track with the active trigger raised. Returns true
/// when the value changed.
pub fn tabs<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> bool {
    let t = tk(ui.ctx());
    let font = text::medium(13.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| text::text_width(ui.painter(), label, font.clone()) + 28.0)
        .collect();
    let total = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 36.0), Sense::hover());
    ui.painter().rect_filled(rect, corner(RADIUS), t.muted);
    let mut changed = false;
    let mut x = rect.left() + 3.0;
    for ((option, label), width) in options.iter().zip(widths) {
        let cell = Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), Vec2::new(width, 30.0));
        let response = ui.interact(cell, ui.make_persistent_id((id, label)), Sense::click());
        let active = *value == *option;
        response
            .widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, active, *label));
        let painter = ui.painter();
        if active {
            painter.rect_filled(
                cell,
                corner(RADIUS - 2.0),
                if t.is_dark() {
                    t.input.lerp_to_gamma(t.accent, 0.5)
                } else {
                    t.background
                },
            );
            if !t.is_dark() {
                painter.rect_stroke(
                    cell,
                    corner(RADIUS - 2.0),
                    hairline(ui.ctx(), t.border),
                    StrokeKind::Inside,
                );
            }
        }
        if response.has_focus() {
            super::focus_ring(painter, ui.ctx(), cell, corner(RADIUS - 2.0), &t);
        }
        text::paint(
            painter,
            cell,
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            if active {
                t.accent_ink
            } else if response.hovered() {
                t.foreground
            } else {
                t.muted_foreground
            },
        );
        if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !active {
            *value = *option;
            changed = true;
        }
        x += width;
    }
    changed
}

// ------------------------------------------------------------------------- select

/// A select trigger that opens `contents` as a popover. With a non-empty `prefix` it
/// reads `prefix  value`. Returns the trigger's response.
#[allow(clippy::too_many_arguments)]
pub fn select(
    ui: &mut Ui,
    id: &str,
    prefix: &str,
    value: &str,
    width: Option<f32>,
    close: egui::PopupCloseBehavior,
    menu_width: f32,
    contents: impl FnOnce(&mut Ui),
) -> Response {
    let t = tk(ui.ctx());
    let label_font = text::regular(13.5);
    let value_font = text::medium(13.5);
    let prefix_width = if prefix.is_empty() {
        0.0
    } else {
        text::text_width(ui.painter(), prefix, label_font.clone()) + 6.0
    };
    let value_width = text::text_width(ui.painter(), value, value_font.clone()).min(180.0);
    let natural = 12.0 + prefix_width + value_width + 8.0 + 16.0 + 10.0;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(width.unwrap_or(natural), 36.0), Sense::hover());
    let response = ui.interact(rect, ui.make_persistent_id(id), Sense::click());
    let label = format!("{prefix} {value}");
    response.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, label.clone()));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        corner(RADIUS),
        if response.hovered() || open {
            t.accent
        } else {
            t.background
        },
    );
    painter.rect_stroke(
        rect,
        corner(RADIUS),
        hairline(ui.ctx(), if open { t.ring } else { t.input }),
        StrokeKind::Inside,
    );
    if response.has_focus() {
        super::focus_ring(painter, ui.ctx(), rect, corner(RADIUS), &t);
    }
    let mut x = rect.left() + 12.0;
    if !prefix.is_empty() {
        text::paint(
            painter,
            Rect::from_min_size(
                Pos2::new(x, rect.top()),
                Vec2::new(prefix_width, rect.height()),
            ),
            Align2::LEFT_CENTER,
            prefix,
            label_font,
            t.muted_foreground,
        );
        x += prefix_width;
    }
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(x, rect.top()),
            Pos2::new(rect.right() - 30.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        value,
        value_font,
        t.foreground,
    );
    icon::paint(
        painter,
        Icon::ChevronsUpDown,
        Pos2::new(rect.right() - 17.0, rect.center().y),
        14.0,
        t.muted_foreground,
    );
    let response = response.on_hover_cursor(CursorIcon::PointingHand);
    egui::Popup::menu(&response)
        .close_behavior(close)
        .gap(4.0)
        .show(|ui| {
            ui.set_width(menu_width);
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

fn menu_hover(ui: &Ui, rect: Rect, response: &Response) {
    let t = tk(ui.ctx());
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, corner(6.0), t.accent);
    }
}

/// One choice in a menu, with a check at the right when `selected`.
pub fn menu_item(ui: &mut Ui, selected: bool, label: &str) -> Response {
    let t = tk(ui.ctx());
    let (rect, response) = menu_row(ui);
    response
        .widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, label));
    menu_hover(ui, rect, &response);
    let painter = ui.painter();
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(rect.left() + 8.0, rect.top()),
            Pos2::new(rect.right() - 30.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        text::regular(14.0),
        t.foreground,
    );
    if selected {
        icon::paint(
            painter,
            Icon::Check,
            Pos2::new(rect.right() - 16.0, rect.center().y),
            14.0,
            t.foreground,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A menu action, optionally with a leading icon and a shortcut hint.
pub fn menu_action(
    ui: &mut Ui,
    icon_kind: Option<Icon>,
    label: &str,
    shortcut: &str,
    destructive: bool,
) -> Response {
    let t = tk(ui.ctx());
    let (rect, response) = menu_row(ui);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    menu_hover(ui, rect, &response);
    let ink = if destructive {
        t.destructive
    } else {
        t.foreground
    };
    let painter = ui.painter();
    let mut left = rect.left() + 8.0;
    if let Some(glyph) = icon_kind {
        icon::paint(
            painter,
            glyph,
            Pos2::new(left + 8.0, rect.center().y),
            15.0,
            if destructive { ink } else { t.muted_foreground },
        );
        left += 26.0;
    }
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(left, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        text::regular(14.0),
        ink,
    );
    if !shortcut.is_empty() {
        text::paint(
            painter,
            Rect::from_min_max(rect.min, Pos2::new(rect.right() - 8.0, rect.bottom())),
            Align2::RIGHT_CENTER,
            shortcut,
            text::regular(12.0),
            t.muted_foreground,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A multi-choice row with a checkbox on the left. Returns true when toggled.
pub fn menu_check(ui: &mut Ui, checked: &mut bool, label: &str) -> bool {
    let t = tk(ui.ctx());
    let (rect, response) = menu_row(ui);
    response.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, *checked, label));
    menu_hover(ui, rect, &response);
    let toggled = response.clicked();
    if toggled {
        *checked = !*checked;
    }
    let painter = ui.painter();
    let boxed = Rect::from_center_size(
        Pos2::new(rect.left() + 18.0, rect.center().y),
        Vec2::splat(16.0),
    );
    if *checked {
        painter.rect_filled(boxed, corner(4.0), t.primary);
        icon::paint(
            painter,
            Icon::Check,
            boxed.center(),
            12.0,
            t.primary_foreground,
        );
    } else {
        painter.rect_stroke(
            boxed,
            corner(4.0),
            hairline(ui.ctx(), t.input.gamma_multiply(1.4)),
            StrokeKind::Inside,
        );
    }
    text::paint(
        painter,
        Rect::from_min_max(
            Pos2::new(rect.left() + 34.0, rect.top()),
            Pos2::new(rect.right() - 8.0, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
        label,
        text::regular(14.0),
        t.foreground,
    );
    response.on_hover_cursor(CursorIcon::PointingHand);
    toggled
}

/// A small muted heading inside a menu.
pub fn menu_label(ui: &mut Ui, label: &str) {
    let t = tk(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 26.0), Sense::hover());
    text::paint(
        ui.painter(),
        Rect::from_min_max(Pos2::new(rect.left() + 8.0, rect.top()), rect.max),
        Align2::LEFT_CENTER,
        label,
        text::medium(12.5),
        t.muted_foreground,
    );
}

pub fn menu_separator(ui: &mut Ui) {
    ui.add_space(2.0);
    super::controls::separator(ui);
    ui.add_space(2.0);
}

// -------------------------------------------------------------------------- table

/// How a table column takes width.
#[derive(Clone, Copy, Debug)]
pub enum Col {
    Fixed(f32),
    /// A share of what is left after the fixed columns.
    Flex(f32),
}

/// The cell rectangles of one row, splitting `rect` by `cols` with `gap` between them
/// and `inset` on both sides.
pub fn cells(rect: Rect, cols: &[Col], gap: f32, inset: f32) -> Vec<Rect> {
    let fixed: f32 = cols
        .iter()
        .map(|c| if let Col::Fixed(w) = c { *w } else { 0.0 })
        .sum();
    let flex: f32 = cols
        .iter()
        .map(|c| if let Col::Flex(w) = c { *w } else { 0.0 })
        .sum();
    let free =
        (rect.width() - inset * 2.0 - fixed - gap * cols.len().saturating_sub(1) as f32).max(0.0);
    let mut x = rect.left() + inset;
    cols.iter()
        .map(|col| {
            let width = match col {
                Col::Fixed(w) => *w,
                Col::Flex(w) => free * w / flex.max(f32::EPSILON),
            };
            let cell =
                Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(width, rect.height()));
            x += width + gap;
            cell
        })
        .collect()
}

pub const TABLE_HEADER_HEIGHT: f32 = 40.0;

/// A header cell. `sort` is `Some(ascending)` when the table is sorted by this column.
pub fn header_cell(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    label: &str,
    align: Align2,
    sortable: bool,
    sort: Option<bool>,
) -> Response {
    let t = tk(ui.ctx());
    let response = if sortable {
        ui.interact(rect, ui.make_persistent_id(("th", id)), Sense::click())
    } else {
        ui.interact(rect, ui.make_persistent_id(("th", id)), Sense::hover())
    };
    if sortable {
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("Sort by {label}"))
        });
    }
    let ink = if sort.is_some() || response.hovered() {
        t.foreground
    } else {
        t.muted_foreground
    };
    let font = text::medium(13.0);
    let painter = ui.painter();
    let arrow = sort.is_some();
    let reserve = if arrow { 20.0 } else { 0.0 };
    let text_rect = Rect::from_min_max(rect.min, Pos2::new(rect.right() - reserve, rect.bottom()));
    let placed = text::paint(painter, text_rect, align, label, font, ink);
    if let Some(ascending) = sort {
        icon::paint(
            painter,
            if ascending {
                Icon::ArrowUp
            } else {
                Icon::ArrowDown
            },
            Pos2::new(placed.right() + 10.0, rect.center().y),
            13.0,
            ink,
        );
    }
    if sortable {
        response.on_hover_cursor(CursorIcon::PointingHand)
    } else {
        response
    }
}

/// The bottom border of a table row.
pub fn row_border(ui: &Ui, rect: Rect) {
    let t = tk(ui.ctx());
    ui.painter()
        .hline(rect.x_range(), rect.bottom(), hairline(ui.ctx(), t.border));
}

/// Fills a hovered row.
pub fn row_hover(ui: &Ui, rect: Rect, response: &Response, selected: bool) {
    let t = tk(ui.ctx());
    if selected {
        ui.painter().rect_filled(rect, 0.0, t.accent);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, t.accent.gamma_multiply(0.5));
    }
}

/// Centred muted text for an empty table.
pub fn empty_state(ui: &mut Ui, message: &str) {
    let t = tk(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 96.0), Sense::hover());
    text::paint(
        ui.painter(),
        rect,
        Align2::CENTER_CENTER,
        message,
        text::regular(14.0),
        t.muted_foreground,
    );
}

// ------------------------------------------------------------------------- dialog

/// A modal dialog with a dimmed backdrop. Returns whether it should close (Escape or a
/// click outside) along with whatever `add` returned.
pub fn dialog<R>(
    ctx: &egui::Context,
    id: &str,
    width: f32,
    add: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let t = tk(ctx);
    let response = egui::Modal::new(egui::Id::new(id))
        .backdrop_color(Color32::from_black_alpha(if t.is_dark() {
            150
        } else {
            90
        }))
        .area(egui::Modal::default_area(egui::Id::new(id)).fade_in(false))
        .frame(
            egui::Frame::new()
                .fill(t.popover)
                .stroke(hairline(ctx, t.border))
                .corner_radius(corner(12.0))
                .inner_margin(Margin::same(24))
                .shadow(egui::Shadow {
                    offset: [0, 8],
                    blur: 24,
                    spread: 0,
                    color: Color32::from_black_alpha(if t.is_dark() { 140 } else { 50 }),
                }),
        )
        .show(ctx, |ui| {
            ui.set_width(width);
            add(ui)
        });
    let close = response.should_close();
    (response.inner, close)
}
