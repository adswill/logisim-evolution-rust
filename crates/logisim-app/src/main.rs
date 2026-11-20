mod actions;
mod app;
mod canvas;
mod catalog;
mod dock;
mod examples;
mod fit;
mod fuzzy;
mod model;
mod nets;
mod numfmt;
mod overlays;
mod paint;
mod palette;
mod panels;
mod prefs;
mod sim;
mod style;

use eframe::egui;

fn main() -> eframe::Result {
    let mut file = None;
    let mut example = false;
    let mut dev = app::DevFlags::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--example" {
            example = true;
        } else if a == "--palette" {
            dev.palette = true;
        } else if a == "--shortcuts" {
            dev.shortcuts = true;
        } else if a == "--theme" {
            dev.theme = match args.next().as_deref() {
                Some("light") => Some(prefs::ThemeChoice::Light),
                Some("dark") => Some(prefs::ThemeChoice::Dark),
                Some("hc") => Some(prefs::ThemeChoice::HighContrast),
                _ => None,
            };
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
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, file, example, &dev)))),
    )
}
