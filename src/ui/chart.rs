//! Charts: a smooth area chart for speeds over time and a stacked bar chart for totals.
use super::{
    text,
    tokens::{hairline, tk},
};
use crate::model::{Point, format_bytes};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo,
    WidgetType,
};

pub fn format_number(mut value: f64) -> String {
    let units = ["", "k", "M", "G", "T"];
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} ")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// `1.50 MiB/s`, or `12.0 Mbit/s` when `bits` is set.
pub fn speed_label(bytes_per_second: f64, bits: bool) -> String {
    if bits {
        format!("{}bit/s", format_number(bytes_per_second * 8.0))
    } else {
        format!("{}/s", format_bytes(bytes_per_second))
    }
}

/// The smallest of 1, 2, 5 or 10 times a power of ten that is at least `value`.
pub fn nice_maximum(value: u64) -> f64 {
    let target = (value as f64 * 1.08).max(1024.0);
    let magnitude = 10f64.powf(target.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|factor| factor * magnitude)
        .find(|v| *v >= target)
        .unwrap_or(target)
}

/// Runs of consecutive observed points inside the window. A gap in observation breaks a
/// run, so downtime is drawn as blank space rather than as a line through zero.
pub fn observed_runs(points: &[Point], window: (i64, i64)) -> Vec<Vec<&Point>> {
    let mut runs = Vec::new();
    let mut current = Vec::new();
    for point in points
        .iter()
        .filter(|p| p.timestamp >= window.0 && p.timestamp <= window.1)
    {
        if point.observed {
            current.push(point);
        } else if !current.is_empty() {
            runs.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// Monotone cubic interpolation in screen space: every sample stays on the curve and
/// the curve never overshoots between samples.
pub fn smooth(points: &[Pos2]) -> Vec<Pos2> {
    if points.len() < 3 || points.windows(2).any(|p| p[1].x <= p[0].x) {
        return points.to_vec();
    }
    let widths: Vec<f32> = points.windows(2).map(|p| p[1].x - p[0].x).collect();
    let slopes: Vec<f32> = points
        .windows(2)
        .zip(&widths)
        .map(|(p, h)| (p[1].y - p[0].y) / h)
        .collect();
    let mut tangents = vec![0.0; points.len()];
    tangents[0] = slopes[0];
    tangents[points.len() - 1] = slopes[slopes.len() - 1];
    for i in 1..points.len() - 1 {
        let (left, right) = (slopes[i - 1], slopes[i]);
        if left != 0.0 && right != 0.0 && left.signum() == right.signum() {
            let w1 = 2.0 * widths[i] + widths[i - 1];
            let w2 = widths[i] + 2.0 * widths[i - 1];
            tangents[i] = (w1 + w2) / (w1 / left + w2 / right);
        }
    }
    let mut curve = Vec::with_capacity(points.len() * 3);
    curve.push(points[0]);
    for (i, pair) in points.windows(2).enumerate() {
        let h = widths[i];
        let steps = (h / 2.0).ceil().clamp(4.0, 64.0) as usize;
        for step in 1..steps {
            let t = step as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let y = (2.0 * t3 - 3.0 * t2 + 1.0) * pair[0].y
                + (t3 - 2.0 * t2 + t) * h * tangents[i]
                + (-2.0 * t3 + 3.0 * t2) * pair[1].y
                + (t3 - t2) * h * tangents[i + 1];
            curve.push(Pos2::new(
                pair[0].x + t * h,
                y.clamp(pair[0].y.min(pair[1].y), pair[0].y.max(pair[1].y)),
            ));
        }
        curve.push(pair[1]);
    }
    curve
}

pub struct AreaChart<'a> {
    pub points: &'a [Point],
    pub window: (i64, i64),
    pub height: f32,
    pub show_down: bool,
    pub show_up: bool,
    pub bits: bool,
    pub log_scale: bool,
}

/// One row of a tooltip: a colour swatch, a name and a value.
fn tooltip(ui: &mut Ui, heading: &str, rows: &[(Color32, &str, String)]) {
    let t = tk(ui.ctx());
    ui.set_min_width(170.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    text::paint(
        ui.painter(),
        rect,
        Align2::LEFT_CENTER,
        heading,
        text::medium(12.5),
        t.muted_foreground,
    );
    for (color, name, value) in rows {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::hover());
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Label, true, format!("{name} {value}"))
        });
        let painter = ui.painter();
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(rect.left(), rect.center().y - 5.0),
                Vec2::splat(10.0),
            ),
            CornerRadius::same(2),
            *color,
        );
        text::paint(
            painter,
            Rect::from_min_max(Pos2::new(rect.left() + 18.0, rect.top()), rect.max),
            Align2::LEFT_CENTER,
            name,
            text::regular(13.5),
            t.muted_foreground,
        );
        text::paint(
            painter,
            rect,
            Align2::RIGHT_CENTER,
            value,
            text::medium(13.5),
            t.foreground,
        );
    }
}

