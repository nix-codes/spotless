//! The playlist's scrollbar, Winamp's: a thumb in a slot of its own beside
//! the rows, from `pledit.bmp` when the skin has it.
//!
//! The thumb is always there, at the top when everything fits. Dragging it
//! scrolls; pressing in the slot brings the thumb's middle to the pointer.

use egui::{Id, Rect, Sense, Ui, pos2, vec2};

use super::skin_art::SkinArt;
use super::theme::{self, Palette};
use super::{GUTTER, View, skinned_strip};
use crate::skin::{Sheet, sprites};

pub struct Scrollbar<'a> {
    art: Option<&'a SkinArt>,
    palette: Palette,
    /// Points per skin pixel, the title bar's.
    unit: f32,
    id: Id,
}

impl<'a> Scrollbar<'a> {
    pub fn new(ui: &Ui, view: &'a View, id: Id) -> Self {
        let width = ui.ctx().content_rect().width() - 2.0 * GUTTER;
        Self {
            art: view.art.filter(|art| art.has(Sheet::PlEdit)),
            palette: view.palette,
            unit: skinned_strip::unit(width),
            id,
        }
    }

    pub fn width(&self) -> f32 {
        (sprites::SCROLL_THUMB.width as f32 * self.unit).round()
    }

    fn thumb_height(&self) -> f32 {
        (sprites::SCROLL_THUMB.height as f32 * self.unit).round()
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
        let Some(art) = self.art else {
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
        let piece = sprites::SCROLL_SLOT;
        let step = piece.height as f32 * self.unit;
        let mut y = slot.top();
        while y < slot.bottom() {
            art.paint(&painter, piece, pos2(slot.left(), y), self.unit);
            y += step;
        }
        let sprite = if pressed {
            sprites::SCROLL_THUMB_PRESSED
        } else {
            sprites::SCROLL_THUMB
        };
        art.paint(&painter, sprite, thumb.min, self.unit);
    }
}
