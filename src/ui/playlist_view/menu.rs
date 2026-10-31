//! The rows' right-click menu, shared with the Spotify tab, and what it
//! does to the selection.

use egui::{RichText, Ui};

use super::selection::{Selection, remove, select_after, selected};
use crate::action::{Action, Actions, Desktop};
use crate::ui::View;
use crate::ui::widgets::share_menu;
use spotless_core::lastfm::{LastFmStatus, Loved};
use spotless_core::model::{Playlist, PlaylistId, TrackRef};

/// What a row's right-click menu asked for, done to the selection.
#[derive(Clone, Copy)]
pub(crate) enum Choice {
    /// Copies to this playlist, or to a new one with None.
    Copy(Option<PlaylistId>),
    /// Copies to a Spotify playlist, asking which first.
    CopyToSpotify,
    Move(Option<PlaylistId>),
    ToggleMarks,
    /// Loves the songs on Last.fm, or unloves them with false.
    Love(bool),
    /// Copies the songs' Spotify links.
    Share,
    Remove,
}

/// What the right-click menu can do to the selected rows.
pub(crate) struct Menu {
    /// Why Move to can't, when it can't.
    pub unmovable: Option<&'static str>,
    /// For a playlist's own rows (Play next): whether they're all marked.
    pub marked: Option<bool>,
    /// While Last.fm is connected: whether the songs are all loved there.
    pub loved: Option<bool>,
    pub can_remove: bool,
    /// More than one row is selected.
    pub several: bool,
}

impl Menu {
    /// For a playlist's own rows, which can always move or go.
    pub(super) fn own(marked: bool, loved: Option<bool>, several: bool) -> Self {
        Self {
            unmovable: None,
            marked: Some(marked),
            loved,
            can_remove: true,
            several,
        }
    }
}

/// An item of a right-click menu.
pub(super) fn item(ui: &mut Ui, name: &str, enabled: bool) -> egui::Response {
    let label = RichText::new(name).monospace().size(11.0);
    ui.add_enabled(enabled, egui::Button::new(label).frame_when_inactive(false))
}

/// The right-click menu of the selected rows: Copy to, Move to, Play next
/// for a playlist's own rows, Love on Last.fm, Share and Remove.
pub(crate) fn row_menu(response: &egui::Response, view: &View, menu: Menu) -> Option<Choice> {
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
        if let Some(loved) = menu.loved {
            let label = if loved {
                "Unlove on Last.fm"
            } else {
                "Love on Last.fm"
            };
            if item(ui, label, true).clicked() {
                chosen = Some(Choice::Love(!loved));
            }
        }
        let copy = if menu.several {
            "Copy song links"
        } else {
            "Copy song link"
        };
        if share_menu(ui, copy) {
            chosen = Some(Choice::Share);
        }
        if item(ui, "Remove", menu.can_remove).clicked() {
            chosen = Some(Choice::Remove);
        }
    });
    chosen
}

/// Does what a playlist's row menu asked for to the selection.
pub(super) fn send(
    list: &Playlist,
    selection: &mut Selection,
    loved: &Loved,
    choice: Choice,
    actions: &mut Actions,
) {
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
        Choice::Love(on) => love(loved, selected_tracks(list, selection), on, actions),
        Choice::Share => actions.push(Desktop::CopyLinks(
            rows.iter()
                .filter_map(|&id| list.get(id))
                .map(|entry| entry.track.uri.clone())
                .collect(),
        )),
        Choice::Remove => remove(list, selection, rows, actions),
    }
}

/// Every selected row is marked to play next.
pub(super) fn all_marked(list: &Playlist, selection: &Selection) -> bool {
    selection
        .rows
        .iter()
        .all(|&id| list.mark_number(id).is_some())
}

pub(super) fn selected_tracks<'a>(
    list: &'a Playlist,
    selection: &'a Selection,
) -> impl Iterator<Item = &'a TrackRef> {
    selection
        .rows
        .iter()
        .filter_map(|&id| list.get(id))
        .map(|entry| &entry.track)
}

/// For the row menu while Last.fm is connected: whether the songs are all
/// loved there. Only worked out while the menu is `open`.
pub(crate) fn all_loved<'a>(
    view: &View,
    open: bool,
    mut tracks: impl Iterator<Item = &'a TrackRef>,
) -> Option<bool> {
    let connected = matches!(view.lastfm.status, LastFmStatus::Connected { .. });
    connected.then(|| open && tracks.all(|track| view.loved.track(track)))
}

/// Loves the songs on Last.fm, or unloves them: those not so already, each
/// song once.
pub(crate) fn love<'a>(
    loved: &Loved,
    tracks: impl Iterator<Item = &'a TrackRef>,
    love: bool,
    actions: &mut Actions,
) {
    let mut sent = Loved::default();
    for track in tracks {
        let artist = track.artists.first().map_or("", |a| a.name.as_str());
        if loved.track(track) != love && sent.set(artist, &track.title, true) {
            actions.push(Action::SetLoved {
                artist: artist.into(),
                title: track.title.clone(),
                loved: love,
            });
        }
    }
}