impl AreaChart<'_> {
    pub fn show(self, ui: &mut Ui) {
        let t = tk(ui.ctx());
        let width = ui.available_width().max(1.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, self.height.max(120.0)), Sense::hover());
        response.widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::Label,
                true,
                "Chart of download and upload speed over the selected period. Hover for the value at a point.",
            )
        });
        if width < 160.0 {
            return;
        }
        let plot = Rect::from_min_max(
            rect.min + Vec2::new(78.0, 8.0),
            rect.max - Vec2::new(4.0, 28.0),
        );
        let (start, end) = (self.window.0, self.window.1.max(self.window.0 + 1));
        let runs = observed_runs(self.points, (start, end));
        let (down, up) = (self.show_down, self.show_up);
        let maximum = nice_maximum(
            runs.iter()
                .flatten()
                .map(|p| (if down { p.down } else { 0 }).max(if up { p.up } else { 0 }))
                .max()
                .unwrap_or(0),
        );
        let log = self.log_scale;
        let transform = |v: f64| if log { v.ln_1p() } else { v };
        let scaled = transform(maximum);
        let x = |ts: i64| {
            plot.left() + ((ts - start) as f64 / (end - start) as f64) as f32 * plot.width()
        };
        let y = |v: u64| plot.bottom() - (transform(v as f64) / scaled) as f32 * plot.height();
        let painter = ui.painter();
        let px = 1.0 / ui.ctx().pixels_per_point();
        // Horizontal grid with value labels.
        for i in 0..5 {
            let f = i as f32 / 4.0;
            let yy = plot.bottom() - plot.height() * f;
            let line = [Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)];
            if i == 0 {
                painter.line_segment(line, hairline(ui.ctx(), t.border));
            } else {
                painter.extend(Shape::dashed_line(
                    &line,
                    hairline(ui.ctx(), t.border),
                    3.0 * px.max(0.5) * 2.0,
                    4.0,
                ));
            }
            let value = if log {
                (scaled * f64::from(f)).exp_m1()
            } else {
                scaled * f64::from(f)
            };
            text::paint(
                painter,
                Rect::from_min_max(
                    Pos2::new(rect.left(), yy - 9.0),
                    Pos2::new(plot.left() - 10.0, yy + 9.0),
                ),
                Align2::RIGHT_CENTER,
                &speed_label(value, self.bits),
                text::regular(12.0),
                t.muted_foreground,
            );
        }
        // Time axis.
        let span = end - start;
        let format = if span <= 300 {
            "%H:%M:%S"
        } else if span >= 90 * 86_400 {
            "%d %b %Y"
        } else if span > 86_400 {
            "%d %b %H:%M"
        } else {
            "%H:%M"
        };
        for i in 0..5 {
            let ts = start + span * i / 4;
            let label = chrono::DateTime::from_timestamp(ts, 0)
                .map(|d| d.with_timezone(&chrono::Local).format(format).to_string())
                .unwrap_or_default();
            let anchor = x(ts);
            let (area, align) = match i {
                0 => (
                    Rect::from_min_size(
                        Pos2::new(anchor, plot.bottom() + 8.0),
                        Vec2::new(100.0, 16.0),
                    ),
                    Align2::LEFT_TOP,
                ),
                4 => (
                    Rect::from_min_size(
                        Pos2::new(anchor - 100.0, plot.bottom() + 8.0),
                        Vec2::new(100.0, 16.0),
                    ),
                    Align2::RIGHT_TOP,
                ),
                _ => (
                    Rect::from_min_size(
                        Pos2::new(anchor - 50.0, plot.bottom() + 8.0),
                        Vec2::new(100.0, 16.0),
                    ),
                    Align2::CENTER_TOP,
                ),
            };
            text::paint(
                painter,
                area,
                align,
                &label,
                text::regular(12.0),
                t.muted_foreground,
            );
        }
        let clipped =
            painter.with_clip_rect(plot.expand2(Vec2::new(0.0, 4.0)).intersect(ui.clip_rect()));
        for (enabled, is_down, color) in [(down, true, t.chart_down), (up, false, t.chart_up)] {
            if !enabled {
                continue;
            }
            for run in &runs {
                let samples: Vec<Pos2> = run
                    .iter()
                    .map(|p| Pos2::new(x(p.timestamp), y(if is_down { p.down } else { p.up })))
                    .collect();
                let line = smooth(&samples);
                if line.len() > 1 {
                    let mut fill = egui::epaint::Mesh::default();
                    let top = Color32::from_rgba_unmultiplied(
                        color.r(),
                        color.g(),
                        color.b(),
                        if t.is_dark() { 70 } else { 56 },
                    );
                    let bottom =
                        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 4);
                    for pair in line.windows(2) {
                        let n = fill.vertices.len() as u32;
                        fill.colored_vertex(pair[0], top);
                        fill.colored_vertex(pair[1], top);
                        fill.colored_vertex(Pos2::new(pair[1].x, plot.bottom()), bottom);
                        fill.colored_vertex(Pos2::new(pair[0].x, plot.bottom()), bottom);
                        fill.add_triangle(n, n + 1, n + 2);
                        fill.add_triangle(n, n + 2, n + 3);
                    }
                    clipped.add(Shape::mesh(fill));
                    clipped.add(Shape::line(line, Stroke::new(2.0, color)));
                }
            }
        }
        if runs.is_empty() {
            text::paint(
                painter,
                plot,
                Align2::CENTER_CENTER,
                "No recorded samples in this period",
                text::regular(14.0),
                t.muted_foreground,
            );
        }
        // Crosshair and tooltip at the sample nearest the pointer.
        if let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) {
            let ts = start + ((pos.x - plot.left()) / plot.width() * span as f32) as i64;
            let tolerance = ((span as f64 / f64::from(plot.width())) * 6.0).ceil() as u64;
            if let Some(point) = runs
                .iter()
                .flatten()
                .min_by_key(|p| p.timestamp.abs_diff(ts))
                .filter(|p| p.timestamp.abs_diff(ts) <= tolerance.max(1))
            {
                let xx = x(point.timestamp);
                clipped.line_segment(
                    [Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())],
                    Stroke::new(px.max(0.5), t.muted_foreground.gamma_multiply(0.6)),
                );
                for (enabled, value, color) in
                    [(down, point.down, t.chart_down), (up, point.up, t.chart_up)]
                {
                    if enabled {
                        let at = Pos2::new(xx, y(value));
                        clipped.circle_filled(at, 5.0, t.card);
                        clipped.circle_stroke(at, 5.0, Stroke::new(2.0, color));
                    }
                }
                let heading = chrono::DateTime::from_timestamp(point.timestamp, 0)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%d %b %Y, %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_default();
                let mut rows = Vec::new();
                if down {
                    rows.push((
                        t.chart_down,
                        "Download",
                        speed_label(point.down as f64, self.bits),
                    ));
                }
                if up {
                    rows.push((
                        t.chart_up,
                        "Upload",
                        speed_label(point.up as f64, self.bits),
                    ));
                }
                response.on_hover_ui_at_pointer(|ui| tooltip(ui, &heading, &rows));
            }
        }
    }
}

