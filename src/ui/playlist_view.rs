//! The playlist editor: the tabs, the rows, and a toolbar.
//!
//! Tabs: click to show, double-click to rename, drag to reorder, ✕ to
//! close (not Default), + for a new playlist; ▶ marks the playing one.
//!
//! Rows: click selects (Ctrl toggles, Shift extends), double-click or Enter
//! plays, Delete removes, Q marks to play next, Alt+Up/Down or dragging
//! moves the selection, Ctrl+A selects all. Up/Down, PageUp/PageDown and
//! Home/End move the selection; with Shift they extend it. Dragged onto
//! another tab, the selection is copied there, or moved with Shift held
//! (from the Spotify tab, songs to come, unless another device has it);
//! right-click for a menu that does the same.

use std::collections::BTreeSet;

use egui::scroll_area::ScrollBarVisibility;
use egui::{
    Align, Align2, Color32, DragAndDrop, FontId, Id, Key, Layout, Modifiers, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::scrollbar::Scrollbar;
use super::skin_paint;
use super::theme::{self, Palette};
use super::widgets::{
    Glyph, drag_ghost, field, small_action, small_button, small_glyph, text_button, tip,
};
use super::{Inputs, View, spotify_view};
use crate::action::{Action, Actions, Desktop};
use crate::model::{EntryId, Playlist, PlaylistId, Run};
use crate::playback::engine::Status;

const ROW_HEIGHT: f32 = 18.0;
/// The longest a jump to the playing song glides, in screens: further, it
/// cuts to that far off first.
const GLIDE_SCREENS: f32 = 3.0;
/// How long a glide takes, in seconds, from the shortest to the longest.
const GLIDE_TIME: (f32, f32) = (0.25, 0.5);
pub(super) const TAB_HEIGHT: f32 = 22.0;
const TAB_GAP: f32 = 3.0;
/// The time a tab takes to slide to its new place.
const TAB_SLIDE: f32 = 0.12;
const NAME_FONT: FontId = FontId::proportional(12.5);
const COUNT_FONT: FontId = FontId::monospace(9.0);
/// Room in a tab for ▶ and for ✕.
const PLAY_WIDTH: f32 = 12.0;
const CLOSE_WIDTH: f32 = 14.0;
/// The tab being renamed.
const RENAME_WIDTH: f32 = 152.0;
const TOOLBAR_HEIGHT: f32 = 18.0;
/// Between the tabs and the rows, and between the rows and the toolbar.
const LIST_GAP: f32 = 16.0;
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
    /// The playlist shown last frame, and the song to open another on,
    /// with the line above it at the top.
    shown: Option<PlaylistId>,
    open_at: Option<usize>,
    /// The song to glide to, placed as on opening (J), and the glide under way.
    glide_to: Option<usize>,
    glide: Option<Glide>,
}

/// A scroll that speeds up and slows down again.
struct Glide {
    from: f32,
    to: f32,
    start: f64,
    time: f32,
}

impl Glide {
    fn new(from: f32, to: f32, start: f64, height: f32) -> Self {
        let far = GLIDE_SCREENS * height;
        let from = from.clamp(to - far, to + far);
        let share = ((to - from).abs() / far.max(1.0)).min(1.0);
        let time = GLIDE_TIME.0 + (GLIDE_TIME.1 - GLIDE_TIME.0) * share;
        Self {
            from,
            to,
            start,
            time,
        }
    }

    /// Where it has got to at `now`, and whether it is done.
    fn at(&self, now: f64) -> (f32, bool) {
        let t = (((now - self.start) as f32) / self.time).clamp(0.0, 1.0);
        let eased = if t < 0.5 {
            4.0 * t * t * t
        } else {
            1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
        };
        (self.from + (self.to - self.from) * eased, t >= 1.0)
    }
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

/// Rows being dragged, in list order. Dropped on another tab, they are
/// copied there, or moved with Shift held.
pub(super) struct RowDrag(pub Vec<EntryId>);

/// What a row's right-click menu asked for, done to the selection.
#[derive(Clone, Copy)]
pub(super) enum Choice {
    /// Copies to this playlist, or to a new one with None.
    Copy(Option<PlaylistId>),
    /// Copies to a Spotify playlist, asking which first.
    CopyToSpotify,
    Move(Option<PlaylistId>),
    ToggleMarks,
    Remove,
}

/// A tab being dragged, and how far along it the pointer took hold.
struct TabDrag {
    id: PlaylistId,
    grab: f32,
}

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let onto_tab = tab_bar(ui, view, inputs, actions);
    let selection = &mut inputs.selection;
    let switched = selection.shown != Some(view.playlist.id);
    if switched {
        selection.shown = Some(view.playlist.id);
        selection.glide = None;
        let list = view.playlist;
        let at = list.cursor().and_then(|id| list.index_of(id));
        selection.open_at = Some(at.unwrap_or(0));
    }
    // The item spacing after the tabs makes up the rest of the gap.
    ui.add_space(LIST_GAP - ui.spacing().item_spacing.y);
    if view.playlist.id == PlaylistId::SPOTIFY
        && let Some(queue) = view.spotify
    {
        return spotify_view::draw(ui, view, queue, inputs, onto_tab, actions);
    }
    let list = view.playlist;
    let full = ui.available_rect_before_wrap();
    let toolbar_rect =
        Rect::from_min_max(pos2(full.left(), full.bottom() - TOOLBAR_HEIGHT), full.max);
    let rows = Rect::from_min_max(full.min, pos2(full.right(), toolbar_rect.top() - LIST_GAP));
    let bar = Scrollbar::new(ui, view, Id::new("playlist-scrollbar"));
    let (rows, slot) = bar.split(rows);

