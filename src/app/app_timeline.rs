//! One app's usage over the selected period, as stacked download/upload bars.
use crate::model::{AppSeries, SeriesView, format_bytes};
use crate::widgets::{self, paint_text, pal, radius, sans};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use eframe::egui::{self, Align2, Pos2, Rect, Sense, Vec2};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TimelineMode {
    /// Bars sized to the period, from minutes up to days.
    #[default]
    Auto,
    Daily,
    Monthly,
}
impl TimelineMode {
    /// How far back the worker has to read for this mode.
    pub(super) fn view(self) -> SeriesView {
        match self {
            Self::Auto => SeriesView::Period,
            Self::Daily => SeriesView::Daily,
            Self::Monthly => SeriesView::Monthly,
        }
    }
    /// What the bars cover, when that is not the period chosen at the top.
    pub(super) fn span_caption(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Daily => Some("Last 30 days"),
            Self::Monthly => Some("Last 12 months"),
        }
    }
    pub(super) const ALL: [(Self, &'static str); 3] = [
        (Self::Auto, "Over time"),
        (Self::Daily, "Daily"),
        (Self::Monthly, "Monthly"),
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Bar {
    pub(super) label: String,
    pub(super) down: u64,
    pub(super) up: u64,
}

const AUTO_STEPS: [i64; 11] = [
    60, 300, 900, 1800, 3600, 7200, 10_800, 21_600, 43_200, 86_400, 604_800,
];
/// Roughly how many bars the automatic mode aims for.
const TARGET_BARS: i64 = 72;
const MAX_DAILY_BARS: usize = 366;
const MAX_MONTHLY_BARS: usize = 60;

fn local(timestamp: i64) -> Option<chrono::DateTime<Local>> {
    Local.timestamp_opt(timestamp, 0).single()
}

/// Groups the series into the bars for `mode`. Periods without traffic stay as empty
/// bars so gaps are visible.
pub(super) fn bars(series: &AppSeries, mode: TimelineMode) -> Vec<Bar> {
    let (start, end) = series.window;
    match mode {
        TimelineMode::Auto => auto_bars(series, start, end),
        TimelineMode::Daily => calendar_bars(series, start, end, false),
        TimelineMode::Monthly => calendar_bars(series, start, end, true),
    }
}

fn auto_bars(series: &AppSeries, start: i64, end: i64) -> Vec<Bar> {
    let span = (end - start).max(series.step);
    let wanted = span / TARGET_BARS;
    let size = AUTO_STEPS
        .into_iter()
        .find(|&step| step >= wanted && step >= series.step)
        .unwrap_or(AUTO_STEPS[AUTO_STEPS.len() - 1]);
    let first = start / size * size;
    let count = ((end / size * size - first) / size + 1).clamp(1, 400) as usize;
    let mut result = vec![(0u64, 0u64); count];
    for &(at, down, up) in &series.points {
        let index = ((at / size * size - first) / size) as usize;
        if let Some(slot) = result.get_mut(index) {
            slot.0 = slot.0.saturating_add(down);
            slot.1 = slot.1.saturating_add(up);
        }
    }
    let format = if size >= 86_400 {
        "%d %b"
    } else if span > 86_400 {
        "%d %b %H:%M"
    } else {
        "%H:%M"
    };
    result
        .into_iter()
        .enumerate()
        .map(|(i, (down, up))| Bar {
            label: local(first + i as i64 * size)
                .map(|time| time.format(format).to_string())
                .unwrap_or_default(),
            down,
            up,
        })
        .collect()
}

/// Bars per local calendar day, or per month when `monthly`.
fn calendar_bars(series: &AppSeries, start: i64, end: i64, monthly: bool) -> Vec<Bar> {
    let (Some(first), Some(last)) = (local(start), local(end)) else {
        return Vec::new();
    };
    let key = |date: NaiveDate| {
        if monthly {
            date.with_day(1).unwrap_or(date)
        } else {
            date
        }
    };
    let next = |date: NaiveDate| {
        if monthly {
            date.checked_add_months(chrono::Months::new(1))
        } else {
            date.succ_opt()
        }
    };
    let mut days = Vec::new();
    let mut day = key(first.date_naive());
    let last_day = key(last.date_naive());
    while day <= last_day {
        days.push(day);
        match next(day) {
            Some(following) => day = following,
            None => break,
        }
    }
    let limit = if monthly {
        MAX_MONTHLY_BARS
    } else {
        MAX_DAILY_BARS
    };
    if days.len() > limit {
        days.drain(..days.len() - limit);
    }
    let mut totals = vec![(0u64, 0u64); days.len()];
    for &(at, down, up) in &series.points {
        let Some(time) = local(at) else { continue };
        if let Ok(index) = days.binary_search(&key(time.date_naive())) {
            totals[index].0 = totals[index].0.saturating_add(down);
            totals[index].1 = totals[index].1.saturating_add(up);
        }
    }
    days.into_iter()
        .zip(totals)
        .map(|(date, (down, up))| Bar {
            label: if monthly {
                date.format("%b %Y").to_string()
            } else {
                date.format("%a %d %b").to_string()
            },
            down,
            up,
        })
        .collect()
}

/// Draws the bars into the width available. `height` includes the axis labels.
pub(super) fn show(ui: &mut egui::Ui, bars: &[Bar], height: f32) {
    let p = pal(ui);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let peak = bars
        .iter()
        .map(|bar| bar.down.saturating_add(bar.up))
        .max()
        .unwrap_or(0);
    if peak == 0 {
        paint_text(
            ui.painter(),
            rect,
            Align2::CENTER_CENTER,
            "No traffic from this app in this period",
            sans(13.5),
            p.dim,
        );
        return;
    }
    let axis = 20.0;
    let chart = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 16.0),
        Pos2::new(rect.right(), rect.bottom() - axis),
    );
    let painter = ui.painter();
    paint_text(
        painter,
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), 14.0)),
        Align2::LEFT_CENTER,
        &format!("Peak {}", format_bytes(peak as f64)),
        sans(12.0),
        p.dim,
    );
    let ppp = ui.ctx().pixels_per_point();
    painter.hline(
        chart.x_range(),
        ((chart.bottom() * ppp).round() + 0.5) / ppp,
        widgets::hairline(ui),
    );
    let slot = chart.width() / bars.len() as f32;
    let width = (slot * 0.72).clamp(1.0, 28.0);
    let hovered = response
        .hover_pos()
        .filter(|pos| chart.expand2(Vec2::new(0.0, 16.0)).contains(*pos))
        .map(|pos| (((pos.x - chart.left()) / slot) as usize).min(bars.len() - 1));
    let scale = chart.height() / peak as f32;
    for (index, bar) in bars.iter().enumerate() {
        let centre = chart.left() + (index as f32 + 0.5) * slot;
        if hovered == Some(index) {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(centre - slot / 2.0, chart.top()),
                    Pos2::new(centre + slot / 2.0, chart.bottom()),
                ),
                radius(4.0),
                p.raised,
            );
        }
        let down = bar.down as f32 * scale;
        let up = bar.up as f32 * scale;
        let x = centre - width / 2.0;
        let bottom = chart.bottom();
        if bar.down > 0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, bottom - down.max(1.0)),
                    Pos2::new(x + width, bottom),
                ),
                radius(if width > 4.0 { 2.0 } else { 0.0 }),
                p.down,
            );
        }
        if bar.up > 0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, bottom - down - up.max(1.0)),
                    Pos2::new(x + width, bottom - down),
                ),
                radius(if width > 4.0 { 2.0 } else { 0.0 }),
                p.up,
            );
        }
    }
    let labels = Rect::from_min_max(Pos2::new(rect.left(), chart.bottom() + 4.0), rect.max);
    if let (Some(first), Some(last)) = (bars.first(), bars.last()) {
        paint_text(
            painter,
            labels,
            Align2::LEFT_CENTER,
            &first.label,
            sans(12.0),
            p.dim,
        );
        if bars.len() > 1 {
            paint_text(
                painter,
                labels,
                Align2::RIGHT_CENTER,
                &last.label,
                sans(12.0),
                p.dim,
            );
        }
    }
    if let Some(index) = hovered {
        let bar = &bars[index];
        response.on_hover_text(format!(
            "{}\nDownloaded {}\nUploaded {}",
            bar.label,
            format_bytes(bar.down as f64),
            format_bytes(bar.up as f64)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(step: i64, window: (i64, i64), points: &[(i64, u64, u64)]) -> AppSeries {
        AppSeries {
            app: "a.exe".into(),
            view: SeriesView::Period,
            read_at: 0,
            revision: 0,
            step,
            window,
            points: points.to_vec(),
        }
    }

    #[test]
    fn automatic_bars_fit_the_period_and_keep_totals() {
        // One hour read by the minute: one bar per minute at most, 72 bars aim.
        let hour = series(
            60,
            (3600, 7200),
            &[(3600, 10, 1), (3660, 20, 2), (7140, 5, 0)],
        );
        let result = bars(&hour, TimelineMode::Auto);
        assert!(result.len() <= 73, "{}", result.len());
        assert_eq!(result.iter().map(|b| b.down).sum::<u64>(), 35);
        assert_eq!(result.iter().map(|b| b.up).sum::<u64>(), 3);

        // A month read by the hour needs bars of at least an hour, and here a day.
        let month = series(3600, (0, 30 * 86_400), &[(0, 1, 1), (86_400 * 29, 9, 9)]);
        let result = bars(&month, TimelineMode::Auto);
        assert!(result.len() <= 80, "{}", result.len());
        assert_eq!(result.iter().map(|b| b.down).sum::<u64>(), 10);
    }

    #[test]
    fn daily_and_monthly_bars_cover_every_calendar_period_in_the_window() {
        let day = 86_400;
        let start = Local
            .with_ymd_and_hms(2026, 1, 30, 12, 0, 0)
            .unwrap()
            .timestamp();
        let end = start + 5 * day;
        let points = [(start, 100, 10), (start + 2 * day, 50, 5), (end, 7, 1)];
        let data = series(3600, (start, end), &points);

        let daily = bars(&data, TimelineMode::Daily);
        assert_eq!(daily.len(), 6, "30 Jan to 4 Feb inclusive");
        assert_eq!(daily.iter().map(|b| b.down).sum::<u64>(), 157);
        assert_eq!(daily[0].down, 100);
        assert_eq!(daily[1].down, 0, "Quiet days stay as empty bars");

        let monthly = bars(&data, TimelineMode::Monthly);
        assert_eq!(monthly.len(), 2);
        assert_eq!(monthly[0].label, "Jan 2026");
        assert_eq!(monthly[0].down + monthly[1].down, 157);
        assert!(monthly[1].down >= 7);
    }

    #[test]
    fn long_periods_are_capped_to_the_most_recent_bars() {
        let end = Local
            .with_ymd_and_hms(2026, 6, 1, 0, 0, 0)
            .unwrap()
            .timestamp();
        let data = series(3600, (end - 4000 * 86_400, end), &[(end, 4, 4)]);
        assert_eq!(bars(&data, TimelineMode::Daily).len(), MAX_DAILY_BARS);
        assert_eq!(bars(&data, TimelineMode::Monthly).len(), MAX_MONTHLY_BARS);
        assert_eq!(
            bars(&data, TimelineMode::Daily).last().map(|b| b.down),
            Some(4)
        );
    }
}
