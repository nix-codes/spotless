//! The playlist editor: the tabs, the rows, and a toolbar.
//!
//! Tabs: click to show, double-click to rename, drag to reorder, ✕ to
//! close (not Default), + for a new playlist; ▶ marks the playing one.
//!
//! Rows: click selects (Ctrl toggles, Shift extends), double-click or Enter
//! plays, Delete removes, Q marks to play next, Alt+Up/Down or dragging
//! moves the selection, Ctrl+A selects all. Up/Down, PageUp/PageDown and
//! Home/End move the selection; with Shift they extend it.

use std::collections::BTreeSet;

use egui::scroll_area::ScrollBarVisibility;
use egui::{
    Align, Align2, Color32, DragAndDrop, FontId, Id, Key, Layout, Modifiers, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::scrollbar::Scrollbar;
use super::skin_paint;
use super::theme::{self, Palette};
use super::widgets::{Glyph, drag_ghost, field, small_action, small_button, small_glyph};
use super::{Inputs, View};
use crate::action::Action;
use crate::model::{EntryId, Playlist, PlaylistId};

const ROW_HEIGHT: f32 = 18.0;
const TAB_HEIGHT: f32 = 22.0;
const TOOLBAR_HEIGHT: f32 = 18.0;
/// Between the tabs and the rows, and between the rows and the toolbar.
const LIST_GAP: f32 = 16.0;
/// Between the rows and the scrollbar's slot.
const SLOT_GAP: f32 = 4.0;
/// Dragging this close to the list's top or bottom edge scrolls it.
const SCROLL_EDGE: f32 = 24.0;

/// The editor's own state, kept between frames.
#[derive(Default)]
pub struct Selection {
    pub rows: BTreeSet<EntryId>,
    /// Where Shift+click and Shift+arrows extend from.
    anchor: Option<EntryId>,
    /// The row the arrow keys move from, framed when several are selected.
    cursor: Option<EntryId>,
    /// Rows that fit in the list, for PageUp and PageDown.
    page: usize,
    /// The scroll offset last frame, and the row to bring into view.
    offset: f32,
    reveal: Option<usize>,
}

impl Selection {
    /// Selects one row and scrolls to it.
    fn select(&mut self, id: EntryId, index: usize) {
        self.rows = BTreeSet::from([id]);
        self.anchor = Some(id);
        self.cursor = Some(id);
        self.reveal = Some(index);
    }
}

/// Rows being dragged, in list order.
struct RowDrag(Vec<EntryId>);

/// A tab being dragged.
struct TabDrag(PlaylistId);

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    tab_bar(ui, view, inputs, actions);
    // The item spacing after the tabs makes up the rest of the gap.
    ui.add_space(LIST_GAP - ui.spacing().item_spacing.y);
    let list = view.playlist;
    let full = ui.available_rect_before_wrap();
    let toolbar_rect =
        Rect::from_min_max(pos2(full.left(), full.bottom() - TOOLBAR_HEIGHT), full.max);
    let rows = Rect::from_min_max(full.min, pos2(full.right(), toolbar_rect.top() - LIST_GAP));
    let bar = Scrollbar::new(ui, view, Id::new("playlist-scrollbar"));
    let slot = Rect::from_min_max(pos2(rows.right() - bar.width(), rows.top()), rows.max);
    let rows = rows.with_max_x(slot.left() - SLOT_GAP);

    if let Some((jump_list, entry)) = inputs.jump
        && jump_list == list.id
    {
        inputs.jump = None;
        if let Some(index) = list.index_of(entry) {
            inputs.selection.select(entry, index);
        }
    }
    inputs
        .selection
        .rows
        .retain(|&id| list.index_of(id).is_some());
    if inputs.renaming.is_none() && inputs.closing.is_none() {
        keyboard(ui, list, &mut inputs.selection, actions);
    }
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        if list.is_empty() {
            ui.label(
                RichText::new("This playlist is empty. Press + ADD to find songs.")
                    .color(view.palette.dim),
            );
            bar.paint(ui, slot, 0.0, 0.0, false);
        } else {
            draw_rows(ui, view, &mut inputs.selection, (&bar, slot), actions);
        }
    });
    ui.scope_builder(UiBuilder::new().max_rect(toolbar_rect), |ui| {
        toolbar(ui, view, inputs, actions);
    });
    confirm_close(ui, view, inputs, actions);
}

