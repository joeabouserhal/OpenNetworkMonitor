use crate::{
    model::{Point, ScopeTotals, format_bytes},
    widgets,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
pub const DOWNLOAD_COLOR: Color32 = Color32::from_rgb(86, 202, 164);
pub const UPLOAD_COLOR: Color32 = Color32::from_rgb(225, 167, 83);

pub fn download_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        DOWNLOAD_COLOR
    } else {
        Color32::from_rgb(24, 116, 77)
    }
}
pub fn upload_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        UPLOAD_COLOR
    } else {
        Color32::from_rgb(140, 86, 12)
    }
}

#[derive(Default)]
pub struct Graph;
impl Graph {
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        points: &[Point],
        window: (i64, i64),
        height: f32,
        down: bool,
        up: bool,
        bits: bool,
        log: bool,
    ) {
        let width = ui.available_width().max(1.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, height.max(90.0)), Sense::hover());
        if width < 150.0 {
            return;
        }
        let plot = Rect::from_min_max(
            rect.min + Vec2::new(88.0, 10.0),
            rect.max - Vec2::new(9.0, 25.0),
        );
        let (start, end) = (window.0, window.1.max(window.0 + 1));
        let runs = observed_runs(points, (start, end));
        let maximum = nice_maximum(
            runs.iter()
                .flatten()
                .map(|p| (if down { p.down } else { 0 }).max(if up { p.up } else { 0 }))
                .max()
                .unwrap_or(0),
        );
        let transform = |v: f64| if log { v.ln_1p() } else { v };
        let scaled = transform(maximum);
        let x = |t: i64| {
            plot.left() + ((t - start) as f64 / (end - start) as f64) as f32 * plot.width()
        };
        let y = |v: u64| plot.bottom() - (transform(v as f64) / scaled) as f32 * plot.height();
        let painter = ui.painter();
        let text = axis_color(ui);
        let grid = if ui.visuals().dark_mode {
            Color32::from_gray(60)
        } else {
            Color32::from_gray(217)
        };
        for i in 0..5 {
            let f = i as f32 / 4.0;
            let yy = plot.bottom() - plot.height() * f;
            painter.line_segment(
                [Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)],
                Stroke::new(0.8, grid),
            );
            let value = if log {
                (scaled * f as f64).exp_m1()
            } else {
                scaled * f as f64
            };
            painter.text(
                Pos2::new(plot.left() - 8.0, yy),
                egui::Align2::RIGHT_CENTER,
                speed_label(value, bits),
                FontId::proportional(12.0),
                text,
            );
        }
        let short = end - start <= 300;
        for i in 0..5 {
            let t = start + (end - start) * i / 4;
            let label = chrono::DateTime::from_timestamp(t, 0)
                .map(|d| {
                    d.with_timezone(&chrono::Local)
                        .format(if short {
                            "%H:%M:%S"
                        } else if end - start > 86400 {
                            "%d %b %H:%M"
                        } else {
                            "%H:%M"
                        })
                        .to_string()
                })
                .unwrap_or_default();
            let align = if i == 0 {
                egui::Align2::LEFT_TOP
            } else if i == 4 {
                egui::Align2::RIGHT_TOP
            } else {
                egui::Align2::CENTER_TOP
            };
            painter.text(
                Pos2::new(x(t), plot.bottom() + 8.0),
                align,
                label,
                FontId::proportional(12.0),
                text,
            );
        }
        let lines = painter.with_clip_rect(plot.intersect(ui.clip_rect()));
        for (enabled, is_down, color) in [
            (down, true, download_color(ui)),
            (up, false, upload_color(ui)),
        ] {
            if !enabled {
                continue;
            }
            for run in &runs {
                let samples: Vec<Pos2> = run
                    .iter()
                    .map(|p| Pos2::new(x(p.timestamp), y(if is_down { p.down } else { p.up })))
                    .collect();
                let line = smooth_line(&samples);
                if line.len() > 1 {
                    let mut fill = egui::epaint::Mesh::default();
                    for pair in line.windows(2) {
                        let n = fill.vertices.len() as u32;
                        let tint =
                            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 20);
                        fill.colored_vertex(pair[0], tint);
                        fill.colored_vertex(pair[1], tint);
                        fill.colored_vertex(
                            Pos2::new(pair[1].x, plot.bottom()),
                            Color32::TRANSPARENT,
                        );
                        fill.colored_vertex(
                            Pos2::new(pair[0].x, plot.bottom()),
                            Color32::TRANSPARENT,
                        );
                        fill.add_triangle(n, n + 1, n + 2);
                        fill.add_triangle(n, n + 2, n + 3);
                    }
                    lines.add(egui::Shape::mesh(fill));
                    lines.add(egui::Shape::line(line, Stroke::new(1.8, color)));
                }
                if let Some(last) = samples.last() {
                    lines.circle_filled(*last, 2.0, color);
                }
            }
        }
        if runs.is_empty() {
            painter.text(
                plot.center(),
                egui::Align2::CENTER_CENTER,
                "No recorded samples in this period",
                FontId::proportional(13.0),
                text,
            );
        }
        if let Some(pos) = response.hover_pos().filter(|p| plot.contains(*p)) {
            let t = start + ((pos.x - plot.left()) / plot.width() * (end - start) as f32) as i64;
            let tolerance = ((end - start) as f64 / plot.width() as f64 * 5.0).ceil() as u64;
            if let Some(point) = runs
                .iter()
                .flatten()
                .min_by_key(|p| p.timestamp.abs_diff(t))
                .filter(|p| p.timestamp.abs_diff(t) <= tolerance.max(1))
            {
                let xx = x(point.timestamp);
                lines.line_segment(
                    [Pos2::new(xx, plot.top()), Pos2::new(xx, plot.bottom())],
                    Stroke::new(1.0, text.gamma_multiply(0.6)),
                );
                response.on_hover_ui_at_pointer(|ui| {
                    if let Some(date) = chrono::DateTime::from_timestamp(point.timestamp, 0) {
                        ui.strong(
                            date.with_timezone(&chrono::Local)
                                .format("%d %b · %H:%M:%S")
                                .to_string(),
                        );
                    }
                    ui.label(format!(
                        "Download  {}",
                        speed_label(point.down as f64, bits)
                    ));
                    ui.label(format!("Upload  {}", speed_label(point.up as f64, bits)));
                });
            }
        }
    }
}
/// Shape-preserving cubic interpolation in screen space. Every recorded sample
/// stays on the curve, with no overshoot, extrapolation or averaging of totals.
/// Called separately for each observed run so downtime remains blank.
fn smooth_line(points: &[Pos2]) -> Vec<Pos2> {
    if points.len() < 3 || points.windows(2).any(|p| p[1].x <= p[0].x) {
        return points.to_vec();
    }
    let widths: Vec<_> = points.windows(2).map(|p| p[1].x - p[0].x).collect();
    let slopes: Vec<_> = points
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
        // Keep curved shoulders even for closely spaced samples on long periods.
        // Bound tessellation so wide segments cannot create unbounded work.
        let steps = (h / 2.0).ceil().clamp(4.0, 64.0) as usize;
        for step in 1..steps {
            let t = step as f32 / steps as f32;
            let t2 = t * t;
            let t3 = t2 * t;
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
fn observed_runs(points: &[Point], window: (i64, i64)) -> Vec<Vec<&Point>> {
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
/// Destination bytes are captured app transfers, separate from overall interface totals.
pub fn destination_breakdown(ui: &mut egui::Ui, rect: Rect, totals: &ScopeTotals) {
    let total = totals
        .lan
        .saturating_add(totals.wan)
        .saturating_add(totals.unknown);
    let title_rect = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.top() + 20.0));
    widgets::text_at(
        ui,
        title_rect,
        egui::RichText::new("LAN / WAN").size(15.0).strong(),
        egui::Align::Min,
    );
    widgets::text_at(
        ui,
        title_rect,
        egui::RichText::new(format!("{} captured", format_bytes(total as f64)))
            .size(12.5)
            .weak(),
        egui::Align::Max,
    );
    let response = ui.interact(
        rect,
        ui.make_persistent_id("destination-summary"),
        Sense::hover(),
    );
    response.on_hover_text("Destination breakdown of captured TCP/UDP app traffic in the selected period and adapters. Its total can differ from overall interface usage. LAN includes local and private destinations; WAN covers public destinations. Other is unclassified address space.");
    let bar = Rect::from_min_size(
        rect.min + Vec2::new(0.0, 25.0),
        Vec2::new(rect.width(), 8.0),
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(bar, 4, widgets::border(ui));
    let fractions = segment_fractions(totals.lan, totals.wan, totals.unknown);
    let colors = [download_color(ui), upload_color(ui), axis_color(ui)];
    let mut offset = 0.0f32;
    for (fraction, color) in fractions.into_iter().zip(colors) {
        if fraction <= 0.0 {
            continue;
        }
        let right = (offset + fraction).min(1.0);
        let segment = Rect::from_min_max(
            Pos2::new(bar.left() + bar.width() * offset, bar.top()),
            Pos2::new(bar.left() + bar.width() * right, bar.bottom()),
        );
        let corners = egui::CornerRadius {
            nw: if offset == 0.0 { 4 } else { 0 },
            sw: if offset == 0.0 { 4 } else { 0 },
            ne: if right >= 1.0 { 4 } else { 0 },
            se: if right >= 1.0 { 4 } else { 0 },
        };
        painter.rect_filled(segment, corners, color);
        offset = right;
    }
    let count = if totals.unknown > 0 { 3 } else { 2 };
    let gap = 14.0;
    let width = ((rect.width() - gap * (count - 1) as f32) / count as f32).floor();
    for (i, (name, value, color)) in [
        ("LAN", totals.lan, colors[0]),
        ("WAN", totals.wan, colors[1]),
        ("Other", totals.unknown, colors[2]),
    ]
    .into_iter()
    .take(count)
    .enumerate()
    {
        let left = rect.left() + i as f32 * (width + gap);
        let label = Rect::from_min_size(Pos2::new(left, rect.top() + 40.0), Vec2::new(width, 17.0));
        widgets::text_at(
            ui,
            label,
            egui::RichText::new(name).size(12.5).color(color),
            egui::Align::Min,
        );
        if count == 2 {
            widgets::text_at(
                ui,
                label,
                egui::RichText::new(share_label(fractions[i]))
                    .size(12.0)
                    .weak(),
                egui::Align::Max,
            );
        } else {
            widgets::text_at(
                ui,
                Rect::from_min_size(Pos2::new(left, rect.top() + 78.0), Vec2::new(width, 16.0)),
                egui::RichText::new(share_label(fractions[i]))
                    .size(12.0)
                    .weak(),
                egui::Align::Min,
            );
        }
        widgets::text_at(
            ui,
            Rect::from_min_size(Pos2::new(left, rect.top() + 60.0), Vec2::new(width, 18.0)),
            egui::RichText::new(crate::model::format_compact_bytes(value))
                .size(if count == 2 { 14.0 } else { 13.0 })
                .strong(),
            egui::Align::Min,
        );
    }
}
fn share_label(fraction: f32) -> String {
    if fraction <= 0.0 {
        "0%".into()
    } else if fraction < 0.001 {
        "<0.1%".into()
    } else if fraction > 0.999 && fraction < 1.0 {
        ">99.9%".into()
    } else {
        format!("{:.1}%", fraction * 100.0)
    }
}
fn segment_fractions(lan: u64, wan: u64, unknown: u64) -> [f32; 3] {
    let values = [lan as f64, wan as f64, unknown as f64];
    let sum: f64 = values.iter().sum();
    if sum == 0.0 {
        [0.0; 3]
    } else {
        values.map(|v| (v / sum) as f32)
    }
}
fn nice_maximum(value: u64) -> f64 {
    let target = if value == 0 {
        1024.0
    } else {
        (value as f64 * 1.15).max(32.0)
    };
    let magnitude = 10f64.powf(target.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|f| f * magnitude)
        .find(|v| *v >= target)
        .unwrap_or(target)
}
fn axis_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_gray(190)
    } else {
        Color32::from_gray(86)
    }
}
fn speed_label(bytes: f64, bits: bool) -> String {
    if bits {
        format!("{}bit/s", format_number(bytes * 8.0))
    } else {
        format!("{}/s", format_bytes(bytes))
    }
}
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_spans_do_not_join_measured_zero_or_traffic() {
        let points = vec![
            Point {
                timestamp: 10,
                observed: true,
                ..Default::default()
            },
            Point {
                timestamp: 11,
                observed: false,
                ..Default::default()
            },
            Point {
                timestamp: 30,
                observed: true,
                down: 50,
                ..Default::default()
            },
        ];
        let runs = observed_runs(&points, (0, 3600));
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0][0].down, 0);
    }
    #[test]
    fn selected_period_clips_samples_without_filling_its_empty_left_edge() {
        let points = vec![
            Point {
                timestamp: 100,
                observed: true,
                ..Default::default()
            },
            Point {
                timestamp: 800,
                observed: true,
                ..Default::default()
            },
        ];
        assert_eq!(observed_runs(&points, (500, 800))[0].len(), 1);
        assert_eq!(observed_runs(&points, (-2800, 800))[0].len(), 2);
        assert!(observed_runs(&points, (900, 1200)).is_empty());
    }
    #[test]
    fn smooth_curves_preserve_samples_and_never_overshoot_spikes_or_plateaus() {
        let points = [
            Pos2::new(0.0, 100.0),
            Pos2::new(20.0, 0.0),
            Pos2::new(90.0, 70.0),
            Pos2::new(110.0, 70.0),
            Pos2::new(140.0, 100.0),
        ];
        let curve = smooth_line(&points);
        assert!(curve.len() > points.len());
        assert_eq!(curve.first(), points.first());
        assert_eq!(curve.last(), points.last());
        assert!(points.iter().all(|p| curve.contains(p)));
        assert!(curve.windows(2).all(|p| p[0].x < p[1].x));
        for pair in points.windows(2) {
            let segment: Vec<_> = curve
                .iter()
                .filter(|p| p.x >= pair[0].x && p.x <= pair[1].x)
                .collect();
            assert!(
                segment
                    .iter()
                    .all(|p| p.y >= pair[0].y.min(pair[1].y) && p.y <= pair[0].y.max(pair[1].y))
            );
            assert!(segment.windows(2).all(|p| if pair[1].y >= pair[0].y {
                p[1].y >= p[0].y
            } else {
                p[1].y <= p[0].y
            }));
        }
        // Curved shoulders replace the straight, pointed joins.
        assert_ne!(curve.iter().find(|p| p.x == 10.0).unwrap().y, 50.0);
    }
    #[test]
    fn smooth_curves_keep_missing_intervals_and_sparse_data_intact() {
        let points: Vec<_> = [
            (10, true, 0),
            (20, true, 100),
            (30, true, 0),
            (40, false, 0),
            (80, true, 0),
            (90, true, 100),
            (100, true, 0),
        ]
        .into_iter()
        .map(|(timestamp, observed, down)| Point {
            timestamp,
            observed,
            down,
            up: 0,
        })
        .collect();
        let curves: Vec<_> = observed_runs(&points, (0, 120))
            .iter()
            .map(|run| {
                smooth_line(
                    &run.iter()
                        .map(|p| Pos2::new(p.timestamp as f32, p.down as f32))
                        .collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(curves.len(), 2);
        assert!(curves[0].iter().all(|p| p.x <= 30.0));
        assert!(curves[1].iter().all(|p| p.x >= 80.0));
        for sparse in [
            &[][..],
            &[Pos2::ZERO][..],
            &[Pos2::ZERO, Pos2::new(5.0, 10.0)][..],
        ] {
            assert_eq!(smooth_line(sparse), sparse);
        }
        let dense = [Pos2::ZERO, Pos2::new(0.5, 100.0), Pos2::new(1.0, 0.0)];
        assert!(smooth_line(&dense).len() > dense.len());
        let duplicate = [Pos2::ZERO, Pos2::ZERO, Pos2::new(5.0, 10.0)];
        assert_eq!(smooth_line(&duplicate), duplicate);
    }
    #[test]
    fn destination_shares_handle_large_or_empty_totals() {
        assert_eq!(segment_fractions(0, 0, 0), [0.0; 3]);
        assert_eq!(share_label(0.0), "0%");
        assert_eq!(share_label(0.00007), "<0.1%");
        assert_eq!(share_label(0.99993), ">99.9%");
        assert_eq!(share_label(0.5), "50.0%");
        let f = segment_fractions(u64::MAX, u64::MAX, u64::MAX);
        assert!((f.iter().sum::<f32>() - 1.0).abs() < 0.0001);
    }
}
