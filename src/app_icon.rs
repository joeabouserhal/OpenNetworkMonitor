/// Shared artwork for the window, tray, and executable resource. This file is also compiled by
/// build.rs, so it depends on nothing but the standard library.
///
/// The icon is the sidebar's mark: a tile in the app's dark background colour with no border, so it reads on light and dark
/// taskbars alike, and three large rounded bars, the middle one in the download blue.
const BARS: [(f64, f64, f64, f64); 3] = [
    (4.4, 16.0, 10.6, 28.0),
    (12.9, 4.0, 19.1, 28.0),
    (21.4, 11.0, 27.6, 28.0),
];
/// The bars alone fill the whole icon, for the tray's single-colour styles.
const GLYPH: [(f64, f64, f64, f64); 3] = [
    (1.0, 16.0, 9.0, 31.0),
    (12.0, 1.0, 20.0, 31.0),
    (23.0, 9.0, 31.0, 31.0),
];
/// The app's dark background.
const BACKGROUND: [f64; 3] = [
    0x09 as f64 / 255.0,
    0x09 as f64 / 255.0,
    0x0b as f64 / 255.0,
];
const WHITE: [f64; 3] = [
    0xfa as f64 / 255.0,
    0xfa as f64 / 255.0,
    0xfa as f64 / 255.0,
];
const BLUE: [f64; 3] = [
    0x3b as f64 / 255.0,
    0x82 as f64 / 255.0,
    0xf6 as f64 / 255.0,
];

/// Signed distance from a point to a rounded rectangle: negative inside.
fn distance(x: f64, y: f64, centre: (f64, f64), half: (f64, f64), radius: f64) -> f64 {
    let dx = ((x - centre.0).abs() - (half.0 - radius)).max(0.0);
    let dy = ((y - centre.1).abs() - (half.1 - radius)).max(0.0);
    dx.hypot(dy) - radius
}

/// The colour at a point of the 32-unit artwork, or `None` outside the tile.
fn shade(x: f64, y: f64) -> Option<[f64; 3]> {
    if distance(x, y, (16.0, 16.0), (16.0, 16.0), 7.0) > 0.0 {
        return None;
    }
    let bar = BARS
        .iter()
        .enumerate()
        .find_map(|(index, &(x0, y0, x1, y1))| {
            let bar = distance(
                x,
                y,
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                ((x1 - x0) / 2.0, (y1 - y0) / 2.0),
                2.2,
            );
            (bar <= 0.0).then_some(if index == 1 { BLUE } else { WHITE })
        });
    Some(bar.unwrap_or(BACKGROUND))
}

/// The full icon: the tile with its bars, smooth at any size.
pub fn pixels(size: u32) -> Vec<u8> {
    render(size, shade)
}

/// The bars alone on a clear background, all in `color`, for the small icon in the window's
/// title bar.
#[allow(dead_code)]
pub fn mark(size: u32, color: [u8; 3]) -> Vec<u8> {
    let color = color.map(|channel| f64::from(channel) / 255.0);
    render(size, |x, y| {
        GLYPH.iter().find_map(|&(x0, y0, x1, y1)| {
            let bar = distance(
                x,
                y,
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                ((x1 - x0) / 2.0, (y1 - y0) / 2.0),
                2.4,
            );
            (bar <= 0.0).then_some(color)
        })
    })
}

/// Anti-aliased RGBA of the 32-unit artwork described by `shade`.
fn render(size: u32, shade: impl Fn(f64, f64) -> Option<[f64; 3]>) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let unit = 32.0 / f64::from(size);
    for y in 0..size {
        for x in 0..size {
            let (mut red, mut green, mut blue, mut covered) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (f64::from(x) + (f64::from(sx) + 0.5) / f64::from(SAMPLES)) * unit;
                    let py = (f64::from(y) + (f64::from(sy) + 0.5) / f64::from(SAMPLES)) * unit;
                    if let Some([r, g, b]) = shade(px, py) {
                        red += r;
                        green += g;
                        blue += b;
                        covered += 1.0;
                    }
                }
            }
            if covered > 0.0 {
                let index = ((y * size + x) * 4) as usize;
                let to_byte = |sum: f64| (sum / covered * 255.0).round() as u8;
                let alpha = (covered / f64::from(SAMPLES * SAMPLES) * 255.0).round() as u8;
                rgba[index..index + 4].copy_from_slice(&[
                    to_byte(red),
                    to_byte(green),
                    to_byte(blue),
                    alpha,
                ]);
            }
        }
    }
    rgba
}

