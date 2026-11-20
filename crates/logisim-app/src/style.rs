//! Visual tokens: theme resolution, spacing / type scale, hit targets and focus rings.
//! One place to change how the whole editor looks.

use crate::paint::ThemeMode;
use crate::prefs::ThemeChoice;
use eframe::egui::{self, Color32, CornerRadius, FontId, Stroke, TextStyle, Vec2};

/// Spacing scale (logical px): 4 / 8 / 12 / 16.
#[allow(dead_code)]
pub const SP1: f32 = 4.0;
pub const SP2: f32 = 8.0;
pub const SP3: f32 = 12.0;
pub const SP4: f32 = 16.0;
/// Smallest clickable control height (WCAG 2.2 target size, comfortable for a mouse).
pub const MIN_TARGET: f32 = 28.0;

/// Resolve "follow system" into a concrete theme.
pub fn resolve(choice: ThemeChoice, ctx: &egui::Context) -> ThemeMode {
    match choice {
        ThemeChoice::Light => ThemeMode::Light,
        ThemeChoice::Dark => ThemeMode::Dark,
        ThemeChoice::HighContrast => ThemeMode::HighContrast,
        ThemeChoice::System => match ctx.system_theme() {
            Some(egui::Theme::Light) => ThemeMode::Light,
            _ => ThemeMode::Dark,
        },
    }
}

/// Accent colour of a theme (selection, focus ring, primary actions).
pub fn accent(mode: ThemeMode) -> Color32 {
    match mode {
        ThemeMode::Dark => Color32::from_rgb(96, 165, 255),
        ThemeMode::Light => Color32::from_rgb(0, 102, 214),
        ThemeMode::HighContrast => Color32::from_rgb(255, 214, 0),
    }
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode, ui_scale: f32, font_scale: f32) {
    ctx.set_zoom_factor(ui_scale);
    // we pick light/dark ourselves (so "follow system" and high contrast are under our control)
    ctx.set_theme(if mode == ThemeMode::Light {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    });
    let acc = accent(mode);
    let mut v = if mode == ThemeMode::Light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    let (panel, window, input, w_idle, w_hover, w_active, text, line) = match mode {
        ThemeMode::Dark => (
            Color32::from_rgb(28, 31, 38),
            Color32::from_rgb(36, 40, 49),
            Color32::from_rgb(20, 22, 27),
            Color32::from_rgb(44, 49, 60),
            Color32::from_rgb(56, 63, 78),
            Color32::from_rgb(68, 77, 98),
            Color32::from_rgb(228, 231, 237),
            Color32::from_rgb(64, 70, 84),
        ),
        ThemeMode::Light => (
            Color32::from_rgb(244, 245, 248),
            Color32::from_rgb(252, 252, 254),
            Color32::from_rgb(255, 255, 255),
            Color32::from_rgb(230, 233, 240),
            Color32::from_rgb(216, 222, 234),
            Color32::from_rgb(200, 209, 228),
            Color32::from_rgb(28, 32, 42),
            Color32::from_rgb(200, 205, 216),
        ),
        ThemeMode::HighContrast => (
            Color32::BLACK,
            Color32::from_rgb(8, 8, 8),
            Color32::BLACK,
            Color32::from_rgb(24, 24, 24),
            Color32::from_rgb(48, 48, 48),
            Color32::from_rgb(80, 80, 80),
            Color32::WHITE,
            Color32::WHITE,
        ),
    };
    let edge = if mode == ThemeMode::HighContrast {
        1.5
    } else {
        1.0
    };
    v.panel_fill = panel;
    v.window_fill = window;
    v.extreme_bg_color = input;
    v.faint_bg_color = w_idle.gamma_multiply(0.5);
    v.override_text_color = Some(text);
    v.window_stroke = Stroke::new(edge, line);
    v.selection.bg_fill = acc.gamma_multiply(if mode == ThemeMode::HighContrast {
        0.45
    } else {
        0.35
    });
    // the keyboard focus ring is drawn with the selection stroke: make it unmistakable
    v.selection.stroke = Stroke::new(2.0, acc);
    v.hyperlink_color = acc;
    let r = CornerRadius::same(6);
    for (w, fill) in [
        (&mut v.widgets.noninteractive, panel),
        (&mut v.widgets.inactive, w_idle),
        (&mut v.widgets.hovered, w_hover),
        (&mut v.widgets.active, w_active),
        (&mut v.widgets.open, w_hover),
    ] {
        w.corner_radius = r;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::new(edge, line);
        w.fg_stroke = Stroke::new(
            if mode == ThemeMode::HighContrast {
                1.8
            } else {
                1.2
            },
            text,
        );
    }
    v.widgets.hovered.bg_stroke = Stroke::new(edge + 0.5, acc);
    v.widgets.active.bg_stroke = Stroke::new(edge + 0.5, acc);
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        let f = font_scale;
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(12.0 * f)),
            (TextStyle::Body, FontId::proportional(14.0 * f)),
            (TextStyle::Button, FontId::proportional(14.0 * f)),
            (TextStyle::Heading, FontId::proportional(17.0 * f)),
            (TextStyle::Monospace, FontId::monospace(13.0 * f)),
        ]
        .into();
        s.spacing.item_spacing = Vec2::new(SP2, 6.0);
        s.spacing.button_padding = Vec2::new(SP3, 5.0);
        s.spacing.interact_size = Vec2::new(36.0, MIN_TARGET);
        s.spacing.menu_margin = egui::Margin::same(SP2 as i8);
        s.spacing.window_margin = egui::Margin::same(SP3 as i8);
        s.spacing.indent = SP4;
        s.spacing.combo_height = 280.0;
        s.interaction.tooltip_delay = 0.35;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_tokens_are_consistent() {
        assert!(MIN_TARGET >= 24.0, "WCAG 2.2 minimum target size");
        assert_eq!([SP1, SP2, SP3, SP4], [4.0, 8.0, 12.0, 16.0]);
    }

    /// Relative luminance contrast ratio (WCAG).
    fn contrast(a: Color32, b: Color32) -> f32 {
        let l = |c: Color32| {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn canvas_text_and_accent_meet_contrast_targets() {
        for mode in [ThemeMode::Dark, ThemeMode::Light, ThemeMode::HighContrast] {
            let th = crate::paint::Theme::new(mode);
            assert!(contrast(th.text, th.bg) >= 7.0, "{mode:?} text on canvas");
            assert!(
                contrast(th.stroke, th.bg) >= 7.0,
                "{mode:?} strokes on canvas"
            );
            assert!(
                contrast(th.dim, th.bg) >= 4.5,
                "{mode:?} secondary text on canvas"
            );
            assert!(
                contrast(accent(mode), th.bg) >= 3.0,
                "{mode:?} accent vs canvas (non-text UI)"
            );
            assert!(contrast(th.v_err, th.bg) >= 3.0, "{mode:?} error colour");
        }
    }
}
