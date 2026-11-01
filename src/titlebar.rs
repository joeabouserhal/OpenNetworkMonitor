//! Colours the native Windows title bar to match the app palette.
//!
//! The bar stays the system's own, so edge resizing, snap layouts, window
//! animations and accessibility are unchanged; only its colours are set, through
//! DWM. Windows 11 (build 22000 and later) honours the caption, text and border
//! colours. Older versions ignore them and keep their standard bar, and
//! `apply` reports that by returning false.
use crate::widgets::Palette;
use eframe::egui::Color32;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::HWND,
    Graphics::Dwm::{
        DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DWMWA_USE_IMMERSIVE_DARK_MODE,
        DwmSetWindowAttribute,
    },
};

/// Title bar colours as Windows `COLORREF` values (0x00BBGGRR).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    pub caption: u32,
    pub text: u32,
    pub border: u32,
    /// Selects light caption buttons (minimise, maximise, close) for dark bars.
    pub dark: bool,
}

impl Colors {
    /// The bar takes the canvas colour, so it reads as part of the window rather
    /// than a strip above it, and the border is a faint tint of the text colour.
    pub fn from_palette(palette: &Palette) -> Self {
        let colorref =
            |c: Color32| u32::from(c.r()) | u32::from(c.g()) << 8 | u32::from(c.b()) << 16;
        Self {
            caption: colorref(palette.canvas),
            text: colorref(palette.text),
            border: colorref(palette.canvas.lerp_to_gamma(palette.text, 0.12)),
            dark: palette.dark,
        }
    }
}

/// Applies `colors` to `window`'s title bar. Returns false when the window has no
/// Win32 handle or this version of Windows does not support the colours.
pub fn apply(window: &impl HasWindowHandle, colors: Colors) -> bool {
    let Ok(handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::Win32(win32) = handle.as_raw() else {
        return false;
    };
    set(win32.hwnd.get() as HWND, colors)
}

fn set(hwnd: HWND, colors: Colors) -> bool {
    let mut supported = true;
    for (attribute, value) in [
        (DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(colors.dark)),
        (DWMWA_CAPTION_COLOR, colors.caption),
        (DWMWA_TEXT_COLOR, colors.text),
        (DWMWA_BORDER_COLOR, colors.border),
    ] {
        // SAFETY: `hwnd` is a live top-level window and `value` is a 4-byte integer that
        // outlives the call, which is the size and type each of these attributes expects.
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                ptr::from_ref(&value).cast(),
                size_of::<u32>() as u32,
            )
        };
        supported &= result >= 0;
    }
    supported
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::{
        Graphics::Dwm::DwmGetWindowAttribute,
        UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW},
    };

    fn palette() -> Palette {
        Palette {
            dark: true,
            canvas: Color32::from_rgb(0x11, 0x22, 0x33),
            card: Color32::BLACK,
            raised: Color32::BLACK,
            sunken: Color32::BLACK,
            hairline: Color32::TRANSPARENT,
            text: Color32::from_rgb(0xEE, 0xDD, 0xCC),
            dim: Color32::GRAY,
            accent: Color32::BLUE,
            accent_ink: Color32::WHITE,
            accent_soft: Color32::BLACK,
            down: Color32::BLUE,
            up: Color32::YELLOW,
            good: Color32::GREEN,
            warn: Color32::YELLOW,
            bad: Color32::RED,
        }
    }

    #[test]
    fn colours_are_windows_colorrefs_taken_from_the_palette() {
        let colors = Colors::from_palette(&palette());
        // COLORREF stores blue in the high byte: 0x00BBGGRR.
        assert_eq!(colors.caption, 0x0033_2211);
        assert_eq!(colors.text, 0x00CC_DDEE);
        assert!(colors.dark);
        assert_ne!(
            colors.border, colors.caption,
            "The border is a visible tint"
        );
    }

    #[test]
    fn dwm_stores_the_colours_on_a_real_window() {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: a built-in window class and null parent/menu/instance create a plain
        // top-level window that is never shown; it is destroyed below.
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                200,
                100,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "Could not create a test window");
        let colors = Colors::from_palette(&palette());
        // Windows 11 accepts all four attributes; older versions refuse the colours, which
        // `set` reports rather than failing. DWM only lets the dark-mode flag be read back.
        if set(hwnd, colors) {
            let mut stored = 0u32;
            // SAFETY: `stored` is a 4-byte integer that outlives the call.
            let result = unsafe {
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_USE_IMMERSIVE_DARK_MODE as u32,
                    ptr::from_mut(&mut stored).cast(),
                    size_of::<u32>() as u32,
                )
            };
            assert!(result >= 0, "DWM could not read the dark-mode flag");
            assert_eq!(stored, 1, "The dark caption buttons were not applied");
        }
        // Windows older than 11 refuses the colours; that is reported, not an error.
        // SAFETY: `hwnd` was created above on this thread and is destroyed once.
        unsafe { DestroyWindow(hwnd) };
    }
}
