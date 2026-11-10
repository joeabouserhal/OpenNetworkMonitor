//! Line icons on a 24-unit grid, in the style of Lucide, drawn with the painter.
use eframe::egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Overview,
    Connections,
    Adapters,
    Settings,
    Search,
    Close,
    Check,
    ChevronDown,
    ChevronUp,
    ChevronLeft,
    ChevronRight,
    ChevronsUpDown,
    ArrowDown,
    ArrowUp,
    Download,
    Upload,
    More,
    Copy,
    External,
    Pause,
    Play,
    Sun,
    Moon,
    Monitor,
    Filter,
    Info,
    Alert,
    Calendar,
    Palette,
    Minimize,
    Activity,
    Globe,
    Bell,
    Database,
    Clock,
}

enum Part {
    Line(&'static [(f32, f32)]),
    Closed(&'static [(f32, f32)]),
    Circle(f32, f32, f32),
    Dot(f32, f32),
    Rect(f32, f32, f32, f32, u8),
}
use Part::{Circle, Closed, Dot, Line, Rect as Box_};

fn parts(icon: Icon) -> &'static [Part] {
    match icon {
        Icon::Overview => &[
            Box_(3.0, 3.0, 7.0, 9.0, 1),
            Box_(14.0, 3.0, 7.0, 5.0, 1),
            Box_(14.0, 12.0, 7.0, 9.0, 1),
            Box_(3.0, 16.0, 7.0, 5.0, 1),
        ],
        Icon::Connections => &[
            Box_(9.0, 2.0, 6.0, 6.0, 1),
            Box_(2.0, 16.0, 6.0, 6.0, 1),
            Box_(16.0, 16.0, 6.0, 6.0, 1),
            Line(&[(5.0, 16.0), (5.0, 12.0), (19.0, 12.0), (19.0, 16.0)]),
            Line(&[(12.0, 12.0), (12.0, 8.0)]),
        ],
        Icon::Adapters => &[
            Line(&[(22.0, 12.0), (2.0, 12.0)]),
            Line(&[
                (5.45, 5.11),
                (2.0, 12.0),
                (2.0, 18.0),
                (4.0, 20.0),
                (20.0, 20.0),
                (22.0, 18.0),
                (22.0, 12.0),
                (18.55, 5.11),
                (16.76, 4.0),
                (7.24, 4.0),
                (5.45, 5.11),
            ]),
            Dot(6.0, 16.0),
            Dot(10.0, 16.0),
        ],
        Icon::Settings => &[
            Line(&[(21.0, 4.0), (14.0, 4.0)]),
            Line(&[(10.0, 4.0), (3.0, 4.0)]),
            Line(&[(21.0, 12.0), (12.0, 12.0)]),
            Line(&[(8.0, 12.0), (3.0, 12.0)]),
            Line(&[(21.0, 20.0), (16.0, 20.0)]),
            Line(&[(12.0, 20.0), (3.0, 20.0)]),
            Line(&[(14.0, 2.0), (14.0, 6.0)]),
            Line(&[(8.0, 10.0), (8.0, 14.0)]),
            Line(&[(16.0, 18.0), (16.0, 22.0)]),
        ],
        Icon::Search => &[
            Circle(11.0, 11.0, 8.0),
            Line(&[(21.0, 21.0), (16.65, 16.65)]),
        ],
        Icon::Close => &[
            Line(&[(18.0, 6.0), (6.0, 18.0)]),
            Line(&[(6.0, 6.0), (18.0, 18.0)]),
        ],
        Icon::Check => &[Line(&[(20.0, 6.0), (9.0, 17.0), (4.0, 12.0)])],
        Icon::ChevronDown => &[Line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)])],
        Icon::ChevronUp => &[Line(&[(18.0, 15.0), (12.0, 9.0), (6.0, 15.0)])],
        Icon::ChevronLeft => &[Line(&[(15.0, 18.0), (9.0, 12.0), (15.0, 6.0)])],
        Icon::ChevronRight => &[Line(&[(9.0, 18.0), (15.0, 12.0), (9.0, 6.0)])],
        Icon::ChevronsUpDown => &[
            Line(&[(7.0, 15.0), (12.0, 20.0), (17.0, 15.0)]),
            Line(&[(7.0, 9.0), (12.0, 4.0), (17.0, 9.0)]),
        ],
        Icon::ArrowDown => &[
            Line(&[(12.0, 5.0), (12.0, 19.0)]),
            Line(&[(19.0, 12.0), (12.0, 19.0), (5.0, 12.0)]),
        ],
        Icon::ArrowUp => &[
            Line(&[(12.0, 19.0), (12.0, 5.0)]),
            Line(&[(5.0, 12.0), (12.0, 5.0), (19.0, 12.0)]),
        ],
        Icon::Download => &[
            Line(&[
                (21.0, 15.0),
                (21.0, 19.0),
                (19.0, 21.0),
                (5.0, 21.0),
                (3.0, 19.0),
                (3.0, 15.0),
            ]),
            Line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]),
            Line(&[(12.0, 15.0), (12.0, 3.0)]),
        ],
        Icon::Upload => &[
            Line(&[
                (21.0, 15.0),
                (21.0, 19.0),
                (19.0, 21.0),
                (5.0, 21.0),
                (3.0, 19.0),
                (3.0, 15.0),
            ]),
            Line(&[(17.0, 8.0), (12.0, 3.0), (7.0, 8.0)]),
            Line(&[(12.0, 3.0), (12.0, 15.0)]),
        ],
        Icon::More => &[Dot(5.0, 12.0), Dot(12.0, 12.0), Dot(19.0, 12.0)],
        Icon::Copy => &[
            Box_(8.0, 8.0, 14.0, 14.0, 2),
            Line(&[
                (16.0, 8.0),
                (16.0, 4.0),
                (4.0, 4.0),
                (4.0, 16.0),
                (8.0, 16.0),
            ]),
        ],
        Icon::External => &[
            Line(&[(15.0, 3.0), (21.0, 3.0), (21.0, 9.0)]),
            Line(&[(10.0, 14.0), (21.0, 3.0)]),
            Line(&[
                (18.0, 13.0),
                (18.0, 19.0),
                (5.0, 19.0),
                (5.0, 6.0),
                (11.0, 6.0),
            ]),
        ],
        Icon::Pause => &[Box_(14.0, 4.0, 4.0, 16.0, 1), Box_(6.0, 4.0, 4.0, 16.0, 1)],
        Icon::Play => &[Closed(&[(6.0, 3.0), (20.0, 12.0), (6.0, 21.0)])],
        Icon::Sun => &[
            Circle(12.0, 12.0, 4.0),
            Line(&[(12.0, 2.0), (12.0, 4.0)]),
            Line(&[(12.0, 20.0), (12.0, 22.0)]),
            Line(&[(4.93, 4.93), (6.34, 6.34)]),
            Line(&[(17.66, 17.66), (19.07, 19.07)]),
            Line(&[(2.0, 12.0), (4.0, 12.0)]),
            Line(&[(20.0, 12.0), (22.0, 12.0)]),
            Line(&[(6.34, 17.66), (4.93, 19.07)]),
            Line(&[(19.07, 4.93), (17.66, 6.34)]),
        ],
        Icon::Moon => &[Closed(&[
            (12.0, 3.0),
            (9.5, 5.0),
            (8.0, 8.0),
            (8.0, 11.5),
            (9.5, 15.0),
            (12.5, 17.5),
            (16.0, 18.5),
            (19.5, 17.5),
            (21.0, 15.5),
            (21.0, 12.0),
            (20.0, 9.0),
            (18.0, 6.0),
            (15.0, 4.0),
        ])],
        Icon::Monitor => &[
            Box_(2.0, 3.0, 20.0, 14.0, 2),
            Line(&[(8.0, 21.0), (16.0, 21.0)]),
            Line(&[(12.0, 17.0), (12.0, 21.0)]),
        ],
        Icon::Filter => &[Closed(&[
            (22.0, 3.0),
            (2.0, 3.0),
            (10.0, 12.46),
            (10.0, 19.0),
            (14.0, 21.0),
            (14.0, 12.46),
        ])],
        Icon::Info => &[
            Circle(12.0, 12.0, 10.0),
            Line(&[(12.0, 16.0), (12.0, 12.0)]),
            Dot(12.0, 8.0),
        ],
        Icon::Alert => &[
            Closed(&[(12.0, 3.0), (22.0, 20.0), (2.0, 20.0)]),
            Line(&[(12.0, 9.5), (12.0, 13.5)]),
            Dot(12.0, 17.0),
        ],
        Icon::Calendar => &[
            Box_(3.0, 4.0, 18.0, 18.0, 2),
            Line(&[(16.0, 2.0), (16.0, 6.0)]),
            Line(&[(8.0, 2.0), (8.0, 6.0)]),
            Line(&[(3.0, 10.0), (21.0, 10.0)]),
        ],
        Icon::Palette => &[
            Circle(12.0, 12.0, 10.0),
            Dot(13.5, 6.5),
            Dot(17.5, 10.5),
            Dot(8.5, 7.5),
            Dot(6.5, 12.5),
        ],
        Icon::Minimize => &[
            Line(&[(4.0, 14.0), (10.0, 14.0), (10.0, 20.0)]),
            Line(&[(20.0, 10.0), (14.0, 10.0), (14.0, 4.0)]),
            Line(&[(14.0, 10.0), (21.0, 3.0)]),
            Line(&[(3.0, 21.0), (10.0, 14.0)]),
        ],
        Icon::Activity => &[Line(&[
            (22.0, 12.0),
            (18.0, 12.0),
            (15.0, 21.0),
            (9.0, 3.0),
            (6.0, 12.0),
            (2.0, 12.0),
        ])],
        Icon::Globe => &[
            Circle(12.0, 12.0, 10.0),
            Line(&[(2.0, 12.0), (22.0, 12.0)]),
            Closed(&[
                (16.5, 12.0),
                (15.9, 17.0),
                (14.25, 20.66),
                (12.0, 22.0),
                (9.75, 20.66),
                (8.1, 17.0),
                (7.5, 12.0),
                (8.1, 7.0),
                (9.75, 3.34),
                (12.0, 2.0),
                (14.25, 3.34),
                (15.9, 7.0),
            ]),
        ],
        Icon::Bell => &[
            Closed(&[
                (6.0, 8.0),
                (6.8, 5.0),
                (9.0, 2.8),
                (12.0, 2.0),
                (15.0, 2.8),
                (17.2, 5.0),
                (18.0, 8.0),
                (18.4, 13.0),
                (21.0, 17.0),
                (3.0, 17.0),
                (5.6, 13.0),
            ]),
            Line(&[(10.3, 21.0), (11.2, 22.0), (12.8, 22.0), (13.7, 21.0)]),
        ],
        Icon::Database => &[
            Closed(&[
                (21.0, 5.0),
                (19.8, 6.5),
                (16.5, 7.6),
                (12.0, 8.0),
                (7.5, 7.6),
                (4.2, 6.5),
                (3.0, 5.0),
                (4.2, 3.5),
                (7.5, 2.4),
                (12.0, 2.0),
                (16.5, 2.4),
                (19.8, 3.5),
            ]),
            Line(&[(3.0, 5.0), (3.0, 19.0)]),
            Line(&[(21.0, 5.0), (21.0, 19.0)]),
            Line(&[
                (3.0, 12.0),
                (4.2, 13.5),
                (7.5, 14.6),
                (12.0, 15.0),
                (16.5, 14.6),
                (19.8, 13.5),
                (21.0, 12.0),
            ]),
            Line(&[
                (3.0, 19.0),
                (4.2, 20.5),
                (7.5, 21.6),
                (12.0, 22.0),
                (16.5, 21.6),
                (19.8, 20.5),
                (21.0, 19.0),
            ]),
        ],
        Icon::Clock => &[
            Circle(12.0, 12.0, 10.0),
            Line(&[(12.0, 6.0), (12.0, 12.0), (16.0, 14.0)]),
        ],
    }
}

