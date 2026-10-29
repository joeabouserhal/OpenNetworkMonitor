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
fn request_exit(ctx: &egui::Context, exit: &AtomicBool) {
    exit.store(true, Ordering::Release);
    // eframe saves state and joins the worker before destroying its window.
    // Hide it first so those necessary final writes don't leave it on screen.
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
}