fn tab_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let mut tabs: Vec<(PlaylistId, Rect)> = Vec::new();
    let bar = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for list in view.library.playlists() {
                let rect = if inputs
                    .renaming
                    .as_ref()
                    .is_some_and(|(id, _)| *id == list.id)
                {
                    rename_field(ui, list.id, &mut inputs.renaming, actions)
                } else {
                    tab(ui, view, list, inputs, actions)
                };
                tabs.push((list.id, rect));
            }
            if small_button(ui, &palette, "+", false)
                .on_hover_text("New playlist")
                .clicked()
            {
                actions.push(Action::NewPlaylist);
            }
        })
        .response
        .rect;
    baseline(ui, view, &tabs, bar);

    // Reordering: drop a dragged tab between two others.
    let Some(dragged) = DragAndDrop::payload::<TabDrag>(ui.ctx()) else {
        return;
    };
    if let Some(name) = view.library.get(dragged.0).map(|l| l.name.clone()) {
        drag_ghost(ui, &palette, &name);
    }
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    if !bar.expand(8.0).contains(pointer) {
        return;
    }
    let index = tabs
        .iter()
        .filter(|(_, r)| r.center().x < pointer.x)
        .count();
    let x = match tabs.get(index) {
        Some((_, r)) => r.left() - 2.0,
        None => tabs.last().map_or(bar.left(), |(_, r)| r.right() + 2.0),
    };
    ui.painter()
        .vline(x, bar.y_range(), Stroke::new(2.0, palette.accent));
    if ui.input(|i| i.pointer.any_released())
        && let Some(dragged) = DragAndDrop::take_payload::<TabDrag>(ui.ctx())
        && let Some(from) = tabs.iter().position(|(id, _)| *id == dragged.0)
    {
        let to = if from < index { index - 1 } else { index };
        if to != from {
            actions.push(Action::MovePlaylist(dragged.0, to));
        }
    }
}

/// The line the tabs stand on, across the panel, broken under the
/// shown tab so it opens into the list.
fn baseline(ui: &Ui, view: &View, tabs: &[(PlaylistId, Rect)], bar: Rect) {
    if skin_paint::has_chrome(ui.ctx(), "tab") {
        return;
    }
    let palette = view.palette;
    let stroke = Stroke::new(1.0, tab_edge(&palette));
    let y = bar.bottom() - 0.5;
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    let painter = ui.painter();
    match tabs.iter().find(|(id, _)| *id == view.playlist.id) {
        Some((_, shown)) => {
            painter.hline(left..=shown.left(), y, stroke);
            painter.hline(shown.right()..=right, y, stroke);
        }
        None => {
            painter.hline(left..=right, y, stroke);
        }
    }
}

/// The colour of the tabs' edges and the line they stand on.
fn tab_edge(palette: &Palette) -> Color32 {
    palette.button.gamma_multiply(0.6)
}

