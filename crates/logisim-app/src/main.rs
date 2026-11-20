use eframe::egui;

struct App {
    offset: egui::Vec2,
    zoom: f32,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::new()
            .fill(egui::Color32::from_rgb(24, 26, 31))
            .show(ui, |ui| {
                let (resp, painter) =
                    ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
                if resp.dragged() {
                    self.offset += resp.drag_delta();
                }
                if resp.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        let old = self.zoom;
                        self.zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.2, 8.0);
                        if let Some(p) = resp.hover_pos() {
                            let c = p - resp.rect.min;
                            self.offset = c - (c - self.offset) * (self.zoom / old);
                        }
                    }
                }
                let step = 20.0 * self.zoom;
                let line = egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 48, 56));
                let r = resp.rect;
                let mut x = r.min.x + self.offset.x.rem_euclid(step);
                while x < r.max.x {
                    painter.vline(x, r.y_range(), line);
                    x += step;
                }
                let mut y = r.min.y + self.offset.y.rem_euclid(step);
                while y < r.max.y {
                    painter.hline(r.x_range(), y, line);
                    y += step;
                }
            });
    }
}

fn main() -> eframe::Result {
    eframe::run_native(
        "rcirc",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(App { offset: egui::Vec2::ZERO, zoom: 1.0 }))),
    )
}
