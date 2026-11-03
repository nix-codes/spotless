//! The tabs: one per open playlist, scrolled when they don't all fit,
//! dragged to reorder and renamed in place; and the dialogs they open.

use egui::{
    Align2, Color32, DragAndDrop, FontId, Id, Key, Modifiers, Rect, RichText, Sense, Stroke,
    TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::menu::item;
use super::{RowDrag, SCROLL_EDGE};
use crate::action::{Actions, Desktop, PlaylistsAction, SpotifyTabAction, SyncAction};
use crate::ui::dialog::{self, Answer};
use crate::ui::skin_paint;
use crate::ui::theme::{self, Palette};
use crate::ui::widgets::{field, small_action, small_button, small_button_width, text_button};
use crate::ui::{Inputs, View};
use spotless_core::SyncStatus;
use spotless_core::model::PlaylistId;
use spotless_core::playback::Status;
use spotless_core::session::SessionState;

const TAB_HEIGHT: f32 = 22.0;
const TAB_GAP: f32 = 3.0;
/// The time a tab takes to slide to its new place.
const TAB_SLIDE: f32 = 0.12;
const NAME_FONT: FontId = FontId::proportional(12.5);
const COUNT_FONT: FontId = FontId::monospace(9.0);
/// Room in a tab for ▶ and for ✕.
const PLAY_WIDTH: f32 = 12.0;
/// A synced tab's mark, and the gaps either side.
const SYNC_WIDTH: f32 = 14.0;
const CLOSE_WIDTH: f32 = 14.0;
/// The tab being renamed.
const RENAME_WIDTH: f32 = 152.0;

/// A tab being dragged, and how far along it the pointer took hold.
struct TabDrag {
    id: PlaylistId,
    grab: f32,
}

/// The tabs. Says whether rows being dragged are over another tab.
pub(super) fn tab_bar(
    ui: &mut Ui,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) -> bool {
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
                sync: None,
            }),
            _ => view.library.get(id).map(|list| Tab {
                id,
                name: &list.name,
                count: list.len(),
                own: true,
                sync: list.synced().then(|| match view.sync.get(&id) {
                    Some(SyncStatus::Waiting) => Sync::Waiting,
                    Some(SyncStatus::Failed(_)) => Sync::Failed,
                    Some(SyncStatus::UpToDate) | None => Sync::UpToDate,
                }),
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
            // Too many to fit: ‹ › and the wheel scroll them.
            let plus_width = small_button_width(ui, "+") + TAB_GAP;
            let arrows_width = small_button_width(ui, "‹") + small_button_width(ui, "›");
            let overflow = total + TAB_GAP + plus_width > ui.available_width();
            let width = if overflow {
                (ui.available_width() - plus_width - arrows_width - 2.0 * TAB_GAP).max(0.0)
            } else {
                total
            };
            let back =
                overflow.then(|| small_action(ui, &palette, "‹", inputs.tab_scroll.to > 0.0));
            let (strip, strip_response) =
                ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::hover());
            let ahead = overflow
                .then(|| small_action(ui, &palette, "›", inputs.tab_scroll.to < total - width));
            let at = tab_layout(&widths, None).0;
            let scroll = &mut inputs.tab_scroll;
            // The shown tab in sight: there already on opening, glided to
            // after.
            if let Some(shown) = tabs.iter().position(|t| t.id == view.playlist.id)
                && (scroll.shown != Some(view.playlist.id) || scroll.width != width)
            {
                scroll.to = scroll_to_show(at[shown], widths[shown], scroll.to, width);
                scroll.shown = Some(view.playlist.id);
                scroll.width = width;
            }
            if back.is_some_and(|b| b.on_hover_text("Earlier tabs").clicked()) {
                scroll.to = scroll_step(&at, &widths, scroll.to, width, false);
            }
            if ahead.is_some_and(|a| a.on_hover_text("Later tabs").clicked()) {
                scroll.to = scroll_step(&at, &widths, scroll.to, width, true);
            }
            if overflow && strip_response.contains_pointer() {
                let delta = ui.input(|i| i.smooth_scroll_delta);
                scroll.to -= delta.x + delta.y;
            }
            // A tab or songs dragged near an end scroll it.
            let dragging = DragAndDrop::has_payload_of_type::<TabDrag>(ui.ctx())
                || DragAndDrop::has_payload_of_type::<RowDrag>(ui.ctx());
            if overflow
                && dragging
                && let Some(pointer) = ui.ctx().pointer_interact_pos()
                && (strip.top()..=strip.bottom()).contains(&pointer.y)
            {
                let speed = 360.0 * ui.input(|i| i.stable_dt).min(0.1);
                if pointer.x < strip.left() + SCROLL_EDGE {
                    scroll.to -= speed;
                    ui.ctx().request_repaint();
                } else if pointer.x > strip.right() - SCROLL_EDGE {
                    scroll.to += speed;
                    ui.ctx().request_repaint();
                }
            }
            scroll.to = scroll.to.clamp(0.0, (total - width).max(0.0));
            let offset = if dragging {
                scroll.to
            } else {
                ui.ctx()
                    .animate_value_with_time(Id::new("tab-scroll"), scroll.to, TAB_SLIDE)
            };
            let mut clipped = ui.new_child(UiBuilder::new().max_rect(strip));
            clipped.set_clip_rect(strip.intersect(ui.clip_rect()));
            {
                let ui = &mut clipped;
                let origin = strip.left() - offset;
                // The dragged tab follows the pointer along the strip.
                let floating = dragged.and_then(|(from, grab)| {
                    let pointer = ui.ctx().pointer_interact_pos()?;
                    let left = (pointer.x - grab - origin).clamp(0.0, total - widths[from]);
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
                        pos2(origin + left, strip.top()),
                        vec2(widths[i], TAB_HEIGHT),
                    );
                    if Some(list.id) == renaming {
                        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                            rename_field(ui, list.id, &mut inputs.renaming, actions);
                        });
                        drawn.push((list.id, rect.intersect(strip)));
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
                            actions.push(PlaylistsAction::MovePlaylist(list.id, to));
                        }
                    }
                    drawn.push((list.id, rect.intersect(strip)));
                }
            }
            // With the Spotify tab closed, + opens it too.
            let spotify_closed = view.settings.spotify_tab_enabled && !view.settings.spotify_tab;
            let plus = small_button(ui, &palette, "+", false);
            if !spotify_closed {
                if plus.on_hover_text("New playlist (Ctrl+T)").clicked() {
                    actions.push(PlaylistsAction::NewPlaylist);
                }
            } else {
                egui::Popup::menu(&plus).show(|ui| {
                    if text_button(ui, &palette, "NEW PLAYLIST", false, true)
                        .on_hover_text("Ctrl+T")
                        .clicked()
                    {
                        actions.push(PlaylistsAction::NewPlaylist);
                    }
                    if text_button(ui, &palette, "SPOTIFY", false, true).clicked() {
                        actions.push(SpotifyTabAction::OpenSpotifyTab);
                    }
                });
            }
        })
        .response
        .rect;
    baseline(ui, view, &drawn, bar);
    onto_tab
}

