use super::*;
use chrono::{Datelike, Local, Months, NaiveDate, NaiveDateTime, TimeZone};

pub(super) const PERIODS: &[(i64, &str, &str)] = &[
    (300, "5m", "5 minutes"),
    (900, "15m", "15 minutes"),
    (1800, "30m", "30 minutes"),
    (3600, "1h", "1 hour"),
    (10800, "3h", "3 hours"),
    (21600, "6h", "6 hours"),
    (43200, "12h", "12 hours"),
    (86400, "24h", "24 hours"),
    (259200, "3d", "3 days"),
    (604800, "7d", "7 days"),
    (2592000, "30d", "30 days"),
    (7776000, "90d", "90 days"),
    (31536000, "1y", "1 year"),
    (0, "All time", "All time"),
];

pub(super) fn period_caption(filter: &Filter) -> &'static str {
    if filter.time_range.is_some() {
        "Custom"
    } else if filter.month_to_date() {
        "Month"
    } else {
        PERIODS
            .iter()
            .find(|p| p.0 == filter.seconds)
            .map_or("Custom", |p| p.1)
    }
}

#[derive(Clone, Copy, PartialEq)]
enum PeriodMode {
    Rolling,
    Dates,
}
#[derive(Clone, Copy, PartialEq)]
enum Unit {
    Minutes,
    Hours,
    Days,
    Weeks,
}
impl Unit {
    const ALL: [Self; 4] = [Self::Minutes, Self::Hours, Self::Days, Self::Weeks];
    fn seconds(self) -> i64 {
        match self {
            Self::Minutes => 60,
            Self::Hours => 3600,
            Self::Days => 86400,
            Self::Weeks => 604800,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Minutes => "minutes",
            Self::Hours => "hours",
            Self::Days => "days",
            Self::Weeks => "weeks",
        }
    }
}
const MODAL_WIDTH: f32 = 460.0;
const MODAL_BODY_HEIGHT: f32 = 184.0;

