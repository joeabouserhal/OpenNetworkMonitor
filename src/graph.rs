use crate::{
    model::{Point, ScopeTotals, format_bytes, format_compact_bytes},
    widgets::{self, bold, paint_text, pal, sans},
};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Vec2};

pub fn download_color(ui: &egui::Ui) -> Color32 {
    pal(ui).down
}
pub fn upload_color(ui: &egui::Ui) -> Color32 {
    pal(ui).up
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
        let p = pal(ui);
        let width = ui.available_width().max(1.0);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(width, height.max(90.0)), Sense::hover());
        if width < 150.0 {
            return;
        }
        let plot = Rect::from_min_max(
            rect.min + Vec2::new(70.0, 8.0),
            rect.max - Vec2::new(6.0, 28.0),
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
        let px = 1.0 / ui.ctx().pixels_per_point();
        for i in 0..5 {
            let f = i as f32 / 4.0;
            let yy = plot.bottom() - plot.height() * f;
            let line = [Pos2::new(plot.left(), yy), Pos2::new(plot.right(), yy)];
            if i == 0 {
                painter.line_segment(
                    line,
                    widgets::px_stroke(ui.ctx(), 1.0, p.hairline.gamma_multiply(2.0)),
                );
            } else {
                painter.extend(egui::Shape::dashed_line(
                    &line,
                    widgets::px_stroke(ui.ctx(), 1.0, p.hairline),
                    3.0 * px,
                    5.0 * px,
                ));
            }
            let value = if log {
                (scaled * f as f64).exp_m1()
            } else {
                scaled * f as f64
            };
            paint_text(
                painter,
                Rect::from_min_max(
                    Pos2::new(rect.left(), yy - 9.0),
                    Pos2::new(plot.left() - 12.0, yy + 9.0),
                ),
                Align2::RIGHT_CENTER,
                &speed_label(value, bits),
                sans(12.0),
                p.dim,
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
                        } else if end - start >= 90 * 86400 {
                            "%d %b %Y"
                        } else if end - start > 86400 {
                            "%d %b %H:%M"
                        } else {
                            "%H:%M"
                        })
                        .to_string()
                })
                .unwrap_or_default();
            let align = match i {
                0 => Align2::LEFT_TOP,
                4 => Align2::RIGHT_TOP,
                _ => Align2::CENTER_TOP,
            };
            let anchor = x(t);
            let area = match i {
                0 => Rect::from_min_size(
                    Pos2::new(anchor, plot.bottom() + 10.0),
                    Vec2::new(90.0, 16.0),
                ),
                4 => Rect::from_min_size(
                    Pos2::new(anchor - 90.0, plot.bottom() + 10.0),
                    Vec2::new(90.0, 16.0),
                ),
                _ => Rect::from_min_size(
                    Pos2::new(anchor - 45.0, plot.bottom() + 10.0),
                    Vec2::new(90.0, 16.0),
                ),
            };
            paint_text(painter, area, align, &label, sans(12.0), p.dim);
        }
        let lines =
            painter.with_clip_rect(plot.expand2(Vec2::new(0.0, 4.0)).intersect(ui.clip_rect()));
        let tint = if p.dark { 52 } else { 40 };
        for (enabled, is_down, color) in [(down, true, p.down), (up, false, p.up)] {
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
                    let top =
                        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), tint);
                    for pair in line.windows(2) {
                        let n = fill.vertices.len() as u32;
                        fill.colored_vertex(pair[0], top);
                        fill.colored_vertex(pair[1], top);
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
                    lines.add(egui::Shape::line(
                        line,
                        widgets::px_stroke(ui.ctx(), 2.0, color),
                    ));
                }
                if let Some(last) = samples.last() {
                    lines.circle_filled(*last, 5.0, p.card);
                    lines.circle_filled(*last, 3.5, color);
                }
            }
        }
        if runs.is_empty() {
            paint_text(
                painter,
                plot,
                Align2::CENTER_CENTER,
                "No recorded samples in this period",
                sans(13.5),
                p.dim,
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
                    widgets::px_stroke(ui.ctx(), 1.0, p.dim.gamma_multiply(0.55)),
                );
                for (enabled, value, color) in [(down, point.down, p.down), (up, point.up, p.up)] {
                    if enabled {
                        let at = Pos2::new(xx, y(value));
                        lines.circle_filled(at, 5.5, p.card);
                        lines.circle_filled(at, 4.0, color);
                    }
                }
                response.on_hover_ui_at_pointer(|ui| {
                    ui.set_min_width(190.0);
                    ui.spacing_mut().item_spacing.y = 6.0;
                    if let Some(date) = chrono::DateTime::from_timestamp(point.timestamp, 0) {
                        ui.label(
                            egui::RichText::new(
                                date.with_timezone(&chrono::Local)
                                    .format("%d %b %Y · %H:%M:%S")
                                    .to_string(),
                            )
                            .size(12.5)
                            .weak(),
                        );
                    }
                    for (enabled, name, value, color) in [
                        (down, "Download", point.down, p.down),
                        (up, "Upload", point.up, p.up),
                    ] {
                        if !enabled {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            widgets::status_dot(ui, color);
                            ui.label(egui::RichText::new(name).size(13.5).weak());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(widgets::strong_text(
                                        speed_label(value as f64, bits),
                                        14.0,
                                    ));
                                },
                            );
                        });
                    }
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
/// Height `destination_breakdown` occupies: heading, bar and a two-line legend.
pub const DESTINATION_HEIGHT: f32 = 78.0;