/// How far the tabs are scrolled when they don't all fit.
#[derive(Default)]
pub struct TabScroll {
    /// The offset scrolled to, which the strip glides to.
    to: f32,
    /// The tab last brought into sight as the one shown, and the strip's
    /// width then: a narrower window brings it into sight again.
    shown: Option<PlaylistId>,
    width: f32,
}

/// The offset from `offset` that puts a tab at `left`, `width` wide, in a
/// strip `strip` wide, moving no more than that takes.
fn scroll_to_show(left: f32, width: f32, offset: f32, strip: f32) -> f32 {
    if left < offset {
        left
    } else if left + width > offset + strip {
        left + width - strip
    } else {
        offset
    }
}

/// The offset from `offset` that brings the next tab cut off or hidden,
/// later ones if `ahead`, wholly into a strip `strip` wide.
fn scroll_step(lefts: &[f32], widths: &[f32], offset: f32, strip: f32, ahead: bool) -> f32 {
    let tabs = lefts.iter().zip(widths);
    let next = if ahead {
        tabs.map(|(l, w)| l + w)
            .find(|&right| right > offset + strip + 0.5)
            .map(|right| right - strip)
    } else {
        tabs.map(|(l, _)| *l)
            .rev()
            .find(|&left| left < offset - 0.5)
    };
    next.unwrap_or(offset)
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
pub(crate) fn tab_baseline(ui: &Ui, palette: &Palette, shown: Option<Rect>, bottom: f32) {
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
    /// Synced: how it stands (None: not synced).
    sync: Option<Sync>,
}

/// A synced tab's mark.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sync {
    UpToDate,
    Waiting,
    Failed,
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
    let sync_width = if list.sync.is_some() { SYNC_WIDTH } else { 0.0 };
    10.0 + play_width
        + width(list.name, NAME_FONT)
        + sync_width
        + 6.0
        + width(&list.count.to_string(), COUNT_FONT)
        + CLOSE_WIDTH
        + 10.0
}

