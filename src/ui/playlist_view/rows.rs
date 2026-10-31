//! The rows of the playlist shown: headers over runs, dragging, and the
//! glide to the playing song.

use std::collections::BTreeSet;

use egui::scroll_area::ScrollBarVisibility;
use egui::{Align2, Color32, DragAndDrop, FontId, Rect, ScrollArea, Sense, Stroke, Ui, pos2, vec2};

use super::menu::{Choice, Menu, all_loved, all_marked, row_menu, selected_tracks, send};
use super::selection::{Selection, click};
use super::{RowDrag, SCROLL_EDGE};
use crate::action::{Action, Actions};
use crate::ui::View;
use crate::ui::scrollbar::Scrollbar;
use crate::ui::theme::{self, Palette};
use crate::ui::widgets::{drag_ghost, row_heart, row_length};
use spotless_core::model::{EntryId, Playlist, Run};

const ROW_HEIGHT: f32 = 18.0;
/// The longest a jump to the playing song glides, in screens: further, it
/// cuts to that far off first.
const GLIDE_SCREENS: f32 = 3.0;
/// How long a glide takes, in seconds, from the shortest to the longest.
const GLIDE_TIME: (f32, f32) = (0.25, 0.5);

/// A scroll that speeds up and slows down again.
pub(super) struct Glide {
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

/// Moves a glide on, started by J or stopped by any other scroll, click
/// or key: where to scroll to this frame.
pub(super) fn glide(
    ui: &Ui,
    selection: &mut Selection,
    line_of: &[usize],
    furthest: f32,
) -> Option<f32> {
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

pub(super) fn draw_rows(
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
            paint_row(&painter, rect, view, list, index, number_width, current);
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
            let open = response.context_menu_opened();
            let marked = open && all_marked(list, selection);
            let loved = all_loved(view, open, selected_tracks(list, selection));
            chosen = chosen.or(row_menu(
                &response,
                view,
                Menu::own(marked, loved, selection.rows.len() > 1),
            ));
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
        send(list, selection, view.loved, choice, actions);
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
    let open = response.context_menu_opened();
    let marked = open && all_marked(list, selection);
    let loved = all_loved(view, open, selected_tracks(list, selection));
    let choice = row_menu(
        &response,
        view,
        Menu::own(marked, loved, selection.rows.len() > 1),
    );
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

fn paint_row(
    painter: &egui::Painter,
    rect: Rect,
    view: &View,
    list: &Playlist,
    index: usize,
    number_width: usize,
    current: Option<EntryId>,
) {
    let palette = view.palette;
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
    let mut text_right = row_length(painter, rect, entry.track.duration_ms, color) - 10.0;
    if view.loved.track(&entry.track) {
        text_right = row_heart(painter, text_right, rect.center().y, color) - 8.0;
    }
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

/// What the ghost following dragged rows says: over another tab, whether
/// they are copied or moved there (with Shift, if they `can_move`).
pub(crate) fn drag_label(ui: &Ui, count: usize, onto_tab: bool, can_move: bool) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
