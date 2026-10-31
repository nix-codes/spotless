//! The slim column: the playlist + ADD adds to, its ADD TO box and its
//! keys.

use egui::{
    Align2, FontId, Id, Key, Rect, RichText, ScrollArea, Sense, Stroke, StrokeKind, Ui, UiBuilder,
    pos2, vec2,
};

use super::{Area, CatalogDrag, Nav, PAGE_ROWS, spans};
use crate::action::{Action, Actions};
use crate::ui::View;
use crate::ui::scrollbar::Scrollbar;
use crate::ui::theme;
use spotless_core::model::playlist::Playlist;

pub(super) fn slim_column(ui: &mut Ui, view: &View, nav: &mut Nav, actions: &mut Actions) {
    let palette = view.palette;
    let browse = view.browse;
    let Some(target) = view.list(browse.target) else {
        return;
    };
    // ↓ from the box is the box's: the songs don't take it too.
    let box_keyed = ui.horizontal(|ui| {
        ui.label(
            RichText::new("ADD TO")
                .monospace()
                .size(10.0)
                .color(palette.chrome),
        );
        // The box in the buttons' colours, which may be light; its list
        // in the window's.
        let window = ui.visuals().clone();
        let visuals = ui.visuals_mut();
        visuals.override_text_color = Some(palette.button_text);
        for (widget, fill) in [
            (&mut visuals.widgets.inactive, palette.button),
            (&mut visuals.widgets.hovered, palette.button_hi),
            (&mut visuals.widgets.active, palette.button_hi),
            (&mut visuals.widgets.open, palette.button_hi),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.fg_stroke.color = palette.button_text;
        }
        let shown = egui::ComboBox::from_id_salt("add-target")
            .selected_text(&target.name)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                *ui.visuals_mut() = window;
                for (index, &id) in view.tabs.iter().enumerate() {
                    let Some(list) = view.list(id) else { continue };
                    let mut item = ui.selectable_label(list.id == target.id, &list.name);
                    if nav.pick == Some(index) {
                        item = item.highlight();
                        item.scroll_to_me(None);
                    }
                    if item.clicked() {
                        actions.push(Action::SetTarget(list.id));
                    }
                }
            });
        let response = shown.response.on_hover_text(
            "The playlist + ADD adds to (Alt+↓ to choose; Ctrl+Tab / Ctrl+Shift+Tab, \
             Ctrl+1 … 9)",
        );
        if nav.on_box && nav.area == Area::Slim && nav.keyboard {
            theme::focus_ring(ui.painter(), response.rect.expand(3.0), &palette);
        }
        box_keys(ui, view, target, nav, &response, actions)
    });
    ui.add_space(6.0);
    let footer = 16.0;
    let rows = Rect::from_min_max(
        ui.cursor().min,
        pos2(ui.max_rect().right(), ui.max_rect().bottom() - footer),
    );
    let bar = Scrollbar::new(ui, view, Id::new("slim-scrollbar"));
    let (rows, slot) = bar.split(rows);
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        if !box_keyed.inner {
            slim_keys(ui, target, nav, actions);
        }
        slim_rows(ui, view, nav, target, (&bar, slot), actions);
    });
    ui.painter().text(
        pos2(ui.max_rect().left(), ui.max_rect().bottom()),
        Align2::LEFT_BOTTOM,
        summary(target),
        FontId::monospace(10.0),
        palette.dim,
    );
    if browse.flashing() {
        ui.ctx().request_repaint();
    }
}

/// The slim column's keys, when it is the active area: the arrows move the
/// highlighted song, Enter plays it, Delete removes it.
/// The ADD TO box's keys, while it is highlighted: Enter (or Alt+↓ / ↑)
/// opens its list; in the list, ↑ and ↓ highlight a playlist, Enter (or
/// Alt+↓ / ↑) chooses it and Esc closes it (egui's own). ↓ goes back to the songs. Says whether it
/// took a key.
fn box_keys(
    ui: &Ui,
    view: &View,
    target: &Playlist,
    nav: &mut Nav,
    response: &egui::Response,
    actions: &mut Actions,
) -> bool {
    let popup = egui::Popup::default_response_id(response);
    let open = egui::Popup::is_id_open(ui.ctx(), popup);
    if !open {
        nav.pick = None;
    }
    nav.box_open = open;
    if nav.area != Area::Slim || !(nav.on_box || open) || ui.ctx().text_edit_focused() {
        return false;
    }
    let (up, down, mut enter) = ui.input(|i| {
        let plain = i.modifiers.is_none();
        (
            plain && i.key_pressed(Key::ArrowUp),
            plain && i.key_pressed(Key::ArrowDown),
            plain && i.key_pressed(Key::Enter),
        )
    });
    enter |= std::mem::take(&mut nav.toggle_box);
    if !open {
        if enter {
            egui::Popup::open_id(ui.ctx(), popup);
            nav.pick = view.tabs.iter().position(|&id| id == target.id);
            nav.keyboard = true;
        } else if down {
            nav.on_box = false;
            nav.slim = target.entries().first().map(|entry| entry.id);
            nav.slim_reveal = nav.slim.map(|_| 0);
            nav.keyboard = true;
        }
        return up || down || enter;
    }
    let last = view.tabs.len().saturating_sub(1);
    let at = nav
        .pick
        .or_else(|| view.tabs.iter().position(|&id| id == target.id));
    if up || down {
        nav.pick = Some(match at {
            Some(at) if up => at.saturating_sub(1),
            Some(at) => (at + 1).min(last),
            None => 0,
        });
        nav.keyboard = true;
    }
    if enter && let Some(&id) = at.and_then(|at| view.tabs.get(at)) {
        actions.push(Action::SetTarget(id));
        egui::Popup::close_id(ui.ctx(), popup);
        nav.box_open = false;
        nav.pick = None;
    }
    up || down || enter
}

