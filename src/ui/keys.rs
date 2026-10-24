//! The keys that work everywhere, and the F1 screen that lists every key.
//!
//! [`global`] runs before the views draw and consumes the keys it handles,
//! so a view never acts on them too. The views' own keys (moving through a
//! playlist, the catalog's Back and Esc) stay in the views; [`HELP`] lists
//! them all, so keep it in step with both.

use egui::{
    Align, Event, Id, InputState, Key, Layout, Modifiers, RichText, Sense, Stroke, Ui, vec2,
};

use super::scrollbar::{self, Scrollbar};
use super::theme::{self, Palette};
use super::widgets::text_button;
use super::{Inputs, View};
use crate::action::{Action, Actions, Desktop};
use crate::backend::SessionState;
use crate::model::PlaylistId;
use crate::playback::engine::Status;

/// ← and →; with Shift, the far step.
const SEEK_MS: i64 = 5_000;
const FAR_SEEK_MS: i64 = 30_000;
const VOLUME_STEP: u8 = 5;

/// The F1 screen: sections of (keys, what they do). One binding a row,
/// and a few words for each: the screen is read at a glance.
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "PLAYBACK",
        &[
            ("Space", "Play / pause"),
            ("V", "Stop"),
            ("← / →", "Back / forward 5 seconds"),
            ("Shift+← / →", "Back / forward 30 seconds"),
            ("Ctrl+← / →", "Previous / next song"),
            ("+ / −", "Volume up / down"),
            ("Ctrl+↑ / ↓", "Volume up / down"),
            ("M", "Mute / unmute"),
            ("S", "Shuffle on / off"),
            ("R", "Repeat on / off"),
            ("J", "Jump to the playing song"),
        ],
    ),
    (
        "PLAYLISTS AND SCREENS",
        &[
            ("Ctrl+Tab", "Next playlist"),
            ("Ctrl+Shift+Tab", "Previous playlist"),
            ("Ctrl+PgDn / PgUp", "Next / previous playlist"),
            ("Ctrl+1 … 9", "Playlist 1 … 9"),
            ("Ctrl+T", "New playlist"),
            ("Ctrl+W", "Close the tab"),
            ("F2", "Rename the playlist"),
            ("Ctrl+B", "Browse / playlists"),
            ("Ctrl+F", "Search"),
            ("Ctrl+V", "Add a pasted Spotify link"),
            ("F10", "Settings"),
            ("F1 or ?", "This list"),
            ("Ctrl+Q", "Quit (closing only hides)"),
        ],
    ),
    (
        "IN A PLAYLIST",
        &[
            ("↑ / ↓", "Select a song"),
            ("PgUp / PgDn", "Select a page up / down"),
            ("Home / End", "Select the first / last"),
            ("Shift+any of those", "Extend the selection"),
            ("Ctrl+A", "Select all"),
            ("Enter", "Play the selected song"),
            ("Delete", "Remove the selection"),
            ("Alt+↑ / ↓", "Move the selection"),
            ("Q", "Play next (mark / unmark)"),
        ],
    ),
    (
        "IN BROWSE",
        &[
            ("Tab / Shift+Tab", "Next / previous area"),
            ("↑ / ↓", "Highlight a row"),
            ("PgUp / PgDn", "A page up / down"),
            ("Enter", "Search; open; play a song"),
            ("Insert or A", "Add the album or song"),
            ("Delete", "Remove from the playlist"),
            ("Alt+1 … 4", "All, Albums, Singles, Appears on"),
            ("Ctrl+S", "Oldest / newest first"),
            ("Ctrl+E", "Show / hide all songs"),
            ("C", "Pics on / off"),
            ("Ctrl+Tab, Ctrl+1 … 9", "The playlist + ADD adds to"),
            ("Backspace / Alt+←", "Back"),
            ("Esc", "Leave the search, then Browse"),
        ],
    ),
];

