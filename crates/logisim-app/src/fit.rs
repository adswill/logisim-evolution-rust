//! "Fit circuit to window": pure geometry, so it is easy to test.

use crate::paint::View;
use eframe::egui::Rect;

pub const MIN_ZOOM: f32 = 0.25;

/// A view that centres `bounds` (world coordinates) in `canvas` (screen) with `margin` px around.
/// Tiny circuits are not blown up beyond `max_zoom`.
pub fn fit_view(bounds: Rect, canvas: Rect, margin: f32, max_zoom: f32) -> View {
    let bw = bounds.width().max(1.0);
    let bh = bounds.height().max(1.0);
    let zx = (canvas.width() - 2.0 * margin).max(1.0) / bw;
    let zy = (canvas.height() - 2.0 * margin).max(1.0) / bh;
    let zoom = zx.min(zy).min(max_zoom).max(MIN_ZOOM);
    let origin = canvas.center().to_vec2() - bounds.center().to_vec2() * zoom;
    View { origin, zoom }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Pos2, Vec2};

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
    }

    #[test]
    fn content_is_centred_and_fits() {
        let canvas = rect(300.0, 100.0, 1100.0, 700.0);
        let bounds = rect(100.0, 80.0, 500.0, 280.0);
        let v = fit_view(bounds, canvas, 40.0, 4.0);
        let tl = v.to_screen(bounds.min.x, bounds.min.y);
        let br = v.to_screen(bounds.max.x, bounds.max.y);
        assert!(tl.x >= canvas.min.x + 39.0 && br.x <= canvas.max.x - 39.0);
        assert!(tl.y >= canvas.min.y + 39.0 && br.y <= canvas.max.y - 39.0);
        let centre = v.to_screen(bounds.center().x, bounds.center().y);
        assert!((centre - canvas.center()).length() < 0.5);
    }

    #[test]
    fn small_content_is_capped_and_huge_content_is_floored() {
        let canvas = rect(0.0, 0.0, 1000.0, 800.0);
        assert_eq!(
            fit_view(rect(0.0, 0.0, 10.0, 10.0), canvas, 40.0, 1.5).zoom,
            1.5
        );
        assert_eq!(
            fit_view(rect(0.0, 0.0, 100000.0, 100000.0), canvas, 40.0, 1.5).zoom,
            MIN_ZOOM
        );
    }

    #[test]
    fn degenerate_bounds_do_not_panic() {
        let v = fit_view(
            Rect::from_min_size(Pos2::new(5.0, 5.0), Vec2::ZERO),
            rect(0.0, 0.0, 100.0, 100.0),
            200.0,
            2.0,
        );
        assert!(v.zoom.is_finite() && v.origin.x.is_finite());
    }
}
