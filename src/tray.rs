use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::Duration,
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{IsWindow, PostMessageW, WM_MOUSEMOVE},
};

pub struct Tray {
    pub _icon: TrayIcon,
    pub exit: Arc<AtomicBool>,
}
pub fn icon_pixels() -> Vec<u8> {
    crate::app_icon::pixels(32)
}

impl Tray {
    pub fn new(ctx: egui::Context) -> anyhow::Result<Self> {
        let menu = Menu::new();
        let show = MenuItem::new("Open OpenNetworkMonitor", true, None);
        let exit_item = MenuItem::new("Exit", true, None);
        menu.append(&show)?;
        menu.append(&exit_item)?;
        let exit = Arc::new(AtomicBool::new(false));
        let flag = exit.clone();
        let wake = ctx.clone();
        let show_id = show.id().clone();
        let exit_id = exit_item.id().clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == exit_id {
                request_exit(&wake, &flag);
            } else if event.id == show_id {
                show_window(&wake, &flag);
            }
            wake.request_repaint();
        }));
        let icon_exit = exit.clone();
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(&ctx, &icon_exit);
            }
        }));
        let icon = tray_icon::Icon::from_rgba(icon_pixels(), 32, 32)?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("OpenNetworkMonitor · monitoring in background")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()?;
        Ok(Self { _icon: tray, exit })
    }
}
/// The main window, for the wake-ups below. Zero until `remember_window` is called.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

pub fn remember_window(window: &impl HasWindowHandle) {
    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::Win32(win32) = handle.as_raw()
    {
        MAIN_WINDOW.store(win32.hwnd.get(), Ordering::Release);
    }
}

pub fn request_exit(ctx: &egui::Context, exit: &AtomicBool) {
    exit.store(true, Ordering::Release);
    // eframe saves state and joins the worker before destroying its window.
    // Hide it first so those necessary final writes don't leave it on screen.
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    wake_until_closed();
}

/// After a choice in the tray menu the event loop can stop drawing frames until the
/// window gets an input event such as focus, so a queued exit would wait for the user
/// to click the window. Posting mouse moves keeps the loop turning until the exit has
/// been carried out. The position changes every time because an unchanged one is
/// ignored.
fn wake_until_closed() {
    let hwnd = MAIN_WINDOW.load(Ordering::Acquire);
    if hwnd == 0 {
        return;
    }
    std::thread::spawn(move || {
        for step in 0..100isize {
            std::thread::sleep(Duration::from_millis(100));
            // SAFETY: IsWindow accepts any handle value, and posting a message to a window
            // that is closing is harmless.
            unsafe {
                if IsWindow(hwnd as HWND) == 0 {
                    return;
                }
                let at = 20 + step % 40;
                PostMessageW(hwnd as HWND, WM_MOUSEMOVE, 0, (at << 16) | at);
            }
        }
    });
}
fn show_window(ctx: &egui::Context, exit: &AtomicBool) {
    if exit.load(Ordering::Acquire) {
        return;
    }
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_exit_hides_before_closing_and_prevents_reopening() {
        let ctx = egui::Context::default();
        let exit = AtomicBool::new(false);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            request_exit(ui.ctx(), &exit);
            show_window(ui.ctx(), &exit);
        });
        assert!(exit.load(Ordering::Acquire));
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        let hide = commands
            .iter()
            .position(|cmd| *cmd == egui::ViewportCommand::Visible(false))
            .unwrap();
        let close = commands
            .iter()
            .position(|cmd| *cmd == egui::ViewportCommand::Close)
            .unwrap();
        assert!(hide < close);
        assert!(!commands.contains(&egui::ViewportCommand::Visible(true)));
        output.textures_delta.clear();
    }

    #[test]
    fn tray_open_still_shows_a_running_app() {
        let ctx = egui::Context::default();
        let exit = AtomicBool::new(false);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show_window(ui.ctx(), &exit));
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands.contains(&egui::ViewportCommand::Visible(true)));
        assert!(!commands.contains(&egui::ViewportCommand::Close));
        output.textures_delta.clear();
    }

    #[test]
    fn an_exit_request_keeps_waking_the_main_window_until_it_closes() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, MSG, PM_REMOVE, PeekMessageW, WS_OVERLAPPEDWINDOW,
        };
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
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
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null());
        MAIN_WINDOW.store(hwnd as isize, Ordering::Release);
        let ctx = egui::Context::default();
        let exit = AtomicBool::new(false);
        request_exit(&ctx, &exit);
        let mut positions = std::collections::BTreeSet::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while positions.len() < 2 && std::time::Instant::now() < deadline {
            // SAFETY: MSG is plain data, filled in by the call.
            let mut message: MSG = unsafe { std::mem::zeroed() };
            // SAFETY: peeks this thread's own queue for messages posted to its window.
            if unsafe { PeekMessageW(&mut message, hwnd, WM_MOUSEMOVE, WM_MOUSEMOVE, PM_REMOVE) }
                != 0
            {
                positions.insert(message.lParam);
            } else {
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        MAIN_WINDOW.store(0, Ordering::Release);
        // SAFETY: `hwnd` was created above on this thread and is destroyed once.
        unsafe { DestroyWindow(hwnd) };
        assert!(
            positions.len() >= 2,
            "The window should keep receiving mouse moves at changing positions: {positions:?}"
        );
    }
}