pub(super) struct CustomPeriod {
    mode: PeriodMode,
    amount: String,
    unit: Unit,
    from: DateTimeFields,
    to: DateTimeFields,
    error: String,
    presented: bool,
}
impl CustomPeriod {
    pub(super) fn from_filter(filter: &Filter) -> Self {
        let now = chrono::Utc::now().timestamp();
        let (start, end) = if filter.all_time() {
            (now - 86400, now)
        } else {
            filter.time_bounds(now)
        };
        let duration = if filter.all_time() {
            86400
        } else if filter.month_to_date() {
            end.saturating_sub(start).max(60)
        } else {
            filter.seconds.max(60)
        };
        let unit = Unit::ALL
            .into_iter()
            .rev()
            .find(|u| duration % u.seconds() == 0)
            .unwrap_or(Unit::Minutes);
        Self {
            mode: if filter.time_range.is_some() || filter.month_to_date() {
                PeriodMode::Dates
            } else {
                PeriodMode::Rolling
            },
            amount: u32::try_from(duration / unit.seconds())
                .unwrap_or(u32::MAX)
                .max(1)
                .to_string(),
            unit,
            from: DateTimeFields::from_timestamp(start),
            to: DateTimeFields::from_timestamp(end),
            error: String::new(),
            presented: false,
        }
    }
    fn selection(&self) -> std::result::Result<(i64, Option<TimeRange>), &'static str> {
        if self.mode == PeriodMode::Rolling {
            let amount = self
                .amount
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|amount| *amount > 0)
                .ok_or("Enter a whole number greater than zero.")?;
            return Ok((i64::from(amount) * self.unit.seconds(), None));
        }
        let start = self.from.timestamp()?;
        let end = self.to.timestamp()?.saturating_add(59);
        if start < 0 {
            return Err("Choose a start date in 1970 or later.");
        }
        if start > end {
            return Err("The end must be on or after the start.");
        }
        Ok((end - start + 1, Some(TimeRange { start, end })))
    }
}
#[derive(Clone)]
struct DateTimeFields {
    date: String,
    time: String,
    month: NaiveDate,
}
impl DateTimeFields {
    fn from_timestamp(timestamp: i64) -> Self {
        let date = Local
            .timestamp_opt(timestamp, 0)
            .single()
            .unwrap_or_else(Local::now);
        Self {
            date: date.format("%Y-%m-%d").to_string(),
            time: date.format("%H:%M").to_string(),
            month: date.date_naive().with_day(1).unwrap(),
        }
    }
    fn timestamp(&self) -> std::result::Result<i64, &'static str> {
        let date = NaiveDateTime::parse_from_str(
            &format!("{} {}", self.date.trim(), self.time.trim()),
            "%Y-%m-%d %H:%M",
        )
        .map_err(|_| "Enter dates as YYYY-MM-DD and times as HH:MM.")?;
        Local
            .from_local_datetime(&date)
            .single()
            .map(|d| d.timestamp())
            .ok_or(
                "This local time is ambiguous or skipped by a clock change. Choose another time.",
            )
    }
}
impl CustomPeriod {
    fn contents(&mut self, ui: &mut egui::Ui) -> (bool, bool) {
        let p = widgets::pal(ui);
        let mut cancel = false;
        ui.set_width(MODAL_WIDTH);
        ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
        ui.horizontal(|ui| {
            ui.label(widgets::strong_text("Custom period", 22.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel |= widgets::icon_button(ui, widgets::Icon::Close, "Close").clicked();
            });
        });
        ui.label(
            RichText::new("Choose how much network usage to show.")
                .size(13.5)
                .weak(),
        );
        ui.add_space(4.0);
        if widgets::segmented(
            ui,
            "period-mode",
            &mut self.mode,
            &[
                (PeriodMode::Rolling, "Rolling window"),
                (PeriodMode::Dates, "Date range"),
            ],
        ) {
            self.error.clear();
        }
        ui.add_space(6.0);
        ui.allocate_ui_with_layout(
            Vec2::new(MODAL_WIDTH, MODAL_BODY_HEIGHT),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(Vec2::new(MODAL_WIDTH, MODAL_BODY_HEIGHT));
                self.fields(ui);
            },
        );
        widgets::divider(ui);
        ui.add_space(2.0);
        let mut apply = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Applies to both traffic views")
                    .size(12.5)
                    .color(p.dim),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                apply = widgets::button_with(
                    ui,
                    widgets::ButtonKind::Primary,
                    None,
                    "Apply",
                    Some(96.0),
                )
                .clicked();
                cancel |= widgets::button_with(
                    ui,
                    widgets::ButtonKind::Ghost,
                    None,
                    "Cancel",
                    Some(80.0),
                )
                .clicked();
            });
        });
        // Enter applies and Escape closes, like any dialog.
        apply |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        cancel |= ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        (apply, cancel)
    }
}
impl CustomPeriod {
    fn fields(&mut self, ui: &mut egui::Ui) {
        if self.mode == PeriodMode::Rolling {
            field_header(ui, "AMOUNT");
            // Frame width = text + 24pt margin + two outlines; keep the dialog width exact.
            let inner = MODAL_WIDTH - 24.0 - 2.0 * widgets::hairline(ui).width;
            input_frame(ui).show(ui, |ui| {
                ui.set_width(inner);
                ui.add(
                    egui::TextEdit::singleline(&mut self.amount)
                        .font(widgets::mono(14.0))
                        .hint_text("e.g. 30")
                        .char_limit(10)
                        .desired_width(inner)
                        .frame(egui::Frame::NONE),
                );
            });
            field_header(ui, "UNIT");
            widgets::segmented(
                ui,
                "period-unit",
                &mut self.unit,
                &Unit::ALL.map(|unit| (unit, unit.label())),
            );
        } else {
            field_header(ui, "DATE AND TIME");
            let _ = date_time_row(ui, "From", &mut self.from);
            let _ = date_time_row(ui, "To", &mut self.to);
        }
        ui.allocate_ui_with_layout(
            Vec2::new(MODAL_WIDTH, 20.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(Vec2::new(MODAL_WIDTH, 20.0));
                if self.error.is_empty() {
                    let hint = if self.mode == PeriodMode::Rolling {
                        "Moves forward with the current time."
                    } else {
                        "Local time · the full ending minute is included."
                    };
                    ui.label(RichText::new(hint).size(12.5).weak());
                } else {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&self.error)
                                .size(13.0)
                                .color(ui.visuals().error_fg_color),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&self.error);
                }
            },
        );
    }
}
pub(super) fn input_frame(ui: &egui::Ui) -> egui::Frame {
    let p = widgets::pal(ui);
    egui::Frame::new()
        .fill(p.sunken)
        .stroke(widgets::hairline(ui))
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(12, 8))
}
fn row_label(ui: &mut egui::Ui, label: &str) {
    ui.allocate_ui_with_layout(
        Vec2::new(44.0, 40.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(Vec2::new(44.0, 40.0));
            ui.label(widgets::strong_text(label, 13.5));
        },
    );
}
fn field_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(
        RichText::new(text)
            .font(widgets::bold(11.5))
            .color(widgets::pal(ui).dim),
    );
}

