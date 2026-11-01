//! Keeps a collapsed window from being remembered.
//!
//! eframe saves the window's size and position when the app exits. A minimized
//! Windows window reports a 0x0 client area at (-32000, -32000), so exiting while
//! minimized saved exactly that. The next launch then clamped it to a 64-point
//! window, ignored the 960x680 minimum and saved the 64-point size again, so every
//! start was tiny until the window was resized by hand. These helpers recognise such
//! records so they can be repaired before launch and never written back.
use std::{fs, path::Path};

/// Smaller than this in either direction cannot be a window the app was used in.
const SMALLEST_REAL_POINTS: f32 = 200.0;
/// Positions this far off-screen are Windows' parking spot for minimized windows.
const FARTHEST_REAL_PIXELS: f32 = -10_000.0;

/// A record with no remembered size or position: the app opens at its default size.
pub const DEFAULT_RECORD: &str = "(inner_position_pixels:None,outer_position_pixels:None,fullscreen:false,maximized:false,inner_size_points:None)";

const KEY: &str = "window";

/// Reads the `(x, y)` pair of `Some((x:..,y:..))` fields in eframe's RON window record.
fn pair(record: &str, field: &str) -> Option<(f32, f32)> {
    let rest = record.split_once(&format!("{field}:Some(("))?.1;
    let (x, rest) = rest.strip_prefix("x:")?.split_once(",y:")?;
    let (y, _) = rest.split_once(')')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// Whether `record` describes a window worth restoring. A field that is absent or
/// cannot be read is not a reason to reject the record.
pub fn is_sane(record: &str) -> bool {
    let size = pair(record, "inner_size_points")
        .is_none_or(|(w, h)| w >= SMALLEST_REAL_POINTS && h >= SMALLEST_REAL_POINTS);
    let placed = ["inner_position_pixels", "outer_position_pixels"]
        .into_iter()
        .all(|field| {
            pair(record, field)
                .is_none_or(|(x, y)| x > FARTHEST_REAL_PIXELS && y > FARTHEST_REAL_PIXELS)
        });
    size && placed
}

/// Replaces a collapsed window record in eframe's preferences file, returning
/// whether it rewrote the file. Best effort: anything unexpected leaves it alone.
pub fn repair_file(path: &Path) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    let Some(key) = text.find(&format!("\"{KEY}\"")) else {
        return false;
    };
    let after_key = key + KEY.len() + 2;
    let Some(open) = text[after_key..].find('"').map(|i| after_key + i + 1) else {
        return false;
    };
    let Some(len) = text[open..].find('"') else {
        return false;
    };
    if is_sane(&text[open..open + len]) {
        return false;
    }
    let repaired = format!("{}{DEFAULT_RECORD}{}", &text[..open], &text[open + len..]);
    let temporary = path.with_extension("ron.tmp");
    fs::write(&temporary, repaired).is_ok() && fs::rename(&temporary, path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEALTHY: &str = "(inner_position_pixels:Some((x:294.0,y:317.0)),outer_position_pixels:Some((x:286.0,y:286.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:1600.0,y:911.11115)))";
    /// What eframe saved when the window was minimized at exit.
    const MINIMIZED: &str = "(inner_position_pixels:Some((x:-32000.0,y:-32000.0)),outer_position_pixels:Some((x:-32000.0,y:-32000.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:0.0,y:0.0)))";
    /// What the next launch saved after clamping that to egui's 64-point floor.
    const CLAMPED: &str = "(inner_position_pixels:Some((x:8.0,y:31.0)),outer_position_pixels:Some((x:0.0,y:0.0)),fullscreen:false,maximized:false,inner_size_points:Some((x:64.44444,y:64.44444)))";

    #[test]
    fn collapsed_records_are_rejected_and_real_ones_kept() {
        assert!(is_sane(HEALTHY));
        assert!(is_sane(DEFAULT_RECORD));
        assert!(is_sane(
            "(inner_position_pixels:Some((x:-1900.0,y:40.0)),outer_position_pixels:Some((x:-1908.0,y:9.0)),fullscreen:false,maximized:true,inner_size_points:Some((x:960.0,y:680.0)))"
        ));
        assert!(!is_sane(MINIMIZED));
        assert!(!is_sane(CLAMPED));
        assert!(!is_sane(
            "(inner_position_pixels:None,outer_position_pixels:None,fullscreen:false,maximized:false,inner_size_points:Some((x:900.0,y:20.0)))"
        ));
        assert!(!is_sane(
            "(inner_position_pixels:Some((x:-32000.0,y:-32000.0)),outer_position_pixels:None,fullscreen:false,maximized:false,inner_size_points:Some((x:1600.0,y:900.0)))"
        ));
        assert!(
            is_sane("not a window record"),
            "Unreadable records are left to eframe"
        );
    }

    fn preferences(record: &str) -> String {
        format!(
            "{{\n    \"window\": \"{record}\",\n    \"egui\": \"(options:(zoom_factor:0.9))\",\n    \"settings\": \"(theme:Dark)\",\n}}"
        )
    }

    #[test]
    fn a_collapsed_record_is_repaired_without_touching_other_settings() {
        let dir = tempfile::tempdir().unwrap();
        for bad in [MINIMIZED, CLAMPED] {
            let path = dir.path().join("preferences.ron");
            fs::write(&path, preferences(bad)).unwrap();
            assert!(repair_file(&path));
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                preferences(DEFAULT_RECORD)
            );
            assert!(
                !path.with_extension("ron.tmp").exists(),
                "No temporary file is left behind"
            );
            assert!(!repair_file(&path), "A repaired file is stable");
        }
    }

    #[test]
    fn healthy_missing_and_unreadable_files_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preferences.ron");
        assert!(!repair_file(&path), "Missing file");
        for text in [
            preferences(HEALTHY),
            "garbage".to_owned(),
            "{\"window\"".to_owned(),
            "{\"egui\": \"()\"}".to_owned(),
        ] {
            fs::write(&path, &text).unwrap();
            assert!(!repair_file(&path), "{text}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
    }
}