pub fn global(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let ctx = ui.ctx();
    // Typing in a field, or a question waiting for an answer: the keys
    // belong to them.
    if ctx.text_edit_focused() || inputs.closing.is_some() || inputs.renaming.is_some() {
        return;
    }
    ui.input_mut(|i| {
        if take(i, Modifiers::NONE, Key::F1) || i.consume_key(Modifiers::NONE, Key::Questionmark) {
            inputs.help = !inputs.help;
            return;
        }
        if take(i, Modifiers::COMMAND, Key::Q) {
            actions.push(Desktop::Quit);
            return;
        }
        // Ctrl+, too, as in most desktop apps.
        if take(i, Modifiers::NONE, Key::F10) || take(i, Modifiers::COMMAND, Key::Comma) {
            super::toggle_settings(view, inputs, actions);
            return;
        }
        if inputs.help {
            if take(i, Modifiers::NONE, Key::Escape) {
                inputs.help = false;
            }
            return;
        }
        if matches!(view.session, SessionState::Ready { .. }) {
            playback(i, view, inputs, actions);
            screens(i, view, inputs, actions);
        }
    });
}

fn playback(i: &mut InputState, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let player = view.player;
    // Space pauses or resumes what the player shows, in whichever tab;
    // with nothing to, it plays the song selected in the tab in view.
    if take(i, Modifiers::NONE, Key::Space) {
        let list = view.playlist;
        let selected = list
            .entries()
            .iter()
            .map(|entry| entry.id)
            .find(|id| inputs.selection.rows.contains(id));
        actions.push(match (player.status, selected) {
            (Status::Stopped, Some(row)) => Action::PlayEntry(list.id, row),
            (Status::Stopped, None) => Action::Play,
            _ => Action::TogglePause,
        });
    }
    // Winamp's key for Stop.
    if take(i, Modifiers::NONE, Key::V) && player.status != Status::Stopped {
        actions.push(Action::Stop);
    }
    if take(i, Modifiers::COMMAND, Key::ArrowLeft) {
        actions.push(Action::Previous);
    }
    if take(i, Modifiers::COMMAND, Key::ArrowRight) {
        actions.push(Action::Next);
    }

    let mut seek = 0;
    for (modifiers, step) in [(Modifiers::NONE, SEEK_MS), (Modifiers::SHIFT, FAR_SEEK_MS)] {
        seek -= step * held(i, modifiers, Key::ArrowLeft) as i64;
        seek += step * held(i, modifiers, Key::ArrowRight) as i64;
    }
    if seek != 0
        && player.status != Status::Stopped
        && let Some(track) = &player.track
    {
        let to = (i64::from(player.position_now()) + seek).clamp(0, i64::from(track.duration_ms));
        actions.push(Action::Seek(to as u32));
    }

    // + is Shift+= on many layouts; = alone works too.
    let up = i.count_and_consume_key(Modifiers::NONE, Key::Plus)
        + held(i, Modifiers::NONE, Key::Equals)
        + held(i, Modifiers::COMMAND, Key::ArrowUp);
    let down = held(i, Modifiers::NONE, Key::Minus) + held(i, Modifiers::COMMAND, Key::ArrowDown);
    if up + down > 0 {
        let step = i32::from(VOLUME_STEP);
        let volume = (i32::from(inputs.volume) + step * (up as i32 - down as i32)).clamp(0, 100);
        set_volume(inputs, volume as u8, actions);
    }
    if take(i, Modifiers::NONE, Key::M) {
        if inputs.volume > 0 {
            let volume = inputs.volume;
            set_volume(inputs, 0, actions);
            inputs.unmuted = Some(volume);
        } else {
            let volume = inputs.unmuted.unwrap_or(50);
            set_volume(inputs, volume, actions);
        }
    }

    if take(i, Modifiers::NONE, Key::S) {
        actions.push(Action::SetShuffle(!view.shuffle));
    }
    if take(i, Modifiers::NONE, Key::R) {
        actions.push(Action::SetRepeat(!view.repeat));
    }
    if take(i, Modifiers::NONE, Key::J)
        && let Some(list) = view.playing_list.and_then(|id| view.list(id))
        && let Some(entry) = list.cursor()
    {
        actions.push(Action::CloseBrowse);
        actions.push(Action::SelectPlaylist(list.id));
        inputs.jump = Some((list.id, entry));
    }
}

