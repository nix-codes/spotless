//! The keys that work everywhere, and the F1 screen that lists every key.
//!
//! [`global`] runs before the views draw and consumes the keys it handles,
//! so a view never acts on them too. The views' own keys (moving through a
//! playlist, the catalog's Back and Esc) stay in the views; [`HELP`] lists
//! them all, so keep it in step with both.

use egui::{Event, Id, InputState, Key, Modifiers, RichText, Stroke, Ui};

use super::theme::Palette;
use super::widgets::small_button;
use super::{Inputs, View};
use crate::action::Action;
use crate::backend::SessionState;
use crate::model::PlaylistId;
use crate::playback::engine::Status;

/// ← and →; with Shift, the far step.
const SEEK_MS: i64 = 5_000;
const FAR_SEEK_MS: i64 = 30_000;
const VOLUME_STEP: u8 = 5;

/// The F1 screen: sections of (keys, what they do).
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "PLAYBACK",
        &[
            ("Space", "Play / pause"),
            ("← / →", "Back / forward 5 seconds"),
            ("Shift+← / →", "Back / forward 30 seconds"),
            ("Ctrl+← / →", "Previous / next song"),
            ("+ / −, Ctrl+↑ / ↓", "Volume up / down"),
            ("M", "Mute / unmute"),
            ("S", "Shuffle on / off"),
            ("R", "Repeat on / off"),
            ("J", "Jump to the playing song"),
        ],
    ),
    (
        "PLAYLISTS AND SCREENS",
        &[
            ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous playlist"),
            ("Ctrl+1 … 9", "Playlist 1 … 9"),
            ("Ctrl+T", "New playlist"),
            ("Ctrl+W", "Close the playlist"),
            ("F2", "Rename the playlist"),
            ("Ctrl+B", "Browse / playlists"),
            ("Ctrl+F", "Search"),
            ("Ctrl+V", "Add a pasted Spotify link"),
            ("F1 or ?", "This list"),
        ],
    ),
    (
        "IN A PLAYLIST",
        &[
            ("↑ / ↓, Home / End", "Select a song"),
            ("Enter", "Play the selected song"),
            ("Delete", "Remove the selection"),
            ("Q", "Play next (mark / unmark)"),
            ("Alt+↑ / ↓", "Move the selection"),
            ("Ctrl+A", "Select all"),
        ],
    ),
    (
        "IN BROWSE",
        &[
            ("Enter", "Search (in the search field)"),
            ("Ctrl+Tab, Ctrl+1 … 9", "Choose the playlist + ADD adds to"),
            ("Backspace / Alt+←", "Back"),
            ("Esc", "Leave the search field, then close Browse"),
        ],
    ),
];

pub fn global(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
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

fn playback(i: &mut InputState, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let player = view.player;
    if take(i, Modifiers::NONE, Key::Space) {
        actions.push(match player.status {
            Status::Stopped => Action::Play,
            _ => Action::TogglePause,
        });
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
        && let Some(list) = view.playing_list.and_then(|id| view.library.get(id))
        && let Some(entry) = list.cursor()
    {
        actions.push(Action::CloseBrowse);
        actions.push(Action::SelectPlaylist(list.id));
        inputs.jump = Some((list.id, entry));
    }
}

fn screens(i: &mut InputState, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let browsing = view.browse.open;
    // In Browse the playlist keys choose where + ADD adds to.
    let current = if browsing {
        view.browse.target
    } else {
        view.playlist.id
    };
    let lists = view.library.playlists();
    let choose = |list: PlaylistId| {
        if browsing {
            Action::SetTarget(list)
        } else {
            Action::SelectPlaylist(list)
        }
    };
    let at = lists.iter().position(|l| l.id == current).unwrap_or(0);
    let step = held(i, Modifiers::COMMAND, Key::Tab) as isize
        - held(i, Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab) as isize;
    if step != 0 {
        let to = (at as isize + step).rem_euclid(lists.len() as isize) as usize;
        actions.push(choose(lists[to].id));
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
            && let Some(list) = lists.get(n)
        {
            actions.push(choose(list.id));
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
        if view.playlist.is_empty() {
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

fn set_volume(inputs: &mut Inputs, volume: u8, actions: &mut Vec<Action>) {
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

/// The F1 screen: the sections in two columns when the window is wide
/// enough, one otherwise; it scrolls when the window is too short.
pub fn help(ui: &Ui, palette: &Palette, inputs: &mut Inputs) {
    if !inputs.help {
        return;
    }
    const KEYS_WIDTH: f32 = 170.0;
    const WHAT_WIDTH: f32 = 210.0;
    const GAP: f32 = 28.0;
    let column = KEYS_WIDTH + 12.0 + WHAT_WIDTH;
    let screen = ui.ctx().content_rect();
    let two = screen.width() >= 2.0 * column + GAP + 80.0;
    let modal = egui::Modal::new(Id::new("keys-help"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.label(RichText::new("Keys").size(15.0).color(palette.current));
            ui.add_space(4.0);
            let section = |ui: &mut Ui, (title, keys): &(&str, &[(&str, &str)])| {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(*title)
                        .monospace()
                        .size(10.0)
                        .color(palette.dim),
                );
                egui::Grid::new(*title)
                    .num_columns(2)
                    .spacing([12.0, 3.0])
                    .min_col_width(KEYS_WIDTH)
                    .max_col_width(WHAT_WIDTH)
                    .show(ui, |ui| {
                        for (keys, what) in *keys {
                            ui.label(RichText::new(*keys).monospace().color(palette.list_fg));
                            ui.label(RichText::new(*what).color(palette.chrome));
                            ui.end_row();
                        }
                    });
            };
            egui::ScrollArea::vertical()
                .max_height((screen.height() - 140.0).max(120.0))
                .show(ui, |ui| {
                    if two {
                        // Playback with the playlist's keys, screens with Browse's.
                        ui.horizontal_top(|ui| {
                            ui.vertical(|ui| {
                                ui.set_width(column);
                                section(ui, &HELP[0]);
                                section(ui, &HELP[2]);
                            });
                            ui.add_space(GAP);
                            ui.vertical(|ui| {
                                ui.set_width(column);
                                section(ui, &HELP[1]);
                                section(ui, &HELP[3]);
                            });
                        });
                    } else {
                        ui.set_width(column);
                        for entry in HELP {
                            section(ui, entry);
                        }
                    }
                });
            ui.add_space(12.0);
            small_button(ui, palette, "CLOSE", false).clicked()
        });
    if modal.inner || modal.backdrop_response.clicked() {
        inputs.help = false;
    }
}
