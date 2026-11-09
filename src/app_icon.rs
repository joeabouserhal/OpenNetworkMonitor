/// Shared artwork for the window, tray, and executable resource.
/// At 32px this preserves the original runtime icon pixel for pixel.
pub fn pixels(size: u32) -> Vec<u8> {
    tinted(size, [76, 199, 190])
}

/// The same artwork in another colour.
pub fn tinted(size: u32, [red, green, blue]: [u8; 3]) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let scale = f64::from(size) / 32.0;
    let bars = [
        (6.0, 17.0, 11.0, 26.0),
        (14.0, 7.0, 19.0, 26.0),
        (22.0, 12.0, 27.0, 26.0),
    ];
    for y in 0..size {
        for x in 0..size {
            let left = f64::from(x);
            let top = f64::from(y);
            let coverage: f64 = bars
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

    #[test]
    fn the_artwork_keeps_its_shape_in_any_colour() {
        let teal = pixels(32);
        let white = tinted(32, [255, 255, 255]);
        assert_eq!(teal.len(), white.len());
        for (a, b) in teal.chunks(4).zip(white.chunks(4)) {
            assert_eq!(a[3], b[3], "the same pixels are covered");
            if b[3] > 0 {
                assert_eq!(&b[..3], &[255, 255, 255]);
            }
        }
        assert!(teal.chunks(4).any(|p| p[3] > 0));
    }
}