/// One tab: ▶ if playing, the name, the song count, ✕ unless Default.
fn tab(
    ui: &mut Ui,
    view: &View,
    list: &Playlist,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) -> Rect {
    let palette = view.palette;
    let shown = list.id == view.playlist.id;
    let playing = view.playing_list == Some(list.id);
    let closable = list.id != PlaylistId::DEFAULT;
    let name_font = FontId::proportional(12.5);
    let count_font = FontId::monospace(9.0);
    let count = list.len().to_string();
    let width = |text: &str, font: &FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), palette.chrome)
            .size()
            .x
    };
    let play_width = if playing { 12.0 } else { 0.0 };
    let close_width = if closable { 14.0 } else { 0.0 };
    let total = 10.0
        + play_width
        + width(&list.name, &name_font)
        + 6.0
        + width(&count, &count_font)
        + close_width
        + 10.0;
    let (rect, response) = ui.allocate_exact_size(vec2(total, TAB_HEIGHT), Sense::click_and_drag());
    let painter = ui.painter();
    let state = skin_paint::state(&response, shown, true);
    let skinned = skin_paint::chrome(painter, "tab", state, rect);
    // Every tab is drawn alike; the shown one opens into the list below
    // the bar's baseline (drawn in `tab_bar`) and has the bright text.
    if !skinned {
        let edge = Stroke::new(1.0, tab_edge(&palette));
        let r = rect.shrink(0.5);
        if !shown {
            painter.rect_filled(rect, 0.0, palette.button.gamma_multiply(0.25));
        }
        painter.line_segment([r.left_bottom(), r.left_top()], edge);
        painter.line_segment([r.left_top(), r.right_top()], edge);
        painter.line_segment([r.right_top(), r.right_bottom()], edge);
        if shown {
            let top = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 2.0));
            painter.rect_filled(top, 0.0, palette.accent);
        }
    }
    let (text_color, count_color) = match (skinned, shown) {
        (true, _) => (palette.button_text, palette.dim),
        (false, true) => (palette.current, palette.dim),
        (false, false) if response.hovered() => (palette.list_fg, palette.dim),
        (false, false) => (palette.dim, palette.dim),
    };
    let mut x = rect.left() + 10.0;
    if playing {
        let c = pos2(x + 3.0, rect.center().y);
        painter.add(egui::Shape::convex_polygon(
            vec![
                pos2(c.x - 3.0, c.y - 4.0),
                pos2(c.x + 4.0, c.y),
                pos2(c.x - 3.0, c.y + 4.0),
            ],
            palette.accent,
            Stroke::NONE,
        ));
        x += play_width;
    }
    let name = painter.text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        &list.name,
        name_font,
        text_color,
    );
    painter.text(
        pos2(name.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        count,
        count_font,
        count_color,
    );
    let close =
        Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(10.0, 10.0));
    let pointer = ui.ctx().pointer_interact_pos();
    let over_close = closable && pointer.is_some_and(|p| close.expand(2.0).contains(p));
    if closable {
        let color = if over_close && response.hovered() {
            palette.accent
        } else {
            palette.dim
        };
        let stroke = Stroke::new(1.2, color);
        let c = close.shrink(2.0);
        painter.line_segment([c.left_top(), c.right_bottom()], stroke);
        painter.line_segment([c.right_top(), c.left_bottom()], stroke);
    }

    if response.clicked() && over_close {
        if list.is_empty() {
            actions.push(Action::ClosePlaylist(list.id));
        } else {
            inputs.closing = Some(list.id);
        }
    } else if response.double_clicked() {
        inputs.renaming = Some((list.id, list.name.clone()));
    } else if response.clicked() {
        actions.push(Action::SelectPlaylist(list.id));
    }
    if response.drag_started() {
        response.dnd_set_drag_payload(TabDrag(list.id));
    }
    let hint = if closable {
        "Double-click to rename, drag to move"
    } else {
        "Double-click to rename"
    };
    response.on_hover_text_at_pointer(hint);
    rect
}

/// The tab being renamed: Enter or clicking away keeps the name, Esc
/// drops it.
fn rename_field(
    ui: &mut Ui,
    id: PlaylistId,
    renaming: &mut Option<(PlaylistId, String)>,
    actions: &mut Vec<Action>,
) -> Rect {
    let Some((_, name)) = renaming.as_mut() else {
        return Rect::NOTHING;
    };
    let field = field(
        ui,
        TextEdit::singleline(name)
            .id(Id::new(("rename", id)))
            .desired_width(140.0)
            .min_size(vec2(0.0, TAB_HEIGHT)),
    );
    if field.lost_focus() {
        if !ui.input(|i| i.key_pressed(Key::Escape)) {
            actions.push(Action::RenamePlaylist(id, name.trim().to_string()));
        }
        *renaming = None;
    } else if !field.has_focus() {
        field.request_focus();
    }
    field.rect
}

fn confirm_close(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let Some(list) = inputs.closing.and_then(|id| view.library.get(id)) else {
        inputs.closing = None;
        return;
    };
    let palette = view.palette;
    let modal = egui::Modal::new(Id::new("close-playlist"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.set_max_width(320.0);
            ui.label(
                RichText::new(format!("Close “{}”?", list.name))
                    .size(15.0)
                    .color(palette.current),
            );
            ui.add_space(4.0);
            let songs = match list.len() {
                1 => "Its song is removed with it.".to_string(),
                n => format!("Its {n} songs are removed with it."),
            };
            ui.label(RichText::new(songs).color(palette.chrome));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if small_button(ui, &palette, "CLOSE PLAYLIST", false).clicked() {
                    actions.push(Action::ClosePlaylist(list.id));
                    return true;
                }
                small_button(ui, &palette, "KEEP", false).clicked()
            })
            .inner
        });
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if modal.inner || modal.backdrop_response.clicked() || escape {
        inputs.closing = None;
    }
}