    if let Some((jump_list, entry)) = inputs.jump
        && jump_list == list.id
    {
        inputs.jump = None;
        if let Some(index) = list.index_of(entry) {
            let selection = &mut inputs.selection;
            selection.select(entry, index);
            // Placed as on opening the playlist, not just brought into view:
            // at once in a playlist just shown, gliding in the one already.
            selection.reveal = None;
            if switched {
                selection.open_at = Some(index);
            } else {
                selection.glide_to = Some(index);
            }
        }
    }
    inputs
        .selection
        .rows
        .retain(|&id| list.index_of(id).is_some());
    if inputs.renaming.is_none() && inputs.closing.is_none() && inputs.spotify_pick.is_none() {
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
            draw_rows(
                ui,
                view,
                &mut inputs.selection,
                (&bar, slot),
                onto_tab,
                actions,
            );
        }
    });
    ui.scope_builder(UiBuilder::new().max_rect(toolbar_rect), |ui| {
        toolbar(ui, view, inputs, actions);
    });
    confirm_close(ui, view, inputs, actions);
}

/// Moves a glide on, started by J or stopped by any other scroll, click
/// or key: where to scroll to this frame.
fn glide(ui: &Ui, selection: &mut Selection, line_of: &[usize], furthest: f32) -> Option<f32> {
    let (now, stopped) = ui.input(|i| {
        let moved = i.events.iter().any(|e| {
            matches!(
                e,
                egui::Event::Key { pressed: true, .. } | egui::Event::MouseWheel { .. }
            )
        });
        (i.time, moved || i.pointer.any_pressed())
    });
    if stopped {
        selection.glide = None;
    }
    if let Some(index) = selection.glide_to.take() {
        let line = line_of[index.min(line_of.len() - 1)].saturating_sub(1);
        let to = (line as f32 * ROW_HEIGHT).min(furthest);
        selection.glide = Some(Glide::new(selection.offset, to, now, ui.available_height()));
    }
    let (offset, done) = selection.glide.as_ref()?.at(now);
    if done {
        selection.glide = None;
    } else {
        ui.ctx().request_repaint();
    }
    Some(offset.min(furthest))
}

/// The tabs. Says whether rows being dragged are over another tab.
fn tab_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) -> bool {
    let palette = view.palette;
    let tabs: Vec<Tab> = view
        .tabs
        .iter()
        .filter_map(|&id| match (id, view.spotify) {
            (PlaylistId::SPOTIFY, Some(queue)) => Some(Tab {
                id,
                name: "Spotify",
                count: queue.len(),
                own: false,
            }),
            _ => view.library.get(id).map(|list| Tab {
                id,
                name: &list.name,
                count: list.len(),
                own: true,
            }),
        })
        .collect();
    let renaming = inputs.renaming.as_ref().map(|(id, _)| *id);
    let widths: Vec<f32> = tabs
        .iter()
        .map(|t| {
            if Some(t.id) == renaming {
                RENAME_WIDTH
            } else {
                tab_width(ui, view, t)
            }
        })
        .collect();
    let total = widths.iter().sum::<f32>() + TAB_GAP * widths.len().saturating_sub(1) as f32;
    let dragged = DragAndDrop::payload::<TabDrag>(ui.ctx()).and_then(|drag| {
        let from = tabs.iter().position(|t| t.id == drag.id)?;
        Some((from, drag.grab))
    });
    let mut drawn: Vec<(PlaylistId, Rect)> = Vec::new();
    let mut onto_tab = false;
    let bar = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = TAB_GAP;
            let (strip, _) = ui.allocate_exact_size(vec2(total, TAB_HEIGHT), Sense::hover());
            // The dragged tab follows the pointer along the strip.
            let floating = dragged.and_then(|(from, grab)| {
                let pointer = ui.ctx().pointer_interact_pos()?;
                let left = (pointer.x - grab - strip.left()).clamp(0.0, total - widths[from]);
                Some((from, left))
            });
            let (lefts, to) = tab_layout(&widths, floating);
            let order = (0..tabs.len())
                .filter(|&i| Some(i) != floating.map(|(from, _)| from))
                .chain(floating.map(|(from, _)| from));
            for i in order {
                let list = &tabs[i];
                let slide = if Some(i) == floating.map(|(from, _)| from) {
                    0.0
                } else {
                    TAB_SLIDE
                };
                let left = ui.ctx().animate_value_with_time(
                    Id::new(("tab-left", list.id)),
                    lefts[i],
                    slide,
                );
                let rect = Rect::from_min_size(
                    pos2(strip.left() + left, strip.top()),
                    vec2(widths[i], TAB_HEIGHT),
                );
                if Some(list.id) == renaming {
                    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                        rename_field(ui, list.id, &mut inputs.renaming, actions);
                    });
                    drawn.push((list.id, rect));
                    continue;
                }
                if slide == 0.0 && !skin_paint::has_chrome(ui.ctx(), "tab") {
                    // Over the others, it hides them.
                    ui.painter().rect_filled(rect, 0.0, palette.list_bg);
                }
                let response = tab(ui, view, list, rect, inputs, actions);
                onto_tab |= list.id != view.playlist.id
                    && response.dnd_hover_payload::<RowDrag>().is_some();
                if slide == 0.0
                    && response.drag_stopped()
                    && let Some(to) = to
                {
                    DragAndDrop::clear_payload(ui.ctx());
                    if to != i {
                        actions.push(Action::MovePlaylist(list.id, to));
                    }
                }
                drawn.push((list.id, rect));
            }
            // With the Spotify tab closed, + opens it too.
            let spotify_closed = view.settings.spotify_tab_enabled && !view.settings.spotify_tab;
            let plus = small_button(ui, &palette, "+", false);
            if !spotify_closed {
                if plus.on_hover_text("New playlist (Ctrl+T)").clicked() {
                    actions.push(Action::NewPlaylist);
                }
            } else {
                egui::Popup::menu(&plus).show(|ui| {
                    if text_button(ui, &palette, "NEW PLAYLIST", false, true)
                        .on_hover_text("Ctrl+T")
                        .clicked()
                    {
                        actions.push(Action::NewPlaylist);
                    }
                    if text_button(ui, &palette, "SPOTIFY", false, true).clicked() {
                        actions.push(Action::OpenSpotifyTab);
                    }
                });
            }
        })
        .response
        .rect;
    baseline(ui, view, &drawn, bar);
    onto_tab
}

