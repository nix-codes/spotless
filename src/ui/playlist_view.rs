//! The playlist editor: a bar to add songs, the rows, and a footer.
//!
//! Click selects (Ctrl toggles, Shift extends), double-click or Enter
//! plays, Delete removes, Q marks to play next, Alt+Up/Down moves the
//! selection, Ctrl+A selects all.

use std::collections::BTreeSet;

use egui::{
    Align, Align2, FontId, Key, Layout, Modifiers, Rect, RichText, ScrollArea, Sense, TextEdit, Ui,
    pos2, vec2,
};

use super::widgets::{BUTTON_HEIGHT, text_button};
use super::{Inputs, View};
use crate::action::Action;
use crate::model::{EntryId, Playlist};

const ROW_HEIGHT: f32 = 18.0;
const FOOTER_HEIGHT: f32 = 18.0;

/// The editor's own state, kept between frames.
#[derive(Default)]
pub struct Selection {
    pub rows: BTreeSet<EntryId>,
    /// Where Shift+click and the arrow keys extend from.
    anchor: Option<EntryId>,
    /// The scroll offset last frame, and the row to bring into view.
    offset: f32,
    reveal: Option<usize>,
}

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    add_bar(ui, view, inputs, actions);
    ui.add_space(8.0);
    let list = view.playlist;
    let full = ui.available_rect_before_wrap();
    let rows = Rect::from_min_max(full.min, pos2(full.right(), full.bottom() - FOOTER_HEIGHT));
    let footer = Rect::from_min_max(pos2(full.left(), rows.bottom()), full.max);

    inputs
        .selection
        .rows
        .retain(|&id| list.index_of(id).is_some());
    keyboard(ui, list, &mut inputs.selection, actions);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rows), |ui| {
        if list.is_empty() {
            ui.label(
                RichText::new("This playlist is empty. Paste a Spotify link above to add songs.")
                    .color(view.palette.dim),
            );
        } else {
            draw_rows(ui, view, &mut inputs.selection, actions);
        }
    });
    draw_footer(ui, footer, view);
}

fn add_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    ui.horizontal(|ui| {
        let field = ui.add(
            TextEdit::singleline(&mut inputs.link)
                .hint_text("Paste a Spotify track, album or artist link")
                .desired_width((ui.available_width() - 160.0).max(120.0))
                .min_size(vec2(0.0, BUTTON_HEIGHT)),
        );
        let link = inputs.link.trim().to_string();
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let clicked = text_button(ui, &palette, "ADD", false, !link.is_empty()).clicked();
        if (submitted || clicked) && !link.is_empty() {
            actions.push(Action::AddLink(link));
            inputs.link.clear();
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if text_button(ui, &palette, "SIGN OUT", false, true).clicked() {
                actions.push(Action::SignOut);
            }
        });
    });
}

fn draw_rows(ui: &mut Ui, view: &View, selection: &mut Selection, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let list = view.playlist;
    let mut area = ScrollArea::vertical().auto_shrink([false, false]);
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
    let number_width = list.len().to_string().len();
    let current = list.cursor().filter(|_| !list.detached());
    let output = area.show_rows(ui, ROW_HEIGHT, list.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let entry = &list.entries()[index];
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
            let selected = selection.rows.contains(&entry.id);
            let painter = ui.painter_at(rect);
            if selected {
                painter.rect_filled(rect, 0.0, palette.selected_bg);
            }
            let color = if Some(entry.id) == current {
                palette.current
            } else {
                palette.list_fg
            };
            let mono = FontId::monospace(11.0);
            let middle = rect.left_center();
            painter.text(
                middle + vec2(4.0, 0.0),
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
                    mono.clone(),
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

            if response.double_clicked() {
                selection.rows = BTreeSet::from([entry.id]);
                selection.anchor = Some(entry.id);
                actions.push(Action::PlayEntry(entry.id));
            } else if response.clicked() {
                let modifiers = ui.input(|i| i.modifiers);
                click(list, selection, entry.id, modifiers);
            }
        }
    });
    selection.offset = output.state.offset.y;
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
}

/// The editor's keys, unless a text field has the keyboard.
fn keyboard(ui: &Ui, list: &Playlist, selection: &mut Selection, actions: &mut Vec<Action>) {
    if ui.ctx().memory(|m| m.focused()).is_some() || list.is_empty() {
        return;
    }
    let selected: Vec<EntryId> = list
        .entries()
        .iter()
        .map(|entry| entry.id)
        .filter(|id| selection.rows.contains(id))
        .collect();
    let indices: Vec<usize> = selected
        .iter()
        .filter_map(|&id| list.index_of(id))
        .collect();
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
        ];
        (
            keys.into_iter().find(|&key| i.key_pressed(key)),
            i.modifiers,
        )
    });
    let Some(key) = pressed else { return };
    match key {
        Key::Enter => {
            if let Some(&first) = selected.first() {
                actions.push(Action::PlayEntry(first));
            }
        }
        Key::Delete if !selected.is_empty() => {
            // The selection moves to the row after the last removed one.
            let last = indices.iter().copied().max().unwrap_or(0);
            let next = list.entries()[last + 1..]
                .iter()
                .chain(list.entries()[..last].iter().rev())
                .map(|entry| entry.id)
                .find(|id| !selection.rows.contains(id));
            selection.rows = next.into_iter().collect();
            selection.anchor = next;
            actions.push(Action::Remove(selected));
        }
        Key::Q if !selected.is_empty() => actions.push(Action::ToggleMarks(selected)),
        Key::A if modifiers.command => {
            selection.rows = list.entries().iter().map(|entry| entry.id).collect();
        }
        Key::ArrowUp | Key::ArrowDown if modifiers.alt && !selected.is_empty() => {
            let (first, last) = (indices[0], *indices.last().unwrap_or(&0));
            if key == Key::ArrowUp && first > 0 {
                actions.push(Action::Move(selected, first - 1));
                selection.reveal = Some(first - 1);
            } else if key == Key::ArrowDown && last + 1 < list.len() {
                actions.push(Action::Move(selected, last + 2));
                selection.reveal = Some(last + 1);
            }
        }
        Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End => {
            let from = selection
                .anchor
                .and_then(|id| list.index_of(id))
                .or(list.cursor().and_then(|id| list.index_of(id)));
            let last = list.len() - 1;
            let to = match (key, from) {
                (Key::Home, _) => 0,
                (Key::End, _) => last,
                (Key::ArrowUp, Some(i)) => i.saturating_sub(1),
                (Key::ArrowDown, Some(i)) => (i + 1).min(last),
                _ => 0,
            };
            let id = list.entries()[to].id;
            selection.rows = BTreeSet::from([id]);
            selection.anchor = Some(id);
            selection.reveal = Some(to);
        }
        _ => {}
    }
}

fn draw_footer(ui: &Ui, rect: Rect, view: &View) {
    let list = view.playlist;
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
    ui.painter().text(
        rect.left_bottom(),
        Align2::LEFT_BOTTOM,
        format!("{} · {songs} · {time}", list.name.to_uppercase()),
        FontId::monospace(10.0),
        view.palette.dim,
    );
}
