//! The skin's colours and the Winamp-style drawing helpers.
//!
//! Classic is built in; a classic Winamp skin gives another (`crate::skin`).

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
    /// The playlist scrollbar's thumb.
    pub thumb: Color32,
    /// A skinned title bar's name, on the skin's plate.
    pub title: Color32,
    /// A text button's label, on its face.
    pub button_text: Color32,
    /// A button's light and dark edges.
    pub button_hi: Color32,
    pub button_lo: Color32,
}

pub use crate::phosphor::Phosphor;

fn rgb([r, g, b]: crate::phosphor::Rgb) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The colour the display's text and digits take: none for green, the
/// colour skins draw them in already.
pub fn display_colour(phosphor: Phosphor) -> Option<Color32> {
    (phosphor != Phosphor::Green).then(|| rgb(phosphor.shades().lit))
}

impl Palette {
    /// The lists and the display in `phosphor`'s colours. Green keeps the
    /// skin's own greens; the selection's bar is a dark shade in each.
    pub fn with_phosphor(self, phosphor: Phosphor) -> Self {
        let shades = phosphor.shades();
        if phosphor == Phosphor::Green {
            return Self {
                selected_bg: rgb(shades.selected),
                ..self
            };
        }
        Self {
            lcd: rgb(shades.lit),
            list_fg: rgb(shades.text),
            current: rgb(shades.current),
            selected_bg: rgb(shades.selected),
            dim: rgb(shades.dim),
            accent: rgb(shades.lit),
            ..self
        }
    }
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
        thumb: Color32::from_rgb(0xb0, 0x99, 0x5e),
        title: Color32::from_rgb(0xc9, 0xc9, 0xda),
        button_text: Color32::from_rgb(0xc9, 0xc9, 0xda),
        button_hi: Color32::from_rgb(0x4c, 0x4c, 0x62),
        button_lo: Color32::from_rgb(0x11, 0x11, 0x17),
    };

    /// A button's label while it is on or under the pointer: the accent,
    /// or on a light face, a darker shade of it that still reads.
    pub fn button_lit(&self) -> Color32 {
        use crate::skin::{contrast, mix};
        [0.0, 0.45, 0.65]
            .into_iter()
            .map(|dark| mix(self.accent, Color32::BLACK, dark))
            .find(|&lit| contrast(lit, self.button) >= 4.5)
            .unwrap_or(self.button_text)
    }

    /// These colours for a button's bevel: its own edges.
    pub fn for_buttons(&self) -> Self {
        Self {
            hi: self.button_hi,
            lo: self.button_lo,
            ..*self
        }
    }

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

/// A panel that holds a list: the skin's, or an inset in the list's
/// colour.
pub fn panel(painter: &Painter, rect: Rect, palette: &Palette) {
    let skinned =
        super::skin_paint::chrome(painter, "panel", crate::skin::format::State::Normal, rect);
    if !skinned {
        inset(painter, rect, palette.list_bg, palette);
    }
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

/// What a refusal is drawn in, whatever the phosphor: a synced tab's mark
/// when Spotify refused a change.
pub const WARNING: Color32 = Color32::from_rgb(222, 84, 64);

/// A synced playlist's mark, centred on `c`, about 9 px high: an arrow up
/// beside one down (⇅).
pub fn sync_mark(painter: &Painter, c: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    for (x, up) in [(c.x - 2.5, true), (c.x + 2.5, false)] {
        let (tip, tail) = if up {
            (c.y - 4.5, c.y + 4.5)
        } else {
            (c.y + 4.5, c.y - 4.5)
        };
        let back = if up { 2.5 } else { -2.5 };
        painter.line_segment([pos2(x, tail), pos2(x, tip)], stroke);
        painter.line_segment([pos2(x - 2.0, tip + back), pos2(x, tip)], stroke);
        painter.line_segment([pos2(x + 2.0, tip + back), pos2(x, tip)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_keeps_the_skin_s_greens_and_the_others_replace_them() {
        let classic = Palette::CLASSIC;
        let green = classic.with_phosphor(Phosphor::Green);
        assert_eq!(green.list_fg, classic.list_fg);
        assert_ne!(green.selected_bg, classic.selected_bg);
        let amber = classic.with_phosphor(Phosphor::Amber);
        assert_eq!(amber.list_fg, Color32::from_rgb(0xff, 0xb0, 0x00));
        assert_eq!(amber.accent, amber.lcd);
        assert_eq!(amber.button, classic.button);
    }
}
