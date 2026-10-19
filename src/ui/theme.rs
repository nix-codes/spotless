//! The skin's colours and the Winamp-style drawing helpers.
//!
//! There is one built-in palette, Classic, for now.

use egui::{Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, pos2};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub desk: Color32,
    pub frame: Color32,
    /// The light and dark edges of a bevel.
    pub hi: Color32,
    pub lo: Color32,
    /// Chrome text: button labels, the title bar.
    pub chrome: Color32,
    pub button: Color32,
    pub lcd_bg: Color32,
    pub lcd: Color32,
    pub list_bg: Color32,
    pub list_fg: Color32,
    pub current: Color32,
    pub selected_bg: Color32,
    pub dim: Color32,
    pub accent: Color32,
}

impl Palette {
    pub const CLASSIC: Self = Self {
        desk: Color32::from_rgb(0x0b, 0x0b, 0x0f),
        frame: Color32::from_rgb(0x2a, 0x2a, 0x37),
        hi: Color32::from_rgb(0x4c, 0x4c, 0x62),
        lo: Color32::from_rgb(0x11, 0x11, 0x17),
        chrome: Color32::from_rgb(0xc9, 0xc9, 0xda),
        button: Color32::from_rgb(0x36, 0x36, 0x46),
        lcd_bg: Color32::BLACK,
        lcd: Color32::from_rgb(0x2b, 0xff, 0x62),
        list_bg: Color32::BLACK,
        list_fg: Color32::from_rgb(0x00, 0xe6, 0x39),
        current: Color32::WHITE,
        selected_bg: Color32::from_rgb(0x0b, 0x2f, 0xbf),
        dim: Color32::from_rgb(0x14, 0xa8, 0x3a),
        accent: Color32::from_rgb(0x2b, 0xff, 0x62),
    };

    /// egui's own widgets (text fields, sliders) in the skin's colours.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = self.frame;
        visuals.window_fill = self.frame;
        visuals.extreme_bg_color = self.lcd_bg;
        visuals.override_text_color = Some(self.list_fg);
        visuals.selection.bg_fill = self.selected_bg;
        visuals.selection.stroke = Stroke::new(1.0, self.current);
        visuals.text_cursor.stroke = Stroke::new(1.5, self.accent);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.open,
        ] {
            widget.corner_radius = CornerRadius::ZERO;
        }
        visuals.widgets.inactive.bg_fill = self.button;
        visuals.widgets.inactive.weak_bg_fill = self.button;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.lo);
        visuals.widgets.hovered.bg_fill = self.hi;
        visuals.widgets.hovered.weak_bg_fill = self.hi;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        visuals.widgets.active.bg_fill = self.accent;
        visuals.widgets.active.weak_bg_fill = self.accent;
        visuals.slider_trailing_fill = true;
        ctx.set_visuals(visuals);
    }
}

/// A raised (button, frame) or sunken (LCD, list) one-pixel bevel.
pub fn bevel(painter: &Painter, rect: Rect, raised: bool, palette: &Palette) {
    let (top_left, bottom_right) = if raised {
        (palette.hi, palette.lo)
    } else {
        (palette.lo, palette.hi)
    };
    let r = rect.shrink(0.5);
    painter.line_segment([r.left_bottom(), r.left_top()], Stroke::new(1.0, top_left));
    painter.line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, top_left));
    painter.line_segment(
        [r.right_top(), r.right_bottom()],
        Stroke::new(1.0, bottom_right),
    );
    painter.line_segment(
        [r.right_bottom(), r.left_bottom()],
        Stroke::new(1.0, bottom_right),
    );
}

/// A filled panel with a sunken edge.
pub fn inset(painter: &Painter, rect: Rect, fill: Color32, palette: &Palette) {
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    bevel(painter, rect, false, palette);
}

/// The title bar's horizontal grooves.
pub fn stripes(painter: &Painter, rect: Rect, palette: &Palette) {
    let mut y = rect.top().floor() + 0.5;
    while y <= rect.bottom() {
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, palette.hi),
        );
        y += 3.0;
    }
}

/// An outline for keyboard focus.
pub fn focus_ring(painter: &Painter, rect: Rect, palette: &Palette) {
    painter.rect_stroke(
        rect.shrink(2.0),
        CornerRadius::ZERO,
        Stroke::new(1.0, palette.accent),
        StrokeKind::Inside,
    );
}
