use eframe::egui;
use open_network_monitor::{app::MonitorApp, widgets};
struct Preview(MonitorApp);
impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.0.refresh_preview();
        eframe::App::ui(&mut self.0, ui, frame);
    }
}
fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args().collect();
    let page = args.get(1).cloned().unwrap_or_else(|| "apps".into());
    let small = args.iter().any(|a| a == "--small");
    let light = args.iter().any(|a| a == "--light");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenNetworkMonitor · UI preview")
            .with_inner_size(if small {
                widgets::MIN_WINDOW_SIZE
            } else {
                widgets::DEFAULT_WINDOW_SIZE
            })
            .with_min_inner_size([960.0, 680.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "OpenNetworkMonitor UI preview",
        options,
        Box::new(move |cc| {
            Ok(Box::new(Preview(MonitorApp::preview(
                &cc.egui_ctx,
                &page,
                light,
            ))))
        }),
    )
}