/// One bar of a totals chart.
pub struct BarValue<'a> {
    pub label: &'a str,
    pub down: u64,
    pub up: u64,
}

/// A stacked bar chart of totals, oldest on the left.
pub fn bars(ui: &mut Ui, values: &[BarValue<'_>], height: f32, empty_message: &str) {
    let t = tk(ui.ctx());
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    response.widget_info(|| {
        WidgetInfo::labeled(
            WidgetType::Label,
            true,
            "Bar chart of this app's download and upload totals. Hover a bar for its values.",
        )
    });
    let peak = values
        .iter()
        .map(|v| v.down.saturating_add(v.up))
        .max()
        .unwrap_or(0);
    if peak == 0 {
        text::paint(
            ui.painter(),
            rect,
            Align2::CENTER_CENTER,
            empty_message,
            text::regular(14.0),
            t.muted_foreground,
        );
        return;
    }
    let axis = 22.0;
    let chart = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 20.0),
        Pos2::new(rect.right(), rect.bottom() - axis),
    );
    let painter = ui.painter();
    text::paint(
        painter,
        Rect::from_min_size(rect.min, Vec2::new(rect.width(), 16.0)),
        Align2::LEFT_CENTER,
        &format!("Peak {}", format_bytes(peak as f64)),
        text::regular(12.0),
        t.muted_foreground,
    );
    let ppp = ui.ctx().pixels_per_point();
    painter.hline(
        chart.x_range(),
        ((chart.bottom() * ppp).round() + 0.5) / ppp,
        hairline(ui.ctx(), t.border),
    );
    let slot = chart.width() / values.len() as f32;
    let width = (slot * 0.7).clamp(1.0, 28.0);
    let hovered = response
        .hover_pos()
        .filter(|pos| chart.expand2(Vec2::new(0.0, 20.0)).contains(*pos))
        .map(|pos| (((pos.x - chart.left()) / slot) as usize).min(values.len() - 1));
    let scale = chart.height() / peak as f32;
    for (index, value) in values.iter().enumerate() {
        let centre = chart.left() + (index as f32 + 0.5) * slot;
        if hovered == Some(index) {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(centre - slot / 2.0, chart.top()),
                    Pos2::new(centre + slot / 2.0, chart.bottom()),
                ),
                CornerRadius::same(4),
                t.muted.gamma_multiply(0.7),
            );
        }
        let (down, up) = (value.down as f32 * scale, value.up as f32 * scale);
        let x = centre - width / 2.0;
        let round = CornerRadius::same(if width > 6.0 { 3 } else { 0 });
        if value.down > 0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, chart.bottom() - down.max(1.0)),
                    Pos2::new(x + width, chart.bottom()),
                ),
                round,
                t.chart_down,
            );
        }
        if value.up > 0 {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x, chart.bottom() - down - up.max(1.0) - 1.0),
                    Pos2::new(x + width, chart.bottom() - down - 1.0),
                ),
                round,
                t.chart_up,
            );
        }
    }
    let labels = Rect::from_min_max(Pos2::new(rect.left(), chart.bottom() + 6.0), rect.max);
    if let (Some(first), Some(last)) = (values.first(), values.last()) {
        text::paint(
            painter,
            labels,
            Align2::LEFT_CENTER,
            first.label,
            text::regular(12.0),
            t.muted_foreground,
        );
        if values.len() > 1 {
            text::paint(
                painter,
                labels,
                Align2::RIGHT_CENTER,
                last.label,
                text::regular(12.0),
                t.muted_foreground,
            );
        }
    }
    if let Some(index) = hovered {
        let value = &values[index];
        let rows = [
            (t.chart_down, "Download", format_bytes(value.down as f64)),
            (t.chart_up, "Upload", format_bytes(value.up as f64)),
        ];
        response.on_hover_ui_at_pointer(|ui| tooltip(ui, value.label, &rows));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(timestamp: i64, observed: bool) -> Point {
        Point {
            observed,
            timestamp,
            down: 10,
            up: 1,
        }
    }

    #[test]
    fn unobserved_time_splits_the_line_instead_of_drawing_zero() {
        let points = [
            point(1, true),
            point(2, true),
            point(3, false),
            point(4, true),
            point(9, true),
        ];
        let runs = observed_runs(&points, (0, 8));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].len(), 2);
        assert_eq!(runs[1].len(), 1, "Points outside the window are dropped");
    }

    #[test]
    fn smoothing_keeps_every_sample_and_never_overshoots() {
        let points = [
            Pos2::new(0.0, 10.0),
            Pos2::new(10.0, 2.0),
            Pos2::new(20.0, 2.0),
            Pos2::new(30.0, 8.0),
        ];
        let curve = smooth(&points);
        for sample in &points {
            assert!(
                curve.iter().any(|c| (*c - *sample).length() < 1e-3),
                "{sample:?}"
            );
        }
        assert!(
            curve
                .iter()
                .all(|c| c.y >= 2.0 - 1e-3 && c.y <= 10.0 + 1e-3)
        );
        assert_eq!(smooth(&points[..2]), points[..2].to_vec());
    }

    #[test]
    fn the_axis_maximum_is_a_round_number_above_the_data() {
        for value in [0u64, 900, 5_000, 123_456, 40_000_000] {
            let maximum = nice_maximum(value);
            assert!(maximum >= value as f64, "{value} -> {maximum}");
            let magnitude = 10f64.powf(maximum.log10().floor());
            assert!(
                [1.0, 2.0, 5.0, 10.0]
                    .iter()
                    .any(|f| (maximum - f * magnitude).abs() < 1e-6),
                "{maximum}"
            );
        }
    }

    #[test]
    fn speeds_read_in_bytes_or_bits() {
        assert_eq!(speed_label(1_048_576.0, false), "1.00 MiB/s");
        assert_eq!(speed_label(125_000.0, true), "1.0 Mbit/s");
    }
}
