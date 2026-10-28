use eframe::egui;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tray_icon::{
    MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem},
};

pub struct Tray {
    pub _icon: TrayIcon,
    pub exit: Arc<AtomicBool>,
}
pub fn icon_pixels() -> Vec<u8> {
    let mut rgba = vec![0u8; 32 * 32 * 4];
    for y in 0..32 {
        for x in 0..32 {
            let i = (y * 32 + x) * 4;
            let bar = (6..11).contains(&x) && (17..26).contains(&y)
                || (14..19).contains(&x) && (7..26).contains(&y)
                || (22..27).contains(&x) && (12..26).contains(&y);
            if bar {
                rgba[i..i + 4].copy_from_slice(&[76, 199, 190, 255]);
            }
        }
    }
    rgba
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
                flag.store(true, Ordering::Relaxed);
                wake.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if event.id == show_id {
                show_window(&wake);
            }
            wake.request_repaint();
        }));
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(&ctx);
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
fn show_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    ctx.request_repaint();
}