/// A tab's frame in `rect`, and the colour for its name. Every tab is
/// drawn alike; the shown one opens into what's below the baseline
/// (`tab_baseline`), with a lit top edge and the bright text.
pub(crate) fn paint_tab(
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
    let mut after = name.right();
    if let Some(sync) = list.sync {
        let color = match sync {
            Sync::UpToDate => palette.accent,
            Sync::Waiting => palette.dim,
            Sync::Failed => theme::WARNING,
        };
        theme::sync_mark(
            painter,
            pos2(after + SYNC_WIDTH / 2.0, rect.center().y),
            color,
        );
        after += SYNC_WIDTH;
    }
    painter.text(
        pos2(after + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        count,
        COUNT_FONT,
        count_color,
    );
    let close =
        Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), vec2(10.0, 10.0));
    let pointer = ui.ctx().pointer_interact_pos();
    let over_close = pointer.is_some_and(|p| close.expand(2.0).contains(p));
    let color = if over_close && response.hovered() {
        palette.accent
    } else {
        palette.dim
    };
    let stroke = Stroke::new(1.2, color);
    let c = close.shrink(2.0);
    painter.line_segment([c.left_top(), c.right_bottom()], stroke);
    painter.line_segment([c.right_top(), c.left_bottom()], stroke);

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
            actions.push(PlaylistsAction::MoveTo(from, list.id, rows.0.clone()));
        } else {
            actions.push(PlaylistsAction::CopyTo(list.id, tracks));
        }
    }

    let close = |inputs: &mut Inputs, actions: &mut Actions| {
        // Closing the Spotify tab loses nothing: the queue is the account's,
        // nor a synced one, which stays on Spotify.
        if list.count == 0 || !list.own || list.sync.is_some() {
            actions.push(PlaylistsAction::ClosePlaylist(list.id));
        } else {
            inputs.closing = Some(list.id);
        }
    };
    if list.own {
        response.context_menu(|ui| {
            if item(ui, "Rename", true).clicked() {
                inputs.renaming = Some((list.id, list.name.to_string()));
            }
            let synced = list.sync.is_some();
            let signed_in = matches!(view.session, SessionState::Ready { .. });
            let label = if synced { "Stop syncing" } else { "Sync" };
            let hint = match (synced, signed_in) {
                (_, false) => "Sign in to sync it with your other devices",
                (false, true) => "Keeps it the same on your other devices",
                (true, true) => "Stops syncing: it's deleted from Spotify and stays only here",
            };
            if item(ui, label, signed_in).on_hover_text(hint).clicked() {
                actions.push(SyncAction::SetSync(list.id, !synced));
            }
            if item(ui, "Export to Spotify…", list.count > 0)
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
            if item(ui, "Close", true).clicked() {
                close(inputs, actions);
            }
        });
    }
    if response.clicked() && over_close {
        close(inputs, actions);
    } else if response.double_clicked() && list.own {
        inputs.renaming = Some((list.id, list.name.to_string()));
    } else if response.clicked() {
        actions.push(PlaylistsAction::SelectPlaylist(list.id));
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
        "The account's queue, as Spotify plays it. Closed, Spotless leaves Spotify Connect; + opens it again. Drag to move".to_string()
    } else {
        let sync = match (list.sync, view.sync.get(&list.id)) {
            (None, _) => "",
            (Some(Sync::Waiting), _) => "Synced: changes wait to be sent.\n",
            (Some(Sync::Failed), Some(SyncStatus::Failed(why))) => {
                return response.on_hover_text_at_pointer(format!(
                    "Synced, but Spotify refused a change: {why}\nDouble-click to rename, drag to move"
                ));
            }
            (Some(_), _) => "Synced: the same on your other devices.\n",
        };
        format!("{sync}Double-click to rename, drag to move")
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
            actions.push(PlaylistsAction::RenamePlaylist(id, name.trim().to_string()));
        }
        // Its Enter, not the list's too: that would play the song.
        ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
        *renaming = None;
    } else if !field.has_focus() {
        field.request_focus();
    }
    field.rect
}

pub(super) fn confirm_close(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some(list) = inputs.closing.and_then(|id| view.library.get(id)) else {
        inputs.closing = None;
        return;
    };
    let palette = view.palette;
    let buttons = [("CLOSE PLAYLIST", "Enter"), ("KEEP", "Esc")];
    let answer = dialog::show(ui, &palette, Id::new("close-playlist"), &buttons, |ui| {
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
    });
    if answer == Answer::Chose(0) {
        actions.push(PlaylistsAction::ClosePlaylist(list.id));
    }
    if answer != Answer::Waiting {
        inputs.closing = None;
    }
}

