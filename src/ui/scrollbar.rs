//! Spotless's scrollbar, Winamp's playlist one: a thumb in a slot of its own
//! beside what scrolls, the skin's when it has one. The playlists, Browse
//! and the settings use it in place of egui's.
//!
//! It shows only when there is more than fits; its slot stays, empty, so
//! nothing moves when it comes. Dragging the thumb scrolls; pressing in the
//! slot brings the thumb's middle to the pointer.

use egui::scroll_area::{ScrollAreaOutput, ScrollBarVisibility};
use egui::{Id, Rect, ScrollArea, Sense, Ui, UiBuilder, pos2, vec2};

use super::skin_paint::SkinPaint;
use super::theme::{self, Palette};
use super::{GUTTER, View, skinned_strip};
use crate::skin::format::State;

/// The thumb's size in points without a skin.
const WIDTH: f32 = 12.0;
const THUMB_HEIGHT: f32 = 27.0;
/// Between what scrolls and the slot.
pub const GAP: f32 = 4.0;

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

    /// Splits `rect` into what scrolls and the slot at its right.
    pub fn split(&self, rect: Rect) -> (Rect, Rect) {
        let slot = Rect::from_min_max(pos2(rect.right() - self.width(), rect.top()), rect.max);
        (rect.with_max_x(slot.left() - GAP), slot)
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

    /// Paints the slot and the thumb, `pressed` while it is held: nothing
    /// when the list doesn't scroll, rather than a bar that suggests more.
    pub fn paint(&self, ui: &Ui, slot: Rect, offset: f32, max: f32, pressed: bool) {
        if max <= 0.0 {
            return;
        }
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

/// Shows a scroll area in the rest of `ui`'s space with Spotless's
/// scrollbar beside it instead of egui's. `show` shows `area`, which it may
/// set further (an offset to reveal something), in the `Ui` it is given.
///
/// How far the content scrolls is only known once it is shown, so the
/// thumb is dragged against the last frame's.
pub fn show<R>(
    ui: &mut Ui,
    view: &View,
    id: Id,
    area: ScrollArea,
    show: impl FnOnce(ScrollArea, &mut Ui) -> ScrollAreaOutput<R>,
) -> R {
    let bar = Scrollbar::new(ui, view, id);
    let rest = ui.available_rect_before_wrap();
    let (content, slot) = bar.split(rest);
    let extent = id.with("extent");
    let (offset, max) = ui.data(|data| data.get_temp(extent)).unwrap_or((0.0, 0.0));
    let thumb_to = bar.interact(ui, slot, offset, max);
    let mut area = area
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    if let Some(offset) = thumb_to {
        area = area.vertical_scroll_offset(offset);
    }
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(content), |ui| show(area, ui))
        .inner;
    let offset = output.state.offset.y;
    let max = (output.content_size.y - output.inner_rect.height()).max(0.0);
    ui.data_mut(|data| data.insert_temp(extent, (offset, max)));
    bar.paint(ui, slot, offset, max, thumb_to.is_some());
    ui.advance_cursor_after_rect(rest);
    output.inner
}
