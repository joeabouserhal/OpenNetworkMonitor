/// Shared artwork for the window, tray, and executable resource. This file is also compiled by
/// build.rs, so it depends on nothing but the standard library.
///
/// The icon is the sidebar's mark without its tile: three rounded bars that fill the whole icon
/// so they read as large as any other app's icon in the taskbar, the middle one in the download
/// blue.
const BARS: [(f64, f64, f64, f64); 3] = [
    (1.0, 16.0, 9.0, 31.0),
    (12.0, 1.0, 20.0, 31.0),
    (23.0, 9.0, 31.0, 31.0),
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

/// The colour at a point of the 32-unit artwork, or `None` between the bars.
fn shade(x: f64, y: f64) -> Option<[f64; 3]> {
    BARS.iter()
        .enumerate()
        .find_map(|(index, &(x0, y0, x1, y1))| {
            let bar = distance(
                x,
                y,
                ((x0 + x1) / 2.0, (y0 + y1) / 2.0),
                ((x1 - x0) / 2.0, (y1 - y0) / 2.0),
                2.4,
            );
            (bar <= 0.0).then_some(if index == 1 { BLUE } else { WHITE })
        })
}

/// The full icon: the three bars, smooth at any size.
pub fn pixels(size: u32) -> Vec<u8> {
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

/// Only the three bars, in one colour, for the tray icon's accent and monochrome styles.
/// The build script needs just `pixels`, hence the allowance.
#[allow(dead_code)]
pub fn tinted(size: u32, [red, green, blue]: [u8; 3]) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let scale = f64::from(size) / 32.0;
    for y in 0..size {
        for x in 0..size {
            let left = f64::from(x);
            let top = f64::from(y);
            let coverage: f64 = BARS
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
    fn the_icon_is_three_large_bars_with_nothing_behind_them_at_every_size() {
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
            assert_eq!(at(0.0, 0.0)[3], 0, "{size}: no tile behind the bars");
            assert_eq!(at(10.5, 8.0)[3], 0, "{size}: the gap between bars is clear");
            assert!(at(5.0, 24.0)[0] > 200, "{size}: the first bar is light");
            let middle = at(16.0, 16.0);
            assert!(
                middle[2] > 200 && middle[0] < 100,
                "{size}: the middle bar is blue"
            );
            // The tallest bar runs the full height, so the icon fills its space.
            assert_eq!(
                at(16.0, 2.5)[3],
                255,
                "{size}: the middle bar reaches the top"
            );
            assert_eq!(at(16.0, 29.5)[3], 255, "{size}: and the bottom");
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