fn slim_keys(ui: &Ui, list: &Playlist, nav: &mut Nav, actions: &mut Actions) {
    nav.slim = nav.slim.filter(|&id| list.index_of(id).is_some());
    if nav.area != Area::Slim || nav.on_box || nav.box_open || ui.ctx().text_edit_focused() {
        return;
    }
    let from = nav.slim.or(list.cursor()).and_then(|id| list.index_of(id));
    // ↑ from the first song, or in an empty playlist, goes up to ADD TO.
    let to_box = ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::ArrowUp))
        && from.is_none_or(|at| at == 0);
    if to_box {
        nav.on_box = true;
        nav.slim = None;
        nav.keyboard = true;
        return;
    }
    if list.is_empty() {
        return;
    }
    let last = list.len() - 1;
    let (to, enter, delete) = ui.input(|i| {
        if !i.modifiers.is_none() {
            return (None, false, false);
        }
        let at = from.unwrap_or(0);
        let to = if i.key_pressed(Key::ArrowDown) {
            Some(from.map_or(0, |at| at + 1))
        } else if i.key_pressed(Key::ArrowUp) {
            Some(at.saturating_sub(1))
        } else if i.key_pressed(Key::PageDown) {
            Some(at + PAGE_ROWS)
        } else if i.key_pressed(Key::PageUp) {
            Some(at.saturating_sub(PAGE_ROWS))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(last)
        } else {
            None
        };
        (to, i.key_pressed(Key::Enter), i.key_pressed(Key::Delete))
    });
    if let Some(to) = to.map(|to| to.min(last)) {
        nav.slim = Some(list.entries()[to].id);
        nav.slim_reveal = Some(to);
        nav.keyboard = true;
    }
    let Some(id) = nav.slim else { return };
    if enter {
        actions.push(Action::PlayEntry(list.id, id));
    }
    if delete && let Some(index) = list.index_of(id) {
        // The next song takes the highlight, or the one before the last.
        let next = list
            .entries()
            .get(index + 1)
            .or(index.checked_sub(1).and_then(|i| list.entries().get(i)));
        nav.slim = next.map(|entry| entry.id);
        actions.push(Action::Remove(list.id, vec![id]));
    }
}