fn screens(i: &mut InputState, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let browsing = view.browse.open;
    // In Browse the playlist keys choose where + ADD adds to.
    let current = if browsing {
        view.browse.target
    } else {
        view.playlist.id
    };
    let lists = view.tabs;
    let choose = |list: PlaylistId| {
        if browsing {
            Action::SetTarget(list)
        } else {
            Action::SelectPlaylist(list)
        }
    };
    let at = lists.iter().position(|&id| id == current).unwrap_or(0);
    let next = held(i, Modifiers::COMMAND, Key::Tab) + held(i, Modifiers::COMMAND, Key::PageDown);
    let previous = held(i, Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab)
        + held(i, Modifiers::COMMAND, Key::PageUp);
    let step = next as isize - previous as isize;
    if step != 0 {
        let to = (at as isize + step).rem_euclid(lists.len() as isize) as usize;
        actions.push(choose(lists[to]));
    }
    const NUMBERS: [Key; 9] = [
        Key::Num1,
        Key::Num2,
        Key::Num3,
        Key::Num4,
        Key::Num5,
        Key::Num6,
        Key::Num7,
        Key::Num8,
        Key::Num9,
    ];
    for (n, key) in NUMBERS.into_iter().enumerate() {
        if take(i, Modifiers::COMMAND, key)
            && let Some(&list) = lists.get(n)
        {
            actions.push(choose(list));
        }
    }

    if take(i, Modifiers::COMMAND, Key::T) {
        actions.push(Action::CloseBrowse);
        actions.push(Action::NewPlaylist);
    }
    if take(i, Modifiers::NONE, Key::F2) {
        actions.push(Action::CloseBrowse);
        inputs.renaming = Some((view.playlist.id, view.playlist.name.clone()));
    }
    if !browsing && take(i, Modifiers::COMMAND, Key::W) && current != PlaylistId::DEFAULT {
        if view.playlist.is_empty() || current == PlaylistId::SPOTIFY {
            actions.push(Action::ClosePlaylist(current));
        } else {
            inputs.closing = Some(current);
        }
    }
    if take(i, Modifiers::COMMAND, Key::B) {
        actions.push(Action::ToggleBrowse);
    }
    if take(i, Modifiers::COMMAND, Key::F) {
        if !browsing {
            actions.push(Action::ToggleBrowse);
        }
        inputs.focus_search = true;
    }

    // Ctrl+V arrives as a paste. Text that isn't a Spotify link is left alone.
    i.events.retain(|event| match event {
        Event::Paste(text) if crate::spotify::catalog::parse_uri(text.trim()).is_ok() => {
            actions.push(Action::AddLink(text.trim().to_string()));
            false
        }
        _ => true,
    });
}

fn set_volume(inputs: &mut Inputs, volume: u8, actions: &mut Actions) {
    inputs.volume = volume;
    inputs.unmuted = None;
    actions.push(Action::Volume(volume));
}

/// Consumes one press of exactly this key and these modifiers, not a
/// repeat from holding it down.
fn take(i: &mut InputState, modifiers: Modifiers, key: Key) -> bool {
    consume(i, modifiers, key, false) > 0
}

/// Consumes the presses of exactly this key and these modifiers, repeats
/// included, and counts them.
fn held(i: &mut InputState, modifiers: Modifiers, key: Key) -> usize {
    consume(i, modifiers, key, true)
}

fn consume(i: &mut InputState, modifiers: Modifiers, key: Key, repeats: bool) -> usize {
    let mut count = 0;
    i.events.retain(|event| {
        let Event::Key {
            key: pressed,
            modifiers: held,
            pressed: true,
            repeat,
            ..
        } = event
        else {
            return true;
        };
        if *pressed != key || !held.matches_exact(modifiers) {
            return true;
        }
        if repeats || !repeat {
            count += 1;
        }
        // Repeats of a single-press key are swallowed too.
        false
    });
    count
}

const KEYS_WIDTH: f32 = 160.0;
const WHAT_WIDTH: f32 = 215.0;
/// Between a section's keys and what they do.
const COLUMN_GAP: f32 = 14.0;
/// Inside a section's panel.
const PAD: f32 = 10.0;
/// Between the panels.
const GAP: f32 = 10.0;
/// Between a section's rows.
const ROW_GAP: f32 = 5.0;