/// Paints `icon` centred on `center`, `size` points square.
pub fn paint(painter: &Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let scale = size / 24.0;
    let origin = center - Vec2::splat(size / 2.0);
    let at = |(x, y): (f32, f32)| origin + Vec2::new(x, y) * scale;
    // Whole physical pixels keep the lines crisp at any display scaling.
    let ppp = painter.pixels_per_point();
    let width = ((2.0 * scale * ppp).round().max(1.0)) / ppp;
    let stroke = Stroke::new(width, color);
    for part in parts(icon) {
        match part {
            Line(points) => {
                painter.add(Shape::line(points.iter().map(|p| at(*p)).collect(), stroke));
            }
            Closed(points) => {
                painter.add(Shape::closed_line(
                    points.iter().map(|p| at(*p)).collect(),
                    stroke,
                ));
            }
            Circle(x, y, r) => {
                painter.circle_stroke(at((*x, *y)), r * scale, stroke);
            }
            Dot(x, y) => {
                painter.circle_filled(at((*x, *y)), (1.2 * scale).max(0.9), color);
            }
            Box_(x, y, w, h, r) => {
                let rect = Rect::from_min_size(at((*x, *y)), Vec2::new(*w, *h) * scale);
                painter.rect_stroke(
                    rect,
                    CornerRadius::same((f32::from(*r) * scale).round() as u8),
                    stroke,
                    StrokeKind::Inside,
                );
            }
        }
    }
}
