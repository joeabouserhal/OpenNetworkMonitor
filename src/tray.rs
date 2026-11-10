use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicIsize, Ordering},
    },
    time::Duration,
};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    Graphics::Gdi::{CreateBitmap, DeleteObject, HGDIOBJ},
    UI::{
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::{
            CreateIconIndirect, ICON_SMALL, ICONINFO, IsWindow, PostMessageW, SendMessageW,
            WM_ENDSESSION, WM_GETICON, WM_MOUSEMOVE, WM_QUERYENDSESSION, WM_SETICON,
        },
    },
};

pub struct Tray {
    icon: TrayIcon,
    pub exit: Arc<AtomicBool>,
    /// Whether the user has paused recording from the menu.
    pub paused: Arc<AtomicBool>,
    /// Set by the menu; the window takes it, goes to Settings and clears it.
    pub open_settings: Arc<AtomicBool>,
    last_tooltip: std::cell::RefCell<String>,
    last_color: std::cell::Cell<Option<[u8; 3]>>,
    last_color_set: std::cell::Cell<bool>,
}
pub fn icon_pixels() -> Vec<u8> {
    crate::app_icon::pixels(32)
}
/// The app icon at another size, for the window, which Windows scales to the taskbar.
pub fn icon_pixels_at(size: u32) -> Vec<u8> {
    crate::app_icon::pixels(size)
}

impl Tray {
    pub fn new(ctx: egui::Context) -> anyhow::Result<Self> {
        let exit = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(false));
        let open_settings = Arc::new(AtomicBool::new(false));
        let (icon_exit, icon_paused) = (exit.clone(), paused.clone());
        let (menu_exit, menu_paused, menu_settings, menu_ctx) = (
            exit.clone(),
            paused.clone(),
            open_settings.clone(),
            ctx.clone(),
        );
        crate::tray_menu::set_handler(move |action| {
            use crate::tray_menu::Action;
            match action {
                Action::Open => show_window(&menu_ctx, &menu_exit),
                Action::Settings => {
                    menu_settings.store(true, Ordering::Release);
                    show_window(&menu_ctx, &menu_exit);
                }
                // The window sees the change and tells the worker.
                Action::TogglePause => {
                    menu_paused.fetch_xor(true, Ordering::AcqRel);
                }
                Action::Exit => request_exit(&menu_ctx, &menu_exit),
            }
            menu_ctx.request_repaint();
        });
        TrayIconEvent::set_event_handler(Some(move |event| match event {
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => show_window(&ctx, &icon_exit),
            // The menu is a small window of its own, placed where the click was.
            TrayIconEvent::Click {
                button: MouseButton::Right,
                button_state: MouseButtonState::Up,
                position,
                ..
            } => {
                crate::tray_menu::open(
                    (position.x, position.y),
                    icon_paused.load(Ordering::Acquire),
                );
            }
            _ => {}
        }));
        let icon = tray_icon::Icon::from_rgba(icon_pixels(), 32, 32)?;
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip("OpenNetworkMonitor · monitoring in background")
            .build()?;
        Ok(Self {
            icon: tray,
            exit,
            paused,
            open_settings,
            last_tooltip: std::cell::RefCell::new(String::new()),
            last_color: std::cell::Cell::new(None),
            last_color_set: std::cell::Cell::new(false),
        })
    }
    /// Draws the icon in `color`, or in its own colours when that is `None`. Unchanged colours
    /// are not sent to Windows again.
    pub fn set_color(&self, color: Option<[u8; 3]>) {
        if self.last_color.get() == color && self.last_color_set.get() {
            return;
        }
        let pixels = match color {
            Some(rgb) => crate::app_icon::tinted(32, rgb),
            None => icon_pixels(),
        };
        if let Ok(icon) = tray_icon::Icon::from_rgba(pixels, 32, 32) {
            let _ = self.icon.set_icon(Some(icon));
            self.last_color.set(color);
            self.last_color_set.set(true);
        }
    }
    /// Shows `text` when the pointer rests on the tray icon. Unchanged text is not
    /// sent to Windows again.
    pub fn set_tooltip(&self, text: &str) {
        if *self.last_tooltip.borrow() != text {
            let _ = self.icon.set_tooltip(Some(text));
            *self.last_tooltip.borrow_mut() = text.to_owned();
        }
    }
}
/// Whether the Windows taskbar, where the tray icon sits, is using its dark theme.
pub fn taskbar_is_dark() -> bool {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
    };
    let key: Vec<u16> = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = "SystemUsesLightTheme"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    let mut light = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: reads one DWORD into a u32 with its byte size; both names are nul-terminated and
    // outlive the call, and nothing is written to the registry.
    let code = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut light).cast(),
            &mut size,
        )
    };
    // Dark is Windows' default when the setting is missing.
    code != ERROR_SUCCESS || light == 0
}