/// Where each tab's left edge goes, from the strip's start, and, while one
/// is dragged (its index and left edge), the index it would move to. The
/// others make room for it: it passes one once it reaches that one's middle.
fn tab_layout(widths: &[f32], dragged: Option<(usize, f32)>) -> (Vec<f32>, Option<usize>) {
    let mut lefts = Vec::with_capacity(widths.len());
    let mut x = 0.0;
    for &width in widths {
        lefts.push(x);
        x += width + TAB_GAP;
    }
    let Some((from, left)) = dragged else {
        return (lefts, None);
    };
    let right = left + widths[from];
    let middle = |i: usize| lefts[i] + widths[i] / 2.0;
    let after = (from + 1..widths.len())
        .filter(|&i| right > middle(i))
        .count();
    let before = (0..from).filter(|&i| left < middle(i)).count();
    let to = from + after - before;
    let mut x = 0.0;
    let mut placed = vec![0.0; widths.len()];
    let others = (0..widths.len()).filter(|&i| i != from);
    for (n, i) in others.enumerate() {
        if n == to {
            x += widths[from] + TAB_GAP;
        }
        placed[i] = x;
        x += widths[i] + TAB_GAP;
    }
    placed[from] = left;
    (placed, Some(to))
}

/// The line the tabs stand on, across the panel, broken under the
/// shown tab so it opens into the list.
fn baseline(ui: &Ui, view: &View, tabs: &[(PlaylistId, Rect)], bar: Rect) {
    let shown = tabs.iter().find(|(id, _)| *id == view.playlist.id);
    tab_baseline(
        ui,
        &view.palette,
        shown.map(|(_, rect)| *rect),
        bar.bottom(),
    );
}

/// The line tabs stand on at `bottom`, across the panel, broken under
/// the `shown` tab's rect. A skin's tabs draw their own.
pub(super) fn tab_baseline(ui: &Ui, palette: &Palette, shown: Option<Rect>, bottom: f32) {
    if skin_paint::has_chrome(ui.ctx(), "tab") {
        return;
    }
    let stroke = Stroke::new(1.0, tab_edge(palette));
    let y = bottom - 0.5;
    let (left, right) = (ui.max_rect().left(), ui.max_rect().right());
    let painter = ui.painter();
    match shown {
        Some(shown) => {
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

/// A tab's playlist.
struct Tab<'a> {
    id: PlaylistId,
    name: &'a str,
    count: usize,
    /// A playlist of Spotless's own, rather than Spotify's queue: it can be
    /// renamed, moved and closed (not Default).
    own: bool,
}

/// The width of a tab: ▶ if playing, the name, the song count, ✕ unless
/// Default.
fn tab_width(ui: &Ui, view: &View, list: &Tab) -> f32 {
    let width = |text: &str, font: FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font, view.palette.chrome)
            .size()
            .x
    };
    let play_width = if view.playing_list == Some(list.id) {
        PLAY_WIDTH
    } else {
        0.0
    };
    let close_width = if list.id != PlaylistId::DEFAULT {
        CLOSE_WIDTH
    } else {
        0.0
    };
    10.0 + play_width
        + width(list.name, NAME_FONT)
        + 6.0
        + width(&list.count.to_string(), COUNT_FONT)
        + close_width
        + 10.0
}

