mod app;
mod examples;
mod model;
mod numfmt;
mod paint;
mod sim;

use eframe::egui;

fn main() -> eframe::Result {
    let mut file = None;
    let mut example = false;
    for a in std::env::args().skip(1) {
        if a == "--example" {
            example = true;
        } else {
            file = Some(std::path::PathBuf::from(a));
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_title("rcirc"),
        ..Default::default()
    };
    eframe::run_native(
        "rcirc",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, file, example)))),
    )
}