fn slim_rows(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    list: &Playlist,
    (bar, slot): (&Scrollbar, Rect),
    actions: &mut Actions,
) {
    let palette = view.palette;
    let dragging = egui::DragAndDrop::payload::<CatalogDrag>(ui.ctx());
    if let Some(drag) = &dragging {
        crate::ui::widgets::drag_ghost(ui, &palette, &drag.label());
    }
    let pointer = ui.ctx().pointer_interact_pos();
    let area_rect = ui.max_rect();
    let hovered = pointer.is_some_and(|p| area_rect.contains(p));
    if list.is_empty() {
        let text = if dragging.is_some() {
            "Drop here to add."
        } else {
            "Empty. Use + ADD or drag songs here."
        };
        ui.label(RichText::new(text).color(palette.dim));
        if dragging.is_some() && hovered {
            ui.painter().rect_stroke(
                area_rect,
                0.0,
                Stroke::new(1.0, palette.accent),
                StrokeKind::Inside,
            );
        }
        drop(ui, dragging.is_some() && hovered, 0, actions);
        bar.paint(ui, slot, 0.0, 0.0, false);
        return;
    }
    let height = 17.0;
    // Named, so it doesn't take the scroll of the playlist drawn here
    // before Browse opened.
    let mut area = ScrollArea::vertical()
        .id_salt("slim-rows")
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let view_height = ui.available_height();
    let furthest = (list.len() as f32 * height - view_height).max(0.0);
    let thumb_to = bar.interact(ui, slot, nav.slim_offset, furthest);
    if let Some(index) = nav.slim_reveal.take() {
        let top = index as f32 * height;
        let offset = if top < nav.slim_offset {
            top
        } else if top + height > nav.slim_offset + view_height {
            top + height - view_height
        } else {
            nav.slim_offset
        };
        area = area.vertical_scroll_offset(offset);
    // Bring the songs just added into view.
    } else if let Some(last) = list
        .entries()
        .iter()
        .rposition(|entry| view.browse.glow(entry.id) > 0.9)
    {
        area = area.vertical_scroll_offset(((last + 1) as f32 * height - view_height).max(0.0));
    }
    if let Some(offset) = thumb_to {
        area = area.vertical_scroll_offset(offset);
    }
    let current = list.cursor().filter(|_| !list.detached());
    let mut drop_at: Option<(usize, f32)> = None;
    // Before show_rows, which counts the spacing into each row's height.
    ui.spacing_mut().item_spacing.y = 0.0;
    let output = area.show_rows(ui, height, list.len(), |ui, range| {
        for index in range {
            let entry = &list.entries()[index];
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
            if response.clicked() {
                nav.slim = Some(entry.id);
                nav.area = Area::Slim;
                nav.on_box = false;
            }
            if response.double_clicked() {
                actions.push(Action::PlayEntry(list.id, entry.id));
            }
            if dragging.is_some()
                && let Some(p) = pointer
                && hovered
                && rect.y_range().contains(p.y)
            {
                drop_at = Some(if p.y < rect.center().y {
                    (index, rect.top())
                } else {
                    (index + 1, rect.bottom())
                });
            }
            let painter = ui.painter_at(rect);
            if nav.area == Area::Slim && nav.slim == Some(entry.id) {
                painter.rect_filled(rect, 0.0, palette.selected_bg);
            }
            let glow = view.browse.glow(entry.id);
            if glow > 0.0 {
                painter.rect_filled(rect, 0.0, palette.accent.gamma_multiply(0.45 * glow));
            }
            let color = if Some(entry.id) == current {
                palette.current
            } else {
                palette.list_fg
            };
            let seconds = entry.track.duration_ms / 1000;
            let duration = painter.text(
                rect.right_center() - vec2(2.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{}:{:02}", seconds / 60, seconds % 60),
                FontId::monospace(10.0),
                palette.dim,
            );
            let text = Rect::from_min_max(rect.min, pos2(duration.left() - 6.0, rect.bottom()));
            let number = format!("{}.", index + 1);
            spans(
                &painter,
                text,
                &[
                    (number.as_str(), FontId::monospace(10.0), palette.dim),
                    (
                        entry.track.title.as_str(),
                        FontId::proportional(12.0),
                        color,
                    ),
                ],
            );
        }
    });
    nav.slim_offset = output.state.offset.y;
    bar.paint(ui, slot, nav.slim_offset, furthest, thumb_to.is_some());
    if dragging.is_none() || !hovered {
        return;
    }
    // Below the last row: the end.
    let (index, y) = drop_at.unwrap_or((
        list.len(),
        (output.inner_rect.top() - output.state.offset.y + list.len() as f32 * height)
            .min(output.inner_rect.bottom() - 1.0),
    ));
    ui.painter().with_clip_rect(output.inner_rect).hline(
        output.inner_rect.x_range(),
        y,
        Stroke::new(2.0, palette.accent),
    );
    drop(ui, true, index, actions);
}

/// Takes a catalog drag released over the slim column, to add before the
/// row at `index`.
fn drop(ui: &Ui, over: bool, index: usize, actions: &mut Actions) {
    if !over || !ui.input(|i| i.pointer.any_released()) {
        return;
    }
    let Some(drag) = egui::DragAndDrop::take_payload::<CatalogDrag>(ui.ctx()) else {
        return;
    };
    actions.push(match &*drag {
        CatalogDrag::Tracks(tracks) => Action::DropTracks(tracks.clone(), index),
        CatalogDrag::Album { uri, .. } => Action::DropAlbum(uri.clone(), index),
        CatalogDrag::Loved(songs) => Action::DropLoved(songs.clone(), index),
    });
}

fn summary(list: &Playlist) -> String {
    let total: u64 = list
        .entries()
        .iter()
        .map(|e| u64::from(e.track.duration_ms))
        .sum::<u64>()
        / 1000;
    let time = if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    };
    match list.len() {
        1 => format!("1 song · {time}"),
        n => format!("{n} songs · {time}"),
    }
}