/// The main window, for the wake-ups below. Zero until `remember_window` is called.
static MAIN_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Gives the window's small icon, the one in its title bar, just the three bars in one colour
/// that shows on the title bar, while the taskbar keeps the full tile. Cheap to call every frame.
pub fn set_caption_icon(dark_title_bar: bool) {
    static ICONS: std::sync::Mutex<[isize; 2]> = std::sync::Mutex::new([0; 2]);
    let hwnd = MAIN_WINDOW.load(Ordering::Acquire);
    let Ok(mut icons) = ICONS.lock() else {
        return;
    };
    let slot = usize::from(dark_title_bar);
    if hwnd == 0 {
        return;
    }
    if icons[slot] == 0 {
        const SIZE: u32 = 32;
        let outer = if dark_title_bar {
            [0xfa, 0xfa, 0xfa]
        } else {
            [0x18, 0x18, 0x1b]
        };
        // Windows wants blue, green, red, alpha, and a mask it ignores for 32-bit icons.
        let bgra: Vec<u8> = crate::app_icon::mark(SIZE, outer)
            .chunks(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect();
        // SAFETY: the bitmaps copy the pixel data, which is SIZE x SIZE x 4 bytes, before the call
        // returns; the icon owns them afterwards and lives for the rest of the process.
        let icon = unsafe {
            let color = CreateBitmap(SIZE as i32, SIZE as i32, 1, 32, bgra.as_ptr().cast());
            let mask = CreateBitmap(SIZE as i32, SIZE as i32, 1, 1, std::ptr::null());
            let info = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: mask,
                hbmColor: color,
            };
            let icon = CreateIconIndirect(&info);
            // The icon keeps its own copies of the bitmaps.
            DeleteObject(color as HGDIOBJ);
            DeleteObject(mask as HGDIOBJ);
            icon
        };
        icons[slot] = icon as isize;
    }
    // The window setup can put its own icon back after this has run, so it is checked every frame
    // and only sent again when it differs.
    // SAFETY: the handles are the main window and an icon created above, both still alive.
    let current = unsafe { SendMessageW(hwnd as HWND, WM_GETICON, ICON_SMALL as WPARAM, 0) };
    if icons[slot] != 0 && current != icons[slot] {
        // SAFETY: as above.
        unsafe {
            SendMessageW(
                hwnd as HWND,
                WM_SETICON,
                ICON_SMALL as WPARAM,
                icons[slot] as LPARAM,
            );
        }
    }
}

pub fn remember_window(window: &impl HasWindowHandle) {
    if let Ok(handle) = window.window_handle()
        && let RawWindowHandle::Win32(win32) = handle.as_raw()
    {
        MAIN_WINDOW.store(win32.hwnd.get(), Ordering::Release);
        watch_session_end(win32.hwnd.get() as HWND);
    }
}

/// What to do when Windows is shutting down or signing the user out.
static SESSION_END: std::sync::OnceLock<Box<dyn Fn(bool) + Send + Sync>> =
    std::sync::OnceLock::new();
/// Sets what runs when the session ends: it must save everything that is waiting, quickly. The
/// flag says whether the session really is ending, rather than being asked about.
pub fn on_session_end(save: impl Fn(bool) + Send + Sync + 'static) {
    let _ = SESSION_END.set(Box::new(save));
}
const SESSION_HOOK: usize = 0x4f4e4d;

/// Windows ends the session by telling each window, then terminating the process; the app's
/// normal exit never runs. This hears the notice first, so what is waiting is saved in time.
fn watch_session_end(window: HWND) {
    // SAFETY: subclasses a window of this process on its own thread; the procedure below only
    // forwards to the next one and runs the saved callback.
    unsafe {
        SetWindowSubclass(window, Some(session_procedure), SESSION_HOOK, 0);
    }
}

/// # Safety
/// Called only by Windows, through the window's subclass chain.
unsafe extern "system" fn session_procedure(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // The query comes first and may be refused by another program; the end notice carries a
    // flag saying whether the session really is ending. Saving on both is harmless, since the
    // second finds little left to save.
    let confirmed = message == WM_ENDSESSION && wparam != 0;
    if (message == WM_QUERYENDSESSION || confirmed)
        && let Some(save) = SESSION_END.get()
    {
        save(confirmed);
    }
    // SAFETY: passes Windows' own arguments to the rest of the chain, which allows the shutdown.
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
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
    fn ending_the_windows_session_saves_first_and_lets_it_end() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW,
        };
        static SAVES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        static CONFIRMED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        on_session_end(|ending| {
            SAVES.fetch_add(1, Ordering::SeqCst);
            if ending {
                CONFIRMED.fetch_add(1, Ordering::SeqCst);
            }
        });
        let class: Vec<u16> = "STATIC".encode_utf16().chain(Some(0)).collect();
        // SAFETY: a built-in window class and null parent/menu/instance create a plain top-level
        // window that is never shown; it is destroyed below.
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
        watch_session_end(hwnd);
        // SAFETY: sends the messages Windows sends, to this thread's own window.
        unsafe {
            // Asked whether the session may end: it saves, and does not object.
            let answer = SendMessageW(hwnd, WM_QUERYENDSESSION, 0, 0);
            assert_eq!(answer, 1, "the shutdown is not held up");
            assert_eq!(SAVES.load(Ordering::SeqCst), 1);
            assert_eq!(
                CONFIRMED.load(Ordering::SeqCst),
                0,
                "only asked, not ending yet"
            );
            // Told the session is ending: it saves again.
            SendMessageW(hwnd, WM_ENDSESSION, 1, 0);
            assert_eq!(SAVES.load(Ordering::SeqCst), 2);
            assert_eq!(CONFIRMED.load(Ordering::SeqCst), 1);
            // Told it was cancelled: nothing to do.
            SendMessageW(hwnd, WM_ENDSESSION, 0, 0);
            assert_eq!(SAVES.load(Ordering::SeqCst), 2);
            DestroyWindow(hwnd);
        }
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