/// A playlist wasn't synced, the Spotless folder having one by its name:
/// RENAME opens its tab's name field (it's synced once renamed), REPLACE IT
/// deletes that one first. Hidden while the name is typed.
pub(super) fn confirm_sync(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some(list) = view.sync_name_taken.and_then(|id| view.library.get(id)) else {
        return;
    };
    if inputs.renaming.is_some() {
        return;
    }
    let palette = view.palette;
    let buttons = [
        ("RENAME", "Enter"),
        (
            "REPLACE IT",
            "Deletes the one on Spotify, then syncs this one",
        ),
        ("CANCEL", "Esc"),
    ];
    let answer = dialog::show(ui, &palette, Id::new("sync-name-taken"), &buttons, |ui| {
        ui.set_max_width(360.0);
        ui.label(
            RichText::new(format!("Sync “{}”?", list.name))
                .size(15.0)
                .color(palette.current),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "A synced playlist called “{}” is already on Spotify. Rename this one, \
                 or replace that one: your other devices keep it, no longer synced.",
                list.name
            ))
            .color(palette.chrome),
        );
    });
    match answer {
        Answer::Chose(0) => inputs.renaming = Some((list.id, list.name.to_string())),
        Answer::Chose(1) => actions.push(SyncAction::ReplaceSynced(list.id)),
        Answer::Waiting => {}
        _ => actions.push(SyncAction::DismissSyncNameTaken),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use egui::{Event, PointerButton, Pos2, RawInput, Vec2};

    use super::*;
    use crate::action::{Action, AnyAction};
    use spotless_core::Paths;
    use spotless_core::browse::{Browse, Loadable};
    use spotless_core::lastfm::LastFmState;
    use spotless_core::model::{Library, Playlist};
    use spotless_core::playback::PlayerSnapshot;
    use spotless_core::session::{SearchState, SessionState};
    use spotless_core::settings::Settings;
    use spotless_core::spotify::SpotifyQueue;

    /// Drags from `from` on the tab bar by `by`, in steps, and releases:
    /// the actions the tab bar pushed meanwhile. With `spotify`, the
    /// Spotify tab comes first.
    fn drag_tab(library: &Library, spotify: bool, from: f32, by: Vec2) -> Vec<AnyAction> {
        let root = std::env::temp_dir().join(format!("spotless-tabs-{}", std::process::id()));
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let settings = Settings::load(&paths);
        let desktop = crate::settings::DesktopSettings::read(settings.extra());
        let analyser = spotless_core::playback::Analyser::default();
        let player = PlayerSnapshot::default();
        let browse = Browse::new(PlaylistId::DEFAULT, false);
        let covers = HashMap::new();
        let lastfm = LastFmState::default();
        let loved = spotless_core::lastfm::Loved::default();
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
            can_undo_reorder: false,
            playlist: library.get(PlaylistId::DEFAULT).unwrap(),
            left_ms: None,
            shuffle: false,
            repeat: false,
            stop_after: false,
            library,
            sync: &std::collections::HashMap::new(),
            sync_name_taken: None,
            tabs: &tabs,
            playing_list: None,
            spotify: spotify.then_some(&queue),
            spotify_list: &spotify_list,
            browse: &browse,
            covers: &covers,
            search: &SearchState::SignedOut,
            lastfm: &lastfm,
            loved: &loved,
            web_client_id: None,
            settings: &settings,
            desktop: &desktop,
            analyser: &analyser,
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
    fn the_shown_tab_scrolls_into_sight_no_further_than_needed() {
        // Tabs at 0..50, 53..153 and 156..206, in a strip 100 wide.
        assert_eq!(scroll_to_show(53.0, 100.0, 0.0, 100.0), 53.0);
        assert_eq!(scroll_to_show(156.0, 50.0, 0.0, 100.0), 106.0);
        assert_eq!(scroll_to_show(0.0, 50.0, 106.0, 100.0), 0.0);
        assert_eq!(scroll_to_show(156.0, 50.0, 120.0, 100.0), 120.0);
    }

    #[test]
    fn the_arrows_bring_the_next_cut_off_tab_wholly_into_sight() {
        let (lefts, widths) = ([0.0, 53.0, 156.0], [50.0, 100.0, 50.0]);
        // The second tab is cut off on the right, then the third.
        assert_eq!(scroll_step(&lefts, &widths, 0.0, 100.0, true), 53.0);
        assert_eq!(scroll_step(&lefts, &widths, 53.0, 100.0, true), 106.0);
        assert_eq!(scroll_step(&lefts, &widths, 106.0, 100.0, true), 106.0);
        // Back: the second tab's start, then the first's.
        assert_eq!(scroll_step(&lefts, &widths, 106.0, 100.0, false), 53.0);
        assert_eq!(scroll_step(&lefts, &widths, 53.0, 100.0, false), 0.0);
        assert_eq!(scroll_step(&lefts, &widths, 0.0, 100.0, false), 0.0);
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
                .filter(|a| {
                    matches!(
                        a,
                        AnyAction::Core(Action::Playlists(PlaylistsAction::MovePlaylist(..)))
                    )
                })
                .collect::<Vec<_>>()
        };
        let to = |id, at| {
            vec![AnyAction::Core(Action::Playlists(
                PlaylistsAction::MovePlaylist(id, at),
            ))]
        };
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
