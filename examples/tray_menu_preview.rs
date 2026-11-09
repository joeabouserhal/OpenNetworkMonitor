//! Opens the tray menu near the bottom right of the screen for a few seconds, for a screenshot.
//! `cargo run --example tray_menu_preview [light]`
use open_network_monitor::{
    tray_menu::{self, Palette},
    ui::tokens::{BaseColor, Scheme, Tokens},
};
use windows_sys::Win32::UI::{
    HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
    WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, GetSystemMetrics, MSG, SM_CXSCREEN, SM_CYSCREEN,
        TranslateMessage,
    },
};

fn main() {
    let scheme = if std::env::args().any(|arg| arg == "light") {
        Scheme::Light
    } else {
        Scheme::Dark
    };
    // SAFETY: plain Win32 calls made once at startup on the main thread.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        tray_menu::set_palette(Palette::from_tokens(&Tokens::new(scheme, BaseColor::Zinc)));
        tray_menu::set_handler(|action| println!("{action:?}"));
        let (width, height) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        tray_menu::open(((width - 60) as f64, (height - 20) as f64), false);
        let started = std::time::Instant::now();
        let mut message: MSG = std::mem::zeroed();
        while started.elapsed().as_secs() < 8
            && GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0
        {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