/// Only the three bars, filling the icon, in one colour, for the tray icon's accent and monochrome styles.
/// The build script needs just `pixels`, hence the allowance.
#[allow(dead_code)]
pub fn tinted(size: u32, [red, green, blue]: [u8; 3]) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let scale = f64::from(size) / 32.0;
    for y in 0..size {
        for x in 0..size {
            let left = f64::from(x);
            let top = f64::from(y);
            let coverage: f64 = GLYPH
                .iter()
                .map(|&(x0, y0, x1, y1)| {
                    let width = ((left + 1.0).min(x1 * scale) - left.max(x0 * scale)).max(0.0);
                    let height = ((top + 1.0).min(y1 * scale) - top.max(y0 * scale)).max(0.0);
                    width * height
                })
                .sum();
            let alpha = (coverage.min(1.0) * 255.0).round() as u8;
            if alpha > 0 {
                let index = ((y * size + x) * 4) as usize;
                rgba[index..index + 4].copy_from_slice(&[red, green, blue, alpha]);
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let index = ((y * size + x) * 4) as usize;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    #[test]
    fn the_icon_is_a_borderless_dark_tile_with_three_large_bars_at_every_size() {
        for size in [16, 32, 48, 256] {
            let icon = pixels(size);
            assert_eq!(icon.len(), (size * size * 4) as usize);
            let at = |x: f64, y: f64| {
                pixel(
                    &icon,
                    size,
                    (x / 32.0 * f64::from(size)) as u32,
                    (y / 32.0 * f64::from(size)) as u32,
                )
            };
            assert_eq!(at(0.0, 0.0)[3], 0, "{size}: the corner is clear");
            let edge = at(16.0, 31.0);
            assert_eq!(edge[3], 255, "{size}: the tile reaches the edge");
            assert_eq!(edge[..3], [0x09, 0x09, 0x0b], "{size}: with no border");
            assert!(at(7.5, 24.0)[0] > 200, "{size}: the first bar is light");
            let middle = at(16.0, 16.0);
            assert!(
                middle[2] > 200 && middle[0] < 100,
                "{size}: the middle bar is blue"
            );
            assert_eq!(at(16.0, 5.5)[2], 0xf6, "{size}: the middle bar is tall");
            if size >= 32 {
                // At 16 px the gap is barely a pixel wide, so it blends with its bars.
                assert!(at(11.75, 24.0)[0] < 60, "{size}: the gap shows the tile");
            }
        }
    }

    #[test]
    fn the_tray_glyph_keeps_the_bars_in_any_colour() {
        let glyph = tinted(32, [255, 255, 255]);
        assert!(glyph.chunks(4).any(|p| p[3] > 0));
        assert!(
            glyph
                .chunks(4)
                .filter(|p| p[3] > 0)
                .all(|p| p[..3] == [255, 255, 255])
        );
        assert_eq!(pixel(&glyph, 32, 1, 1)[3], 0, "nothing outside the bars");
    }

    #[test]
    #[ignore = "Writes the icon at several sizes for offline visual review"]
    fn export_icon() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-review");
        for size in [16u32, 32, 48, 256] {
            std::fs::write(dir.join(format!("icon-{size}.rgba")), pixels(size)).unwrap();
        }
        std::fs::write(dir.join("icon-glyph-32.rgba"), tinted(32, [255, 255, 255])).unwrap();
    }
}