fn date_time_row(ui: &mut egui::Ui, label: &str, fields: &mut DateTimeFields) -> egui::Response {
    ui.push_id(label, |ui| {
        ui.horizontal(|ui| {
            ui.set_min_height(40.0);
            row_label(ui, label);
            // 44pt label, 24pt of margins and two outlines per frame, 82pt time field, 20pt spacing.
            let inner = MODAL_WIDTH - 194.0 - 4.0 * widgets::hairline(ui).width;
            let calendar = input_frame(ui)
                .show(ui, |ui| {
                    ui.set_width(inner);
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut fields.date)
                                .font(widgets::mono(14.0))
                                .hint_text("YYYY-MM-DD")
                                .char_limit(10)
                                .desired_width(inner - 28.0)
                                .frame(egui::Frame::NONE),
                        );
                        calendar_button(ui)
                    })
                    .inner
                })
                .inner;
            if calendar.clicked()
                && let Ok(date) = NaiveDate::parse_from_str(&fields.date, "%Y-%m-%d")
            {
                fields.month = date.with_day(1).unwrap();
            }
            egui::Popup::menu(&calendar)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .gap(6.0)
                .show(|ui| calendar_contents(ui, fields));
            input_frame(ui).show(ui, |ui| {
                ui.set_width(82.0);
                ui.add(
                    egui::TextEdit::singleline(&mut fields.time)
                        .font(widgets::mono(14.0))
                        .hint_text("HH:MM")
                        .char_limit(5)
                        .desired_width(82.0)
                        .frame(egui::Frame::NONE),
                );
            });
            calendar
        })
        .inner
    })
    .inner
}
fn calendar_button(ui: &mut egui::Ui) -> egui::Response {
    let p = widgets::pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(24.0, 22.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), "Choose date")
    });
    let stroke = widgets::snapped(
        ui.ctx(),
        1.4,
        if response.hovered() || response.has_focus() {
            p.accent
        } else {
            p.dim
        },
    );
    let icon = egui::Rect::from_center_size(rect.center(), Vec2::new(15.0, 14.0));
    ui.painter()
        .rect_stroke(icon, 3, stroke, egui::StrokeKind::Inside);
    ui.painter().hline(icon.x_range(), icon.top() + 4.5, stroke);
    for x in [icon.left() + 4.5, icon.right() - 4.5] {
        ui.painter()
            .vline(x, icon.top() - 2.0..=icon.top() + 2.0, stroke);
    }
    response
        .on_hover_text("Choose date")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn day_cell(ui: &mut egui::Ui, label: &str, selected: bool, today: bool) -> egui::Response {
    let p = widgets::pal(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(32.0, 30.0), egui::Sense::click());
    let body = rect.shrink(1.0);
    let painter = ui.painter();
    if selected {
        painter.rect_filled(body, widgets::radius(8.0), p.accent);
    } else if response.hovered() {
        painter.rect_filled(body, widgets::radius(8.0), p.raised);
    }
    if today && !selected {
        painter.rect_stroke(
            body,
            widgets::radius(8.0),
            widgets::px_stroke(ui.ctx(), 1.0, p.accent.gamma_multiply(0.7)),
            egui::StrokeKind::Inside,
        );
    }
    widgets::paint_text(
        painter,
        rect,
        egui::Align2::CENTER_CENTER,
        label,
        if selected {
            widgets::bold(13.5)
        } else {
            widgets::sans(13.5)
        },
        if selected { p.accent_ink } else { p.text },
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
fn calendar_contents(ui: &mut egui::Ui, fields: &mut DateTimeFields) {
    let p = widgets::pal(ui);
    ui.set_width(238.0 + 22.0);
    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_enabled_ui(
            fields.month.year() > 1970 || fields.month.month() > 1,
            |ui| {
                if widgets::button_with(ui, widgets::ButtonKind::Ghost, None, "‹", Some(32.0))
                    .clicked()
                    && let Some(month) = fields.month.checked_sub_months(Months::new(1))
                {
                    fields.month = month;
                }
            },
        );
        let (title, _) = ui.allocate_exact_size(Vec2::new(150.0, 32.0), egui::Sense::hover());
        widgets::paint_text(
            ui.painter(),
            title,
            egui::Align2::CENTER_CENTER,
            &fields.month.format("%B %Y").to_string(),
            widgets::bold(14.0),
            p.text,
        );
        if widgets::button_with(ui, widgets::ButtonKind::Ghost, None, "›", Some(32.0)).clicked()
            && let Some(month) = fields.month.checked_add_months(Months::new(1))
        {
            fields.month = month;
        }
    });
    ui.add_space(4.0);
    let selected = NaiveDate::parse_from_str(&fields.date, "%Y-%m-%d").ok();
    let today = Local::now().date_naive();
    let offset = fields.month.weekday().num_days_from_monday();
    egui::Grid::new("calendar-days")
        .spacing([2.0, 2.0])
        .show(ui, |ui| {
            for day in ["M", "T", "W", "T", "F", "S", "S"] {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 22.0), egui::Sense::hover());
                widgets::paint_text(
                    ui.painter(),
                    rect,
                    egui::Align2::CENTER_CENTER,
                    day,
                    widgets::bold(11.5),
                    p.dim,
                );
            }
            ui.end_row();
            for cell in 0..42u32 {
                let date = cell
                    .checked_sub(offset)
                    .and_then(|day| fields.month.with_day(day + 1));
                if let Some(date) = date {
                    if day_cell(
                        ui,
                        &date.day().to_string(),
                        selected == Some(date),
                        date == today,
                    )
                    .clicked()
                    {
                        fields.date = date.format("%Y-%m-%d").to_string();
                        ui.close();
                    }
                } else {
                    ui.allocate_space(Vec2::new(32.0, 30.0));
                }
                if cell % 7 == 6 {
                    ui.end_row();
                }
            }
        });
    ui.add_space(6.0);
    widgets::divider(ui);
    ui.add_space(4.0);
    if widgets::button(ui, widgets::ButtonKind::Ghost, "Today").clicked() {
        fields.date = today.format("%Y-%m-%d").to_string();
        fields.month = today.with_day(1).unwrap();
        ui.close();
    }
}
impl MonitorApp {
    pub(super) fn custom_period_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.custom_period.take() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("custom-period-dialog"))
            .area(egui::Modal::default_area(egui::Id::new("custom-period-dialog")).fade_in(false))
            .frame(
                egui::Frame::window(&ctx.global_style())
                    .inner_margin(24)
                    .corner_radius(18),
            )
            .show(ctx, |ui| draft.contents(ui));
        let (apply, cancel) = response.inner;
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
        if !cancel && !(draft.presented && response.should_close()) {
            draft.presented = true;
            self.custom_period = Some(draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_period_modes_and_validation_keep_the_same_modal_size() {
        for theme in [Theme::Dark, Theme::Light, Theme::Nord, Theme::Mocha] {
            for scale in [0.8, 1.1] {
                let ctx = egui::Context::default();
                theme.apply(&ctx);
                install_fonts(&ctx);
                ctx.set_zoom_factor(scale);
                let mut draft = CustomPeriod::from_filter(&Filter::default());
                let mut expected_size = None;
                for mode in [PeriodMode::Rolling, PeriodMode::Dates] {
                    draft.mode = mode;
                    for error in [
                        "",
                        "This local time is ambiguous or skipped by a clock change. Choose another time.",
                    ] {
                        draft.error = error.into();
                        let mut bounds = egui::Rect::NOTHING;
                        for _ in 0..2 {
                            let mut output = ctx.run_ui(
                                egui::RawInput {
                                    screen_rect: Some(egui::Rect::from_min_size(
                                        egui::Pos2::ZERO,
                                        Vec2::new(960.0, 680.0) / scale,
                                    )),
                                    ..Default::default()
                                },
                                |_root| {
                                    egui::Modal::new(egui::Id::new("period-size-test"))
                                        .area(
                                            egui::Modal::default_area(egui::Id::new(
                                                "period-size-test",
                                            ))
                                            .fade_in(false),
                                        )
                                        .frame(
                                            egui::Frame::window(&ctx.global_style())
                                                .inner_margin(20)
                                                .corner_radius(12),
                                        )
                                        .show(&ctx, |ui| {
                                            draft.contents(ui);
                                            bounds = ui.min_rect();
                                        });
                                },
                            );
                            output.textures_delta.clear();
                        }
                        let size = bounds.size();
                        let expected = *expected_size.get_or_insert(size);
                        assert!(
                            (size.x - expected.x).abs() < 0.5 && (size.y - expected.y).abs() < 0.5,
                            "Period mode changed dialog size: {expected:?} -> {size:?}; dates={}, error={}",
                            mode == PeriodMode::Dates,
                            !error.is_empty()
                        );
                        assert!(size.x * scale + 40.0 * scale < 960.0);
                        assert!(size.y * scale + 40.0 * scale < 680.0);
                    }
                }
            }
        }
    }
    #[test]
    fn calendar_popover_can_choose_a_leap_day_without_dismissing_the_dialog() {
        let ctx = egui::Context::default();
        Theme::Dark.apply(&ctx);
        install_fonts(&ctx);
        let mut fields = DateTimeFields::from_timestamp(0);
        fields.date = "2024-02-01".into();
        fields.time = "12:30".into();
        let mut button = egui::Pos2::ZERO;
        let mut run_frame = |events| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(960.0, 680.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_root| {
                    let response = egui::Modal::new(egui::Id::new("test-calendar"))
                        .area(
                            egui::Modal::default_area(egui::Id::new("test-calendar"))
                                .fade_in(false),
                        )
                        .show(&ctx, |ui| {
                            ui.set_width(460.0);
                            button = date_time_row(ui, "From", &mut fields).rect.center();
                        });
                    assert!(!response.should_close());
                },
            )
        };
        for _ in 0..2 {
            run_frame(vec![]).textures_delta.clear();
        }
        // Release the closure's borrows before reusing the captured button position.
        drop(run_frame);
        fn clicks(pos: egui::Pos2) -> Vec<egui::Event> {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ]
        }
        let mut position = None;
        fn find_text(shape: &egui::Shape) -> Option<egui::Pos2> {
            match shape {
                egui::Shape::Vec(shapes) => shapes.iter().find_map(find_text),
                egui::Shape::Text(text) if text.galley.job.text == "29" => Some(text.pos),
                _ => None,
            }
        }
        for events in [clicks(button), vec![], vec![]] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(960.0, 680.0),
                    )),
                    events,
                    ..Default::default()
                },
                |_root| {
                    let response =
                        egui::Modal::new(egui::Id::new("test-calendar")).show(&ctx, |ui| {
                            ui.set_width(460.0);
                            let _ = date_time_row(ui, "From", &mut fields);
                        });
                    assert!(!response.should_close());
                },
            );
            position = output.shapes.iter().find_map(|s| find_text(&s.shape));
            output.textures_delta.clear();
        }
        let position =
            position.expect("Calendar popover is missing its leap day") + Vec2::new(3.0, 5.0);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(960.0, 680.0),
                )),
                events: clicks(position),
                ..Default::default()
            },
            |_root| {
                let response = egui::Modal::new(egui::Id::new("test-calendar")).show(&ctx, |ui| {
                    ui.set_width(460.0);
                    let _ = date_time_row(ui, "From", &mut fields);
                });
                assert!(!response.should_close());
            },
        );
        output.textures_delta.clear();
        assert_eq!(fields.date, "2024-02-29");
        assert_eq!(fields.time, "12:30");
    }
    #[test]
    fn custom_duration_and_dates_validate_and_keep_saved_selection() {
        let mut draft = CustomPeriod::from_filter(&Filter::default());
        draft.amount = "540".into();
        draft.unit = Unit::Days;
        let (seconds, range) = draft.selection().unwrap();
        assert_eq!(seconds, 540 * 86400);
        assert!(range.is_none());
        for amount in ["0", "", "1.5", "-1", "4294967296"] {
            draft.amount = amount.into();
            assert!(draft.selection().is_err());
        }
        draft.mode = PeriodMode::Dates;
        draft.from.date = "2025-01-02".into();
        draft.from.time = "12:15".into();
        draft.to.date = "2025-01-02".into();
        draft.to.time = "12:45".into();
        let (seconds, range) = draft.selection().unwrap();
        assert_eq!(seconds, 31 * 60);
        let range = range.unwrap();
        assert_eq!(range.end - range.start + 1, seconds);
        let filter = Filter {
            seconds,
            time_range: Some(range),
            ..Default::default()
        };
        let restored = CustomPeriod::from_filter(&filter);
        assert_eq!(restored.selection().unwrap(), (seconds, Some(range)));
        assert_eq!(period_caption(&filter), "Custom");
        draft.from.date = "2025-01-03".into();
        assert!(draft.selection().is_err());
        draft.from.date = "bad date".into();
        assert!(draft.selection().is_err());
    }
}