/// A tab's frame in `rect`, and the colour for its name. Every tab is
/// drawn alike; the shown one opens into what's below the baseline
/// (`tab_baseline`), with a lit top edge and the bright text.
pub(super) fn paint_tab(
    ui: &Ui,
    palette: &Palette,
    rect: Rect,
    response: &egui::Response,
    shown: bool,
) -> Color32 {
    let painter = ui.painter();
    let state = skin_paint::state(response, shown, true);
    if skin_paint::chrome(painter, "tab", state, rect) {
        return palette.button_text;
    }
    let edge = Stroke::new(1.0, tab_edge(palette));
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
        palette.current
    } else if response.hovered() {
        palette.list_fg
    } else {
        palette.dim
    }
}

/// One tab, in `rect`. Rows dropped on it are copied there, or moved with
/// Shift held.
fn tab(
    ui: &mut Ui,
    view: &View,
    list: &Tab,
    rect: Rect,
    inputs: &mut Inputs,
    actions: &mut Actions,
) -> egui::Response {
    let palette = view.palette;
    let shown = list.id == view.playlist.id;
    let playing = view.playing_list == Some(list.id);
    let closable = list.id != PlaylistId::DEFAULT;
    let count = list.count.to_string();
    let play_width = if playing { PLAY_WIDTH } else { 0.0 };
    let response = ui.interact(rect, Id::new(("tab", list.id)), Sense::click_and_drag());
    let painter = ui.painter();
    let text_color = paint_tab(ui, &palette, rect, &response, shown);
    let count_color = palette.dim;
    let mut x = rect.left() + 10.0;
    if playing && view.player.status == Status::Playing {
        dancing_bars(ui, painter, pos2(x + 3.0, rect.center().y), &palette);
        x += play_width;
    } else if playing {
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
        list.name,
        NAME_FONT,
        text_color,
    );
    painter.text(
        pos2(name.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        count,
        COUNT_FONT,
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

    // Rows dragged from another tab: copied here when dropped, or moved.
    if !shown && response.dnd_hover_payload::<RowDrag>().is_some() {
        painter.rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(2.0, palette.accent),
            egui::StrokeKind::Inside,
        );
    }
    if !shown && let Some(rows) = response.dnd_release_payload::<RowDrag>() {
        let tracks = rows
            .0
            .iter()
            .filter_map(|&id| view.playlist.get(id))
            .map(|entry| entry.track.clone())
            .collect();
        let from = view.playlist.id;
        let movable =
            from != PlaylistId::SPOTIFY || view.spotify.is_some_and(|queue| queue.movable(&rows.0));
        if ui.input(|i| i.modifiers.shift) && movable {
            actions.push(Action::MoveTo(from, list.id, rows.0.clone()));
        } else {
            actions.push(Action::CopyTo(list.id, tracks));
        }
    }

    let close = |inputs: &mut Inputs, actions: &mut Actions| {
        // Closing the Spotify tab loses nothing: the queue is the account's.
        if list.count == 0 || !list.own {
            actions.push(Action::ClosePlaylist(list.id));
        } else {
            inputs.closing = Some(list.id);
        }
    };
    if list.own {
        response.context_menu(|ui| {
            if item(ui, "Rename", true).clicked() {
                inputs.renaming = Some((list.id, list.name.to_string()));
            }
            if item(ui, "Save to Spotify…", list.count > 0)
                .on_hover_text("Copies its songs to a new playlist on Spotify")
                .clicked()
                && let Some(songs) = view.library.get(list.id)
            {
                actions.push(Desktop::PickSpotifyPlaylist {
                    tracks: songs.entries().iter().map(|e| e.track.clone()).collect(),
                    name: list.name.to_string(),
                    new: true,
                });
            }
            if closable && item(ui, "Close", true).clicked() {
                close(inputs, actions);
            }
        });
    }
    if response.clicked() && over_close {
        close(inputs, actions);
    } else if response.double_clicked() && list.own {
        inputs.renaming = Some((list.id, list.name.to_string()));
    } else if response.clicked() {
        actions.push(Action::SelectPlaylist(list.id));
    }
    if response.drag_started()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        DragAndDrop::set_payload(
            ui.ctx(),
            TabDrag {
                id: list.id,
                grab: pointer.x - rect.left(),
            },
        );
    }
    if DragAndDrop::has_payload_of_type::<TabDrag>(ui.ctx()) {
        return response;
    }
    let hint = if !list.own {
        "The account's queue, as Spotify plays it. Closed, Spotless leaves Spotify Connect; + opens it again. Drag to move"
    } else {
        "Double-click to rename, drag to move"
    };
    response.on_hover_text_at_pointer(hint)
}

/// Three bars bouncing like a level meter, centred on `c`: the playing
/// tab's mark while a song plays (paused, it is ▶).
fn dancing_bars(ui: &Ui, painter: &egui::Painter, c: egui::Pos2, palette: &Palette) {
    let time = ui.input(|i| i.time);
    for (n, (speed, phase)) in [(7.0, 0.0), (9.5, 1.3), (6.0, 2.4)].into_iter().enumerate() {
        let height = 2.0 + 7.0 * (time * speed + phase).sin().abs() as f32;
        let x = c.x - 3.5 + n as f32 * 3.0;
        let bar = Rect::from_min_max(pos2(x, c.y + 4.0 - height), pos2(x + 2.0, c.y + 4.0));
        painter.rect_filled(bar, 0.0, palette.accent);
    }
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(60));
}