fn toolbar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let list = view.playlist;
    let selected = selected(list, &inputs.selection);
    let any = !selected.is_empty();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if small_action(ui, &palette, "+ ADD", true).clicked() {
            actions.push(Action::ToggleBrowse);
            inputs.focus_search = true;
        }
        if small_action(ui, &palette, "REM", any).clicked() {
            remove(list, &mut inputs.selection, selected.clone(), actions);
        }
        if small_action(ui, &palette, "Q NEXT", any).clicked() {
            actions.push(Action::ToggleMarks(selected.clone()));
        }
        if small_glyph(ui, &palette, Glyph::Up, any).clicked() {
            move_by(list, &mut inputs.selection, selected.clone(), true, actions);
        }
        if small_glyph(ui, &palette, Glyph::Down, any).clicked() {
            move_by(
                list,
                &mut inputs.selection,
                selected.clone(),
                false,
                actions,
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(summary(list))
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        });
    });
}

fn draw_rows(
    ui: &mut Ui,
    view: &View,
    selection: &mut Selection,
    (bar, slot): (&Scrollbar, Rect),
    actions: &mut Vec<Action>,
) {
    let palette = view.palette;
    let list = view.playlist;
    let mut area = ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    let furthest = (list.len() as f32 * ROW_HEIGHT - ui.available_height()).max(0.0);
    let thumb_to = bar.interact(ui, slot, selection.offset, furthest);
    if let Some(index) = selection.reveal.take() {
        let top = index as f32 * ROW_HEIGHT;
        let height = ui.available_height();
        let offset = if top < selection.offset {
            top
        } else if top + ROW_HEIGHT > selection.offset + height {
            top + ROW_HEIGHT - height
        } else {
            selection.offset
        };
        area = area.vertical_scroll_offset(offset);
    }
    if let Some(offset) = thumb_to {
        area = area.vertical_scroll_offset(offset);
    }
    let number_width = list.len().to_string().len();
    let current = list.cursor().filter(|_| !list.detached());
    let dragging = DragAndDrop::payload::<RowDrag>(ui.ctx());
    let pointer = ui.ctx().pointer_interact_pos();
    // Where dragged rows would land: the index, and the line to show it.
    let mut drop_at: Option<(usize, f32)> = None;
    // Rows touch. show_rows counts the spacing into each row's height, so
    // it is set before: inside, it would fit a row less in every three.
    ui.spacing_mut().item_spacing.y = 0.0;
    let output = area.show_rows(ui, ROW_HEIGHT, list.len(), |ui, range| {
        let viewport = ui.clip_rect();
        for index in range {
            let entry = &list.entries()[index];
            let (rect, response) = ui.allocate_exact_size(
                vec2(ui.available_width(), ROW_HEIGHT),
                Sense::click_and_drag(),
            );
            let selected = selection.rows.contains(&entry.id);
            let painter = ui.painter_at(rect);
            if selected {
                painter.rect_filled(rect, 0.0, palette.selected_bg);
            }
            if selection.rows.len() > 1 && selection.cursor == Some(entry.id) {
                theme::focus_ring(&painter, rect.expand(1.0), &palette);
            }
            paint_row(&painter, rect, &palette, list, index, number_width, current);

            if response.double_clicked() {
                selection.select(entry.id, index);
                selection.reveal = None;
                actions.push(Action::PlayEntry(list.id, entry.id));
            } else if response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                click(list, selection, entry.id, modifiers);
            }
            if response.drag_started() {
                if !selected {
                    selection.select(entry.id, index);
                    selection.reveal = None;
                }
                let rows = list
                    .entries()
                    .iter()
                    .map(|e| e.id)
                    .filter(|id| selection.rows.contains(id))
                    .collect();
                response.dnd_set_drag_payload(RowDrag(rows));
            }
            if dragging.is_some()
                && let Some(p) = pointer
                && viewport.contains(p)
                && rect.y_range().contains(p.y)
            {
                drop_at = Some(if p.y < rect.center().y {
                    (index, rect.top())
                } else {
                    (index + 1, rect.bottom())
                });
            }
        }
        if dragging.is_some()
            && let Some(p) = pointer
            && viewport.x_range().contains(p.x)
        {
            // Scroll while held near an edge.
            if p.y < viewport.top() + SCROLL_EDGE && p.y > viewport.top() - SCROLL_EDGE {
                ui.scroll_with_delta(vec2(0.0, 6.0));
                ui.ctx().request_repaint();
            } else if p.y > viewport.bottom() - SCROLL_EDGE && p.y < viewport.bottom() + SCROLL_EDGE
            {
                ui.scroll_with_delta(vec2(0.0, -6.0));
                ui.ctx().request_repaint();
            }
        }
    });
    selection.offset = output.state.offset.y;
    bar.paint(ui, slot, selection.offset, furthest, thumb_to.is_some());
    selection.page = ((output.inner_rect.height() / ROW_HEIGHT) as usize).max(1);

    let Some(dragging) = dragging else { return };
    let count = match dragging.0.len() {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    };
    drag_ghost(ui, &palette, &count);
    // Below the last row: the end of the list.
    if drop_at.is_none()
        && let Some(p) = pointer
        && output.inner_rect.contains(p)
    {
        drop_at = Some((
            list.len(),
            output.inner_rect.top() - output.state.offset.y + list.len() as f32 * ROW_HEIGHT,
        ));
    }
    let Some((index, y)) = drop_at else { return };
    ui.painter().with_clip_rect(output.inner_rect).hline(
        output.inner_rect.x_range(),
        y,
        Stroke::new(2.0, palette.accent),
    );
    if ui.input(|i| i.pointer.any_released())
        && let Some(rows) = DragAndDrop::take_payload::<RowDrag>(ui.ctx())
    {
        actions.push(Action::Move(rows.0.clone(), index));
    }
}