/// The F1 screen: each section in a panel of its own, two side by side
/// when the window is wide enough, one under another otherwise. It takes
/// the window's height, and scrolls when that isn't enough (about 700
/// points show every key at once).
pub fn help(ui: &Ui, view: &View, inputs: &mut Inputs) {
    if !inputs.help {
        return;
    }
    let palette = &view.palette;
    let id = Id::new("keys-help-scrollbar");
    let slot = Scrollbar::new(ui, view, id).width() + scrollbar::GAP;
    let panel = KEYS_WIDTH + COLUMN_GAP + WHAT_WIDTH + 2.0 * PAD;
    let screen = ui.ctx().content_rect();
    // The dialog's own margins and edge take the rest.
    let two = screen.width() >= 2.0 * panel + GAP + slot + 48.0;
    let width = if two { 2.0 * panel + GAP } else { panel } + slot;
    let modal = egui::Modal::new(Id::new("keys-help"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            // The title and the close button on one line, as in Settings,
            // so the keys get the height.
            let close = ui
                .horizontal(|ui| {
                    ui.set_width(width);
                    ui.label(RichText::new("Keys").size(15.0).color(palette.current));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        text_button(ui, palette, "ESC · CLOSE", false, true).clicked()
                    })
                    .inner
                })
                .inner;
            ui.add_space(8.0);
            // What's left of the window after the dialog's margins and title,
            // from the first frame on: the dialog's place isn't known yet then.
            let height = (screen.height() - 90.0).max(120.0);
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                scrollbar::show(ui, view, id, egui::ScrollArea::vertical(), |area, ui| {
                    area.show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                        if two {
                            // Keys for everywhere on the left, for one screen on
                            // the right; the panels side by side are as tall as
                            // each other, so the rows of panels line up.
                            for (row, pair) in [[0, 2], [1, 3]].into_iter().enumerate() {
                                let id = Id::new(("keys-help-row", row));
                                let height = ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0);
                                let tallest = ui
                                    .horizontal_top(|ui| {
                                        pair.map(|i| section(ui, palette, &HELP[i], height))
                                            .into_iter()
                                            .fold(0.0, f32::max)
                                    })
                                    .inner;
                                if tallest != height {
                                    ui.data_mut(|d| d.insert_temp(id, tallest));
                                    ui.ctx().request_repaint();
                                }
                            }
                        } else {
                            for entry in HELP {
                                section(ui, palette, entry, 0.0);
                            }
                        }
                    })
                });
            });
            close
        });
    if modal.inner || modal.backdrop_response.clicked() {
        inputs.help = false;
    }
}

/// One section's panel, at least `height` tall inside. Returns how tall its
/// title and rows are.
fn section(
    ui: &mut Ui,
    palette: &Palette,
    (title, keys): &(&str, &[(&str, &str)]),
    height: f32,
) -> f32 {
    let mut content = 0.0;
    let frame = egui::Frame::new()
        .fill(palette.list_bg)
        .inner_margin(PAD)
        .show(ui, |ui| {
            // Top to bottom, also when the panels sit side by side.
            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                ui.set_width(KEYS_WIDTH + COLUMN_GAP + WHAT_WIDTH);
                ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
                let top = ui.cursor().top();
                // The title, with the window title bar's grooves after it.
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(*title)
                            .monospace()
                            .size(11.0)
                            .color(palette.chrome),
                    );
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 7.0), Sense::hover());
                    theme::stripes(ui.painter(), rect, palette);
                });
                ui.add_space(8.0);
                egui::Grid::new(*title)
                    .num_columns(2)
                    .spacing([COLUMN_GAP, ROW_GAP])
                    .min_col_width(KEYS_WIDTH)
                    .max_col_width(WHAT_WIDTH)
                    .show(ui, |ui| {
                        for (keys, what) in *keys {
                            ui.label(RichText::new(*keys).monospace().color(palette.list_fg));
                            ui.label(RichText::new(*what).color(palette.chrome));
                            ui.end_row();
                        }
                    });
                content = ui.cursor().top() - top;
                if height > content {
                    ui.add_space(height - content);
                }
            });
        });
    theme::bevel(ui.painter(), frame.response.rect, false, palette);
    content
}