/// The tab being renamed: Enter or clicking away keeps the name, Esc
/// drops it.
fn rename_field(
    ui: &mut Ui,
    id: PlaylistId,
    renaming: &mut Option<(PlaylistId, String)>,
    actions: &mut Actions,
) -> Rect {
    let Some((_, name)) = renaming.as_mut() else {
        return Rect::NOTHING;
    };
    let field = field(
        ui,
        TextEdit::singleline(name)
            .id(Id::new(("rename", id)))
            .desired_width(RENAME_WIDTH - 12.0)
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

fn confirm_close(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
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
                small_button(ui, &palette, "KEEP", false)
                    .on_hover_text("Esc")
                    .clicked()
            })
            .inner
        });
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    if modal.inner || modal.backdrop_response.clicked() || escape {
        inputs.closing = None;
    }
}

fn toolbar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let list = view.playlist;
    let selected = selected(list, &inputs.selection);
    let any = !selected.is_empty();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let add = small_action(ui, &palette, "+ ADD", true);
        if tip(add, "Add songs from Browse (Ctrl+B)").clicked() {
            super::browse_view::opening(view, inputs);
            actions.push(Action::ToggleBrowse);
        }
        let rem = small_action(ui, &palette, "REM", any);
        if tip(rem, "Remove the selected songs (Delete)").clicked() {
            remove(list, &mut inputs.selection, selected.clone(), actions);
        }
        let next = small_action(ui, &palette, "Q NEXT", any);
        if tip(next, "Play the selected songs next, or not (Q)").clicked() {
            actions.push(Action::ToggleMarks(selected.clone()));
        }
        let up = small_glyph(ui, &palette, Glyph::Up, any);
        if tip(up, "Move the selected songs up (Alt+↑)").clicked() {
            move_by(list, &mut inputs.selection, selected.clone(), true, actions);
        }
        let down = small_glyph(ui, &palette, Glyph::Down, any);
        if tip(down, "Move the selected songs down (Alt+↓)").clicked() {
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

/// A line of the list: the header over a run, or a song, by its
/// index.
#[derive(Clone, Copy)]
enum Line {
    Header(Run),
    Song(usize),
}

/// The list's lines, and the line each song is on.
fn lines(list: &Playlist, runs: &[Run]) -> (Vec<Line>, Vec<usize>) {
    let mut lines = Vec::with_capacity(list.len());
    let mut line_of = Vec::with_capacity(list.len());
    for &run in runs {
        if run.header {
            lines.push(Line::Header(run));
        }
        for index in run.start..run.start + run.len {
            line_of.push(lines.len());
            lines.push(Line::Song(index));
        }
    }
    (lines, line_of)
}

fn draw_rows(
    ui: &mut Ui,
    view: &View,
    selection: &mut Selection,
    (bar, slot): (&Scrollbar, Rect),
    onto_tab: bool,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let list = view.playlist;
    let mut chosen = None;
    let mut area = ScrollArea::vertical()
        .auto_shrink([false, false])
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden);
    let runs = list.runs(view.settings.grouping);
    let (lines, line_of) = lines(list, &runs);
    // The songs after a header's run that aren't one: a line sets them off.
    let set_off: BTreeSet<usize> = runs
        .iter()
        .filter(|run| !run.header && run.start > 0)
        .map(|run| run.start)
        .collect();
    let furthest = (lines.len() as f32 * ROW_HEIGHT - ui.available_height()).max(0.0);
    let thumb_to = bar.interact(ui, slot, selection.offset, furthest);
    if let Some(index) = selection.reveal.take() {
        let top = line_of[index.min(list.len() - 1)] as f32 * ROW_HEIGHT;
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
    if let Some(index) = selection.open_at.take() {
        let line = line_of[index.min(list.len() - 1)].saturating_sub(1);
        area = area.vertical_scroll_offset((line as f32 * ROW_HEIGHT).min(furthest));
    }
    if let Some(offset) = thumb_to {
        area = area.vertical_scroll_offset(offset);
    }
    if thumb_to.is_some() {
        selection.glide = None;
    }
    if let Some(offset) = glide(ui, selection, &line_of, furthest) {
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
    let output = area.show_rows(ui, ROW_HEIGHT, lines.len(), |ui, range| {
        let viewport = ui.clip_rect();
        for line in range {
            let index = match lines[line] {
                Line::Song(index) => index,
                Line::Header(run) => {
                    let (rect, choice) = header(ui, view, selection, run, actions);
                    chosen = chosen.or(choice);
                    if dragging.is_some()
                        && let Some(p) = pointer
                        && viewport.contains(p)
                        && rect.y_range().contains(p.y)
                    {
                        drop_at = Some((run.start, rect.bottom()));
                    }
                    continue;
                }
            };
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
            if set_off.contains(&index) {
                painter.hline(
                    rect.x_range(),
                    rect.top() + 0.5,
                    Stroke::new(1.0, palette.lcd.gamma_multiply(0.3)),
                );
            }

            if response.double_clicked() {
                selection.select(entry.id, index);
                selection.reveal = None;
                actions.push(Action::PlayEntry(list.id, entry.id));
            } else if response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                click(list, selection, entry.id, modifiers);
            } else if response.secondary_clicked() && !selected {
                selection.select(entry.id, index);
                selection.reveal = None;
            }
            let marked = response.context_menu_opened() && all_marked(list, selection);
            chosen = chosen.or(row_menu(&response, view, Menu::own(marked)));
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
    if let Some(choice) = chosen {
        send(list, selection, choice, actions);
    }

    let Some(dragging) = dragging else { return };
    let label = drag_label(ui, dragging.0.len(), onto_tab, true);
    drag_ghost(ui, &palette, &label);
    // Below the last row: the end of the list.
    if drop_at.is_none()
        && let Some(p) = pointer
        && output.inner_rect.contains(p)
    {
        drop_at = Some((
            list.len(),
            output.inner_rect.top() - output.state.offset.y + lines.len() as f32 * ROW_HEIGHT,
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

/// The background of a run's header.
fn header_bg(palette: &Palette) -> Color32 {
    palette.lcd.gamma_multiply(0.12)
}

/// The header over a run: the name of its album, or of where its songs
/// came from, and its artist or what it is. A click selects the run (Ctrl adds or takes it away); a double-click
/// plays it; a right-click selects it and opens the rows' menu.
fn header(
    ui: &mut Ui,
    view: &View,
    selection: &mut Selection,
    run: Run,
    actions: &mut Actions,
) -> (Rect, Option<Choice>) {
    let palette = view.palette;
    let list = view.playlist;
    let songs = &list.entries()[run.start..run.start + run.len];
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let chosen = songs.iter().all(|entry| selection.rows.contains(&entry.id));
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        0.0,
        if chosen {
            palette.selected_bg
        } else {
            header_bg(&palette)
        },
    );
    let mark = Rect::from_center_size(pos2(rect.left() + 6.0, rect.center().y), vec2(2.0, 10.0));
    painter.rect_filled(mark, 0.0, palette.accent);
    let (name, detail) = list.heading(run, view.settings.grouping);
    let font = FontId::monospace(10.0);
    let name = painter.text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        name.to_uppercase(),
        font.clone(),
        palette.lcd,
    );
    if let Some(detail) = detail {
        painter.text(
            pos2(name.right(), rect.center().y),
            Align2::LEFT_CENTER,
            format!("  ·  {}", detail.to_uppercase()),
            font,
            palette.dim,
        );
    }
    let first = songs[0].id;
    if response.double_clicked() {
        selection.select(first, run.start);
        selection.reveal = None;
        actions.push(Action::PlayEntry(list.id, first));
        return (rect, None);
    }
    if response.secondary_clicked() && !chosen {
        selection.rows = songs.iter().map(|entry| entry.id).collect();
        selection.anchor = Some(first);
        selection.cursor = Some(first);
    }
    let marked = response.context_menu_opened() && all_marked(list, selection);
    let choice = row_menu(&response, view, Menu::own(marked));
    if response.clicked() {
        let ids = songs.iter().map(|entry| entry.id);
        if ui.input(|i| i.modifiers.command) {
            if chosen {
                for id in ids {
                    selection.rows.remove(&id);
                }
            } else {
                selection.rows.extend(ids);
            }
        } else {
            selection.rows = ids.collect();
        }
        selection.anchor = Some(first);
        selection.cursor = Some(first);
    }
    (rect, choice)
}

/// Every selected row is marked to play next.
fn all_marked(list: &Playlist, selection: &Selection) -> bool {
    selection
        .rows
        .iter()
        .all(|&id| list.mark_number(id).is_some())
}

/// What the ghost following dragged rows says: over another tab, whether
/// they are copied or moved there (with Shift, if they `can_move`).
pub(super) fn drag_label(ui: &Ui, count: usize, onto_tab: bool, can_move: bool) -> String {
    let songs = match count {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    };
    match (onto_tab, can_move, ui.input(|i| i.modifiers.shift)) {
        (false, ..) => songs,
        (true, true, true) => format!("Move {songs}"),
        (true, true, false) => format!("Copy {songs} · Shift moves"),
        (true, false, _) => format!("Copy {songs}"),
    }
}

/// What the right-click menu can do to the selected rows.
pub(super) struct Menu {
    /// Why Move to can't, when it can't.
    pub unmovable: Option<&'static str>,
    /// For a playlist's own rows (Play next): whether they're all marked.
    pub marked: Option<bool>,
    pub can_remove: bool,
}

impl Menu {
    /// For a playlist's own rows, which can always move or go.
    fn own(marked: bool) -> Self {
        Self {
            unmovable: None,
            marked: Some(marked),
            can_remove: true,
        }
    }
}

/// An item of a right-click menu.
fn item(ui: &mut Ui, name: &str, enabled: bool) -> egui::Response {
    let label = RichText::new(name).monospace().size(11.0);
    ui.add_enabled(enabled, egui::Button::new(label).frame_when_inactive(false))
}

/// The right-click menu of the selected rows: Copy to, Move to, Play next
/// for a playlist's own rows, and Remove.
pub(super) fn row_menu(response: &egui::Response, view: &View, menu: Menu) -> Option<Choice> {
    let mut chosen = None;
    let targets = |ui: &mut Ui,
                   chosen: &mut Option<Choice>,
                   choice: fn(Option<PlaylistId>) -> Choice| {
        let others = view.tabs.iter().filter(|&&id| id != view.playlist.id);
        for &id in others {
            let (name, takes) = match (id, view.spotify) {
                (PlaylistId::SPOTIFY, Some(queue)) => ("Spotify", queue.takes_songs()),
                _ => match view.library.get(id) {
                    Some(list) => (list.name.as_str(), true),
                    None => continue,
                },
            };
            let response = item(ui, name, takes).on_disabled_hover_text(
                "Spotify takes songs for the queue only from the device playing it. TAKE OVER the queue first.",
            );
            if response.clicked() {
                *chosen = Some(choice(Some(id)));
            }
        }
        ui.separator();
        if item(ui, "New playlist", true).clicked() {
            *chosen = Some(choice(None));
        }
    };
    response.context_menu(|ui| {
        ui.menu_button(RichText::new("Copy to").monospace().size(11.0), |ui| {
            targets(ui, &mut chosen, Choice::Copy);
            if item(ui, "Spotify playlist…", true).clicked() {
                chosen = Some(Choice::CopyToSpotify);
            }
        });
        match menu.unmovable {
            None => {
                ui.menu_button(RichText::new("Move to").monospace().size(11.0), |ui| {
                    targets(ui, &mut chosen, Choice::Move);
                });
            }
            Some(why) => {
                item(ui, "Move to", false).on_disabled_hover_text(why);
            }
        }
        ui.separator();
        if let Some(marked) = menu.marked {
            let label = if marked {
                "Don't play next"
            } else {
                "Play next"
            };
            if item(ui, label, true).clicked() {
                chosen = Some(Choice::ToggleMarks);
            }
        }
        if item(ui, "Remove", menu.can_remove).clicked() {
            chosen = Some(Choice::Remove);
        }
    });
    chosen
}

/// Does what a playlist's row menu asked for to the selection.
fn send(list: &Playlist, selection: &mut Selection, choice: Choice, actions: &mut Actions) {
    let rows = selected(list, selection);
    let tracks = || {
        rows.iter()
            .filter_map(|&id| list.get(id))
            .map(|entry| entry.track.clone())
            .collect()
    };
    match choice {
        Choice::Copy(Some(to)) => actions.push(Action::CopyTo(to, tracks())),
        Choice::Copy(None) => actions.push(Action::NewPlaylistWith(tracks())),
        Choice::CopyToSpotify => actions.push(Desktop::PickSpotifyPlaylist {
            tracks: tracks(),
            name: list.name.clone(),
            new: false,
        }),
        Choice::Move(Some(to)) => {
            actions.push(Action::MoveTo(list.id, to, rows.clone()));
            select_after(list, selection, &rows);
        }
        Choice::Move(None) => {
            actions.push(Action::NewPlaylistWith(tracks()));
            remove(list, selection, rows, actions);
        }
        Choice::ToggleMarks => actions.push(Action::ToggleMarks(rows)),
        Choice::Remove => remove(list, selection, rows, actions),
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
fn remove(list: &Playlist, selection: &mut Selection, rows: Vec<EntryId>, actions: &mut Actions) {
    if select_after(list, selection, &rows) {
        actions.push(Action::Remove(list.id, rows));
    }
}

/// Selects the row after the last of `rows`, which are going; says whether
/// there were any.
fn select_after(list: &Playlist, selection: &mut Selection, rows: &[EntryId]) -> bool {
    let Some(last) = rows.iter().filter_map(|&id| list.index_of(id)).max() else {
        return false;
    };
    let next = list.entries()[last + 1..]
        .iter()
        .chain(list.entries()[..last].iter().rev())
        .map(|entry| entry.id)
        .find(|id| !selection.rows.contains(id));
    selection.rows = next.into_iter().collect();
    selection.anchor = next;
    selection.cursor = next;
    true
}

/// Moves the selection one row up or down.
fn move_by(
    list: &Playlist,
    selection: &mut Selection,
    rows: Vec<EntryId>,
    up: bool,
    actions: &mut Actions,
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
fn keyboard(ui: &Ui, list: &Playlist, selection: &mut Selection, actions: &mut Actions) {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use egui::{Event, PointerButton, Pos2, RawInput, Vec2};

    use super::*;
    use crate::action::AnyAction;
    use crate::backend::{SearchState, SessionState};
    use crate::browse::{Browse, Loadable};
    use crate::lastfm::LastFmState;
    use crate::model::Library;
    use crate::paths::Paths;
    use crate::playback::engine::PlayerSnapshot;
    use crate::spotify::queue::SpotifyQueue;
    use crate::store::Settings;

    /// Drags from `from` on the tab bar by `by`, in steps, and releases:
    /// the actions the tab bar pushed meanwhile. With `spotify`, the
    /// Spotify tab comes first.
    fn drag_tab(library: &Library, spotify: bool, from: f32, by: Vec2) -> Vec<AnyAction> {
        let root = std::env::temp_dir().join(format!("spotless-tabs-{}", std::process::id()));
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let settings = Settings::load(&paths);
        let player = PlayerSnapshot::default();
        let browse = Browse::new(PlaylistId::DEFAULT, false);
        let covers = HashMap::new();
        let lastfm = LastFmState::default();
        let outputs = Loadable::Loading;
        let spotify_list = Playlist::new(PlaylistId::SPOTIFY, "Spotify");
        let queue = SpotifyQueue::default();
        let tabs: Vec<_> = spotify
            .then_some(PlaylistId::SPOTIFY)
            .into_iter()
            .chain(library.playlists().iter().map(|l| l.id))
            .collect();
        let view = View {
            palette: Palette::CLASSIC,
            session: &SessionState::SignedOut,
            player: &player,
            error: None,
            notice: None,
            playlist: library.get(PlaylistId::DEFAULT).unwrap(),
            shuffle: false,
            repeat: false,
            stop_after: false,
            library,
            tabs: &tabs,
            playing_list: None,
            spotify: spotify.then_some(&queue),
            spotify_list: &spotify_list,
            browse: &browse,
            covers: &covers,
            search: &SearchState::SignedOut,
            lastfm: &lastfm,
            web_client_id: None,
            settings: &settings,
            storage: None,
            outputs: &outputs,
            tray_shown: false,
            art: None,
            skin_note: None,
            skin_error: None,
            update: &crate::update::Status::Unknown,
        };
        let ctx = egui::Context::default();
        let mut inputs = Inputs::default();
        let mut actions = Actions::default();
        let mut time = 0.0;
        let mut frame = |events: Vec<Event>, actions: &mut Actions| {
            time += 0.2;
            let raw = RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                tab_bar(ui, &view, &mut inputs, actions);
            });
            output.textures_delta.clear();
        };
        let start = pos2(from, 11.0);
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        frame(vec![Event::PointerMoved(start)], &mut actions);
        frame(vec![button(start, true)], &mut actions);
        let steps = 10;
        for n in 1..=steps {
            let pos = start + by * n as f32 / steps as f32;
            frame(vec![Event::PointerMoved(pos)], &mut actions);
        }
        let end = start + by;
        frame(vec![button(end, false)], &mut actions);
        frame(vec![], &mut actions);
        let _ = std::fs::remove_dir_all(&root);
        actions.into_iter().collect()
    }

    #[test]
    fn a_glide_eases_from_where_it_was_to_the_song() {
        let glide = Glide::new(0.0, 100.0, 10.0, 400.0);
        assert_eq!(glide.at(10.0), (0.0, false));
        let (early, _) = glide.at(10.0 + f64::from(glide.time) / 4.0);
        let (middle, _) = glide.at(10.0 + f64::from(glide.time) / 2.0);
        assert!(early < 25.0, "slow to start: {early}");
        assert!((middle - 50.0).abs() < 0.01);
        assert_eq!(glide.at(20.0), (100.0, true));
    }

    #[test]
    fn a_long_glide_starts_a_few_screens_off_and_takes_no_longer() {
        let glide = Glide::new(0.0, 50_000.0, 0.0, 400.0);
        assert_eq!(glide.from, 50_000.0 - GLIDE_SCREENS * 400.0);
        assert_eq!(glide.time, GLIDE_TIME.1);
        assert!(Glide::new(0.0, 10.0, 0.0, 400.0).time < GLIDE_TIME.1);
    }

    #[test]
    fn the_others_make_room_once_the_dragged_tab_reaches_their_middle() {
        let widths = [100.0, 60.0, 80.0];
        let (lefts, to) = tab_layout(&widths, None);
        assert_eq!((lefts, to), (vec![0.0, 103.0, 166.0], None));
        // The first tab, 20 to the right: its right edge is short of the
        // second's middle.
        assert_eq!(tab_layout(&widths, Some((0, 20.0))).1, Some(0));
        // 40 to the right, past it: the second slides into its place.
        let (lefts, to) = tab_layout(&widths, Some((0, 40.0)));
        assert_eq!((lefts, to), (vec![40.0, 0.0, 166.0], Some(1)));
        // The last tab, all the way left.
        let (lefts, to) = tab_layout(&widths, Some((2, 0.0)));
        assert_eq!((lefts, to), (vec![83.0, 186.0, 0.0], Some(0)));
    }

    #[test]
    fn a_tab_dragged_past_the_next_one_moves() {
        let mut library = Library::new();
        let mix = library.create("Mix");
        let moved = |spotify, from, by| {
            drag_tab(&library, spotify, from, by)
                .into_iter()
                .filter(|a| matches!(a, AnyAction::Core(Action::MovePlaylist(..))))
                .collect::<Vec<_>>()
        };
        let to = |id, at| vec![AnyAction::Core(Action::MovePlaylist(id, at))];
        assert_eq!(
            moved(false, 20.0, vec2(150.0, 0.0)),
            to(PlaylistId::DEFAULT, 1)
        );
        // Let go below the bar, it still moves.
        assert_eq!(
            moved(false, 20.0, vec2(150.0, 120.0)),
            to(PlaylistId::DEFAULT, 1)
        );
        // Not far enough: nothing.
        assert_eq!(moved(false, 20.0, vec2(10.0, 0.0)), []);
        // The Spotify tab moves too, and Mix to its place.
        assert_eq!(
            moved(true, 20.0, vec2(400.0, 0.0)),
            to(PlaylistId::SPOTIFY, 2)
        );
        assert_eq!(moved(true, 200.0, vec2(-300.0, 0.0)), to(mix, 0));
    }
}
