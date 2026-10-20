//! The playlist's scrollbar, Winamp's: a thumb in a slot of its own beside
//! the rows, the skin's when it has one.
//!
//! The thumb is always there, at the top when everything fits. Dragging it
//! scrolls; pressing in the slot brings the thumb's middle to the pointer.

use egui::{Id, Rect, Sense, Ui, pos2, vec2};

use super::skin_paint::SkinPaint;
use super::theme::{self, Palette};
use super::{GUTTER, View, skinned_strip};
use crate::skin::format::State;

/// The thumb's size in points without a skin.
const WIDTH: f32 = 12.0;
const THUMB_HEIGHT: f32 = 27.0;

pub struct Scrollbar<'a> {
    paint: Option<&'a SkinPaint>,
    palette: Palette,
    /// Points per image pixel, the title bar's.
    unit: f32,
    id: Id,
}

impl<'a> Scrollbar<'a> {
    pub fn new(ui: &Ui, view: &'a View, id: Id) -> Self {
        let width = ui.ctx().content_rect().width() - 2.0 * GUTTER;
        let paint = view
            .art
            .filter(|paint| paint.has("scrollbar.track") && paint.has("scrollbar.thumb"));
        Self {
            paint,
            palette: view.palette,
            unit: paint.map_or(1.0, |paint| skinned_strip::unit(paint, width)),
            id,
        }
    }

    /// The thumb's size: the skin's, or Classic's.
    fn thumb(&self) -> egui::Vec2 {
        self.paint
            .and_then(|paint| paint.size("scrollbar.thumb", self.unit))
            .map_or(vec2(WIDTH, THUMB_HEIGHT), |size| size.round())
    }

    pub fn width(&self) -> f32 {
        self.thumb().x
    }

    fn thumb_height(&self) -> f32 {
        self.thumb().y
    }

    /// Where the thumb's top is when the list is `offset` of `max`
    /// scrolled.
    fn thumb_top(&self, slot: Rect, offset: f32, max: f32) -> f32 {
        let travel = (slot.height() - self.thumb_height()).max(0.0);
        let along = if max > 0.0 {
            (offset / max).clamp(0.0, 1.0)
        } else {
            0.0
        };
        slot.top() + (along * travel).round()
    }

    /// Lets the thumb be dragged; while it is held, gives the offset it
    /// was dragged to. `offset` is the one shown, `max` the furthest the
    /// list scrolls.
    pub fn interact(&self, ui: &Ui, slot: Rect, offset: f32, max: f32) -> Option<f32> {
        let response = ui.interact(slot, self.id, Sense::click_and_drag());
        if max <= 0.0 || !response.is_pointer_button_down_on() {
            return None;
        }
        let pointer = response.interact_pointer_pos()?;
        let thumb_height = self.thumb_height();
        // How far below the thumb's top it was taken hold of.
        let grab = if ui.input(|i| i.pointer.any_pressed()) {
            let top = self.thumb_top(slot, offset, max);
            let on_thumb = (top..=top + thumb_height).contains(&pointer.y);
            let grab = if on_thumb {
                pointer.y - top
            } else {
                thumb_height / 2.0
            };
            ui.data_mut(|data| data.insert_temp(self.id, grab));
            grab
        } else {
            ui.data(|data| data.get_temp(self.id))
                .unwrap_or(thumb_height / 2.0)
        };
        let travel = slot.height() - thumb_height;
        if travel <= 0.0 {
            return None;
        }
        let along = ((pointer.y - grab - slot.top()) / travel).clamp(0.0, 1.0);
        Some(along * max)
    }

    /// Paints the slot and the thumb, `pressed` while it is held.
    pub fn paint(&self, ui: &Ui, slot: Rect, offset: f32, max: f32, pressed: bool) {
        let top = self.thumb_top(slot, offset, max);
        let thumb = Rect::from_min_size(
            pos2(slot.left(), top),
            vec2(slot.width(), self.thumb_height()),
        );
        let painter = ui.painter().with_clip_rect(slot);
        let Some(paint) = self.paint else {
            theme::inset(&painter, slot, self.palette.frame, &self.palette);
            let face = self.palette.thumb;
            painter.rect_filled(thumb, 0.0, face);
            let edges = Palette {
                hi: face.lerp_to_gamma(egui::Color32::WHITE, 0.45),
                lo: face.lerp_to_gamma(egui::Color32::BLACK, 0.9),
                ..self.palette
            };
            theme::bevel(&painter, thumb, !pressed, &edges);
            return;
        };
        paint.paint(&painter, "scrollbar.track", State::Normal, slot, self.unit);
        let state = if pressed {
            State::Pressed
        } else {
            State::Normal
        };
        paint.paint(&painter, "scrollbar.thumb", state, thumb, self.unit);
    }
}
