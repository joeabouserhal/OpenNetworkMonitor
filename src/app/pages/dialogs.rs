//! The custom period dialog and its calendar.
use super::*;
use crate::ui::layout::{dialog, tabs};
use chrono::{Datelike, Local, Months, NaiveDate};

const DIALOG_WIDTH: f32 = 440.0;

impl MonitorApp {
    pub(super) fn custom_period_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.custom_period.take() else {
            return;
        };
        let (outcome, should_close) = dialog(ctx, "custom-period-dialog", DIALOG_WIDTH, |ui| {
            period_contents(ui, &mut draft)
        });
        let (apply, cancel) = outcome;
        if apply {
            match draft.selection() {
                Ok((seconds, range)) => {
                    self.settings.filter.seconds = seconds;
                    self.settings.filter.time_range = range;
                    self.update_filter();
                    return;
                }
                Err(error) => draft.error = error.to_owned(),
            }
        }
        if !cancel && !(draft.presented && should_close) {
            draft.presented = true;
            self.custom_period = Some(draft);
        }
    }
}

/// Draws the dialog body and returns `(apply, cancel)`.
fn period_contents(ui: &mut egui::Ui, draft: &mut CustomPeriod) -> (bool, bool) {
    let t = tk(ui.ctx());
    ui.spacing_mut().item_spacing = Vec2::new(8.0, 12.0);
    let (title, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 24.0), Sense::hover());
    text::paint(
        ui.painter(),
        title,
        Align2::LEFT_CENTER,
        "Custom period",
        text::semibold(18.0),
        t.foreground,
    );
    let sub = ui.painter().layout(
        "Choose how much network usage to show. It applies to both traffic views.".to_owned(),
        text::regular(13.0),
        t.muted_foreground,
        ui.available_width(),
    );
    let (rect, _) = ui.allocate_exact_size(sub.size(), Sense::hover());
    ui.painter().galley(rect.min, sub, t.muted_foreground);
    if tabs(
        ui,
        "period-mode",
        &mut draft.mode,
        &[
            (PeriodMode::Rolling, "Rolling window"),
            (PeriodMode::Dates, "Date range"),
        ],
    ) {
        draft.error.clear();
    }
    if draft.mode == PeriodMode::Rolling {
        field_label(ui, "Amount");
        controls::input(ui, &mut draft.amount, "e.g. 30", DIALOG_WIDTH);
        field_label(ui, "Unit");
        tabs(
            ui,
            "period-unit",
            &mut draft.unit,
            &Unit::ALL.map(|unit| (unit, unit.label())),
        );
    } else {
        field_label(ui, "From");
        date_time_row(ui, "from", &mut draft.from);
        field_label(ui, "To");
        date_time_row(ui, "to", &mut draft.to);
    }
    let (hint, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
    if draft.error.is_empty() {
        let message = if draft.mode == PeriodMode::Rolling {
            "Moves forward with the current time."
        } else {
            "Local time. The full ending minute is included."
        };
        text::paint(
            ui.painter(),
            hint,
            Align2::LEFT_CENTER,
            message,
            text::regular(12.5),
            t.muted_foreground,
        );
    } else {
        text::paint(
            ui.painter(),
            hint,
            Align2::LEFT_CENTER,
            &draft.error,
            text::regular(13.0),
            t.destructive,
        );
    }
    let mut apply = false;
    let mut cancel = false;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        apply = Button::new("Apply").show(ui).clicked();
        cancel = Button::new("Cancel")
            .variant(Variant::Outline)
            .show(ui)
            .clicked();
    });
    apply |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    (apply, cancel)
}

fn field_label(ui: &mut egui::Ui, label: &str) {
    let t = tk(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    text::paint(
        ui.painter(),
        rect,
        Align2::LEFT_CENTER,
        label,
        text::medium(13.0),
        t.foreground,
    );
}

fn date_time_row(ui: &mut egui::Ui, id: &str, fields: &mut DateTimeFields) {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            controls::input(ui, &mut fields.date, "YYYY-MM-DD", 180.0);
            let calendar = Button::icon_only(Icon::Calendar, "Choose date")
                .variant(Variant::Outline)
                .show(ui);
            if calendar.clicked()
                && let Ok(date) = NaiveDate::parse_from_str(&fields.date, "%Y-%m-%d")
            {
                fields.month = date.with_day(1).unwrap();
            }
            egui::Popup::menu(&calendar)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .gap(4.0)
                .show(|ui| calendar_contents(ui, fields));
            controls::input(ui, &mut fields.time, "HH:MM", 96.0);
        });
    });
}

fn calendar_contents(ui: &mut egui::Ui, fields: &mut DateTimeFields) {
    let t = tk(ui.ctx());
    ui.set_width(7.0 * 36.0);
    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
    ui.horizontal(|ui| {
        let can_go_back = fields.month.year() > 1970 || fields.month.month() > 1;
        if Button::icon_only(Icon::ChevronLeft, "Previous month")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .enabled(can_go_back)
            .show(ui)
            .clicked()
            && let Some(month) = fields.month.checked_sub_months(Months::new(1))
        {
            fields.month = month;
        }
        let (title, _) = ui.allocate_exact_size(
            Vec2::new(7.0 * 36.0 - 2.0 * 34.0 - 8.0, 32.0),
            Sense::hover(),
        );
        text::paint(
            ui.painter(),
            title,
            Align2::CENTER_CENTER,
            &fields.month.format("%B %Y").to_string(),
            text::medium(14.0),
            t.foreground,
        );
        if Button::icon_only(Icon::ChevronRight, "Next month")
            .variant(Variant::Ghost)
            .size(Size::Sm)
            .show(ui)
            .clicked()
            && let Some(month) = fields.month.checked_add_months(Months::new(1))
        {
            fields.month = month;
        }
    });
    let selected = NaiveDate::parse_from_str(&fields.date, "%Y-%m-%d").ok();
    let today = Local::now().date_naive();
    let offset = fields.month.weekday().num_days_from_monday();
    egui::Grid::new("calendar-days")
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for day in ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(34.0, 24.0), Sense::hover());
                text::paint(
                    ui.painter(),
                    rect,
                    Align2::CENTER_CENTER,
                    day,
                    text::regular(12.0),
                    t.muted_foreground,
                );
            }
            ui.end_row();
            for cell in 0..42u32 {
                let date = cell
                    .checked_sub(offset)
                    .and_then(|day| fields.month.with_day(day + 1));
                if let Some(date) = date {
                    let is_selected = selected == Some(date);
                    let label = date.day().to_string();
                    let variant = if is_selected {
                        Variant::Default
                    } else if date == today {
                        Variant::Secondary
                    } else {
                        Variant::Ghost
                    };
                    if Button::new(&label)
                        .variant(variant)
                        .size(Size::Sm)
                        .width(34.0)
                        .show(ui)
                        .clicked()
                    {
                        fields.date = date.format("%Y-%m-%d").to_string();
                        ui.close();
                    }
                } else {
                    ui.allocate_space(Vec2::new(34.0, 32.0));
                }
                if cell % 7 == 6 {
                    ui.end_row();
                }
            }
        });
    ui.add_space(4.0);
    if Button::new("Today")
        .variant(Variant::Outline)
        .size(Size::Sm)
        .width(7.0 * 36.0 - 2.0)
        .show(ui)
        .clicked()
    {
        fields.date = today.format("%Y-%m-%d").to_string();
        fields.month = today.with_day(1).unwrap();
        ui.close();
    }
}