fn paint_row(
    painter: &egui::Painter,
    rect: Rect,
    palette: &Palette,
    list: &Playlist,
    index: usize,
    number_width: usize,
    current: Option<EntryId>,
) {
    let entry = &list.entries()[index];
    let color = if Some(entry.id) == current {
        palette.current
    } else {
        palette.list_fg
    };
    let mono = FontId::monospace(11.0);
    painter.text(
        rect.left_center() + vec2(4.0, 0.0),
        Align2::LEFT_CENTER,
        format!("{:>number_width$}.", index + 1),
        mono.clone(),
        color,
    );
    let duration = entry.track.duration_ms / 1000;
    let right = painter.text(
        rect.right_center() - vec2(6.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("{}:{:02}", duration / 60, duration % 60),
        mono.clone(),
        color,
    );
    let mut text_right = right.left() - 10.0;
    if let Some(mark) = list.mark_number(entry.id) {
        let badge = painter.text(
            pos2(text_right, rect.center().y),
            Align2::RIGHT_CENTER,
            format!("Q{mark}"),
            mono,
            palette.accent,
        );
        text_right = badge.left() - 8.0;
    }
    let left = rect.left() + 12.0 + (number_width as f32 + 1.0) * 7.0;
    let artists = entry
        .track
        .artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let title = if artists.is_empty() {
        entry.track.title.clone()
    } else {
        format!("{artists} - {}", entry.track.title)
    };
    painter
        .with_clip_rect(Rect::from_min_max(
            pos2(left, rect.top()),
            pos2(text_right, rect.bottom()),
        ))
        .text(
            pos2(left, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(12.5),
            color,
        );
}

fn click(list: &Playlist, selection: &mut Selection, id: EntryId, modifiers: Modifiers) {
    if modifiers.shift
        && let Some(anchor) = selection.anchor
        && let (Some(a), Some(b)) = (list.index_of(anchor), list.index_of(id))
    {
        let range = a.min(b)..=a.max(b);
        if !modifiers.command {
            selection.rows.clear();
        }
        selection
            .rows
            .extend(list.entries()[range].iter().map(|entry| entry.id));
        selection.cursor = Some(id);
        return;
    }
    if modifiers.command {
        if !selection.rows.remove(&id) {
            selection.rows.insert(id);
        }
    } else {
        selection.rows = BTreeSet::from([id]);
    }
    selection.anchor = Some(id);
    selection.cursor = Some(id);
}

/// The selected rows, in list order.
fn selected(list: &Playlist, selection: &Selection) -> Vec<EntryId> {
    list.entries()
        .iter()
        .map(|entry| entry.id)
        .filter(|id| selection.rows.contains(id))
        .collect()
}

/// Removes the selection; the row after the last removed one is selected.
fn remove(
    list: &Playlist,
    selection: &mut Selection,
    rows: Vec<EntryId>,
    actions: &mut Vec<Action>,
) {
    let Some(last) = rows.iter().filter_map(|&id| list.index_of(id)).max() else {
        return;
    };
    let next = list.entries()[last + 1..]
        .iter()
        .chain(list.entries()[..last].iter().rev())
        .map(|entry| entry.id)
        .find(|id| !selection.rows.contains(id));
    selection.rows = next.into_iter().collect();
    selection.anchor = next;
    selection.cursor = next;
    actions.push(Action::Remove(list.id, rows));
}

/// Moves the selection one row up or down.
fn move_by(
    list: &Playlist,
    selection: &mut Selection,
    rows: Vec<EntryId>,
    up: bool,
    actions: &mut Vec<Action>,
) {
    let indices: Vec<usize> = rows.iter().filter_map(|&id| list.index_of(id)).collect();
    let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        return;
    };
    if up && first > 0 {
        actions.push(Action::Move(rows, first - 1));
        selection.reveal = Some(first - 1);
    } else if !up && last + 1 < list.len() {
        actions.push(Action::Move(rows, last + 2));
        selection.reveal = Some(last + 1);
    }
}

/// The editor's keys, unless a text field has the keyboard.
fn keyboard(ui: &Ui, list: &Playlist, selection: &mut Selection, actions: &mut Vec<Action>) {
    if ui.ctx().text_edit_focused() || list.is_empty() {
        return;
    }
    let rows = selected(list, selection);
    let (pressed, modifiers) = ui.input(|i| {
        let keys = [
            Key::Enter,
            Key::Delete,
            Key::Q,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::A,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        (
            keys.into_iter().find(|&key| i.key_pressed(key)),
            i.modifiers,
        )
    });
    let Some(key) = pressed else { return };
    match key {
        Key::Enter => {
            if let Some(&first) = rows.first() {
                actions.push(Action::PlayEntry(list.id, first));
            }
        }
        Key::Delete if !rows.is_empty() => remove(list, selection, rows, actions),
        Key::Q if !rows.is_empty() => actions.push(Action::ToggleMarks(rows)),
        Key::A if modifiers.command => {
            selection.rows = list.entries().iter().map(|entry| entry.id).collect();
        }
        Key::ArrowUp | Key::ArrowDown if modifiers.alt && !rows.is_empty() => {
            move_by(list, selection, rows, key == Key::ArrowUp, actions);
        }
        Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::PageUp | Key::PageDown
            if !modifiers.alt && !modifiers.command =>
        {
            let from = selection
                .cursor
                .or(selection.anchor)
                .and_then(|id| list.index_of(id))
                .or(list.cursor().and_then(|id| list.index_of(id)));
            let last = list.len() - 1;
            let page = selection.page.max(1);
            let to = match (key, from) {
                (Key::Home, _) => 0,
                (Key::End, _) => last,
                (Key::ArrowUp, Some(i)) => i.saturating_sub(1),
                (Key::ArrowDown, Some(i)) => (i + 1).min(last),
                (Key::PageUp, Some(i)) => i.saturating_sub(page),
                (Key::PageDown, Some(i)) => (i + page).min(last),
                _ => 0,
            };
            let id = list.entries()[to].id;
            if modifiers.shift {
                // Extend from the anchor to the new row.
                let anchor = selection
                    .anchor
                    .and_then(|id| list.index_of(id))
                    .unwrap_or(from.unwrap_or(to));
                selection.anchor = Some(list.entries()[anchor].id);
                selection.rows = list.entries()[anchor.min(to)..=anchor.max(to)]
                    .iter()
                    .map(|entry| entry.id)
                    .collect();
                selection.cursor = Some(id);
                selection.reveal = Some(to);
            } else {
                selection.select(id, to);
            }
        }
        _ => {}
    }
}

/// "12 songs · 54:10".
pub fn summary(list: &Playlist) -> String {
    let total: u64 = list
        .entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .sum::<u64>()
        / 1000;
    let songs = match list.len() {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    };
    let time = if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    };
    format!("{songs} · {time}")
}