/// Destination bytes are captured app transfers, separate from overall interface totals.
/// Greys keep download/upload blue and orange meaning only direction.
pub fn destination_breakdown(ui: &mut egui::Ui, totals: &ScopeTotals) {
    let p = pal(ui);
    let total = totals
        .lan
        .saturating_add(totals.wan)
        .saturating_add(totals.unknown);
    let width = ui.available_width();
    let (head, _) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::hover());
    paint_text(
        ui.painter(),
        head,
        Align2::LEFT_CENTER,
        "LAN / WAN",
        bold(14.5),
        p.text,
    );
    paint_text(
        ui.painter(),
        head,
        Align2::RIGHT_CENTER,
        &format!("{} captured", format_bytes(total as f64)),
        sans(12.5),
        p.dim,
    );
    ui.add_space(2.0);
    let (bar, _) = ui.allocate_exact_size(Vec2::new(width, 8.0), Sense::hover());
    let fractions = segment_fractions(totals.lan, totals.wan, totals.unknown);
    let colors = [
        p.text.gamma_multiply(0.4),
        p.text.gamma_multiply(0.68),
        p.text.gamma_multiply(0.2),
    ];
    widgets::split_bar(
        ui.painter(),
        bar,
        &[
            (fractions[0], colors[0]),
            (fractions[1], colors[1]),
            (fractions[2], colors[2]),
        ],
        p.raised,
    );
    ui.add_space(8.0);
    let count = if totals.unknown > 0 { 3 } else { 2 };
    let gap = 14.0;
    let cell_width = ((width - gap * (count - 1) as f32) / count as f32).floor();
    let (legend, _) = ui.allocate_exact_size(Vec2::new(width, 40.0), Sense::hover());
    for (i, (name, value, color)) in [
        ("LAN", totals.lan, colors[0]),
        ("WAN", totals.wan, colors[1]),
        ("Other", totals.unknown, colors[2]),
    ]
    .into_iter()
    .take(count)
    .enumerate()
    {
        let cell = Rect::from_min_size(
            Pos2::new(legend.left() + i as f32 * (cell_width + gap), legend.top()),
            Vec2::new(cell_width, legend.height()),
        );
        let painter = ui.painter();
        painter.circle_filled(Pos2::new(cell.left() + 4.0, cell.top() + 9.0), 3.5, color);
        let label = Rect::from_min_size(
            Pos2::new(cell.left() + 14.0, cell.top()),
            Vec2::new(cell.width() - 14.0, 18.0),
        );
        paint_text(painter, label, Align2::LEFT_CENTER, name, sans(13.0), p.dim);
        if cell_width >= 118.0 {
            paint_text(
                painter,
                label,
                Align2::RIGHT_CENTER,
                &share_label(fractions[i]),
                sans(12.0),
                p.dim,
            );
        }
        paint_text(
            painter,
            Rect::from_min_size(
                cell.min + Vec2::new(0.0, 19.0),
                Vec2::new(cell.width(), 20.0),
            ),
            Align2::LEFT_CENTER,
            &format_compact_bytes(value),
            bold(if count == 3 { 13.5 } else { 15.0 }),
            p.text,
        );
    }
    ui.interact(
        head.union(legend),
        ui.make_persistent_id("destination-summary"),
        Sense::hover(),
    )
    .on_hover_text(format!(
        "LAN {} · WAN {} · Other {}
Destination breakdown of captured TCP/UDP app traffic in the selected period and adapters. Its total can differ from overall interface usage. LAN includes local and private destinations; WAN covers public destinations. Other is unclassified address space.",
        share_label(fractions[0]),
        share_label(fractions[1]),
        share_label(fractions[2]),
    ));
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
