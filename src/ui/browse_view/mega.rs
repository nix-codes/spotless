//! A public MEGA folder, opened from its link: the playlists of its M3Us
//! to choose from, or the one shown, with its songs to add, those its M3U
//! names but the folder lacks shown dim.

use egui::{Align, Align2, Layout, Rect, RichText, Ui, pos2, vec2};

use super::album::song_from;
use super::{Act, Nav, ROW_HEIGHT, TRACK_HEIGHT, body, chevron, mono, note, row, spans};
use crate::action::{Actions, BrowseAction, PlaylistsAction};
use crate::ui::View;
use crate::ui::widgets::{SMALL_HEIGHT, small_action, tip};
use spotless_core::browse::Loadable;
use spotless_core::model::Source;
use spotless_core::model::catalog::{FolderPlaylist, FolderSong, MegaFolder};

pub(super) fn mega_page(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    link: &str,
    actions: &mut Actions,
) {
    let folder = match view.browse.megas.get(link) {
        None | Some(Loadable::Loading) => return note(ui, view, "Reading the MEGA folder…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(folder)) => folder,
    };
    match view.browse.folder_playlist() {
        Some(playlist) => songs(ui, view, nav, playlist, actions),
        None => playlists(ui, view, nav, link, folder, actions),
    }
}

/// The page's title, with what it is beside it.
fn title(ui: &mut Ui, view: &View, name: &str, about: &str) {
    let palette = view.palette;
    ui.horizontal(|ui| {
        ui.label(RichText::new(name).size(22.0).color(palette.current));
        ui.label(
            RichText::new(about)
                .monospace()
                .size(10.0)
                .color(palette.dim),
        );
    });
    ui.add_space(8.0);
}

fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}

/// A folder with several M3Us: one row for each, opening its songs.
fn playlists(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    link: &str,
    folder: &MegaFolder,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let about = format!(
        "MEGA FOLDER · {}",
        count(folder.playlists.len(), "playlist", "playlists")
    );
    title(ui, view, &folder.name, &about);
    for (at, playlist) in folder.playlists.iter().enumerate() {
        let songs = count(playlist.songs.len(), "song", "songs");
        let row = row(ui, &palette, nav, ROW_HEIGHT, None, |painter, rect| {
            chevron(
                painter,
                pos2(rect.left() + 5.0, rect.center().y),
                false,
                palette.dim,
            );
            let right = painter.text(
                rect.right_center() - vec2(4.0, 0.0),
                Align2::RIGHT_CENTER,
                &songs,
                mono(),
                palette.dim,
            );
            let text = Rect::from_min_max(
                pos2(rect.left() + 16.0, rect.top()),
                pos2(right.left() - 8.0, rect.bottom()),
            );
            spans(
                painter,
                text,
                &[(playlist.name.as_str(), body(), palette.list_fg)],
            );
        });
        if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
            actions.push(BrowseAction::OpenFolderPlaylist(link.to_string(), at));
        }
    }
}

/// A folder's playlist: + ADD ALL, + NEW PLAYLIST and its songs.
fn songs(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    playlist: &FolderPlaylist,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let browse = view.browse;
    let missing = playlist.songs.iter().filter(|song| !song.found).count();
    let mut about = format!(
        "MEGA FOLDER · {}",
        count(playlist.songs.len(), "song", "songs")
    );
    if missing > 0 {
        about.push_str(&format!(" · {missing} not in the folder"));
    }
    title(ui, view, &playlist.name, &about);

    let find = browse.finder();
    let shown: Vec<(usize, &FolderSong)> = (playlist.songs.iter().enumerate())
        .filter(|(_, song)| find.as_ref().is_none_or(|find| find.track(&song.track)))
        .collect();
    let from = Source {
        uri: playlist.uri.clone(),
        name: playlist.name.clone(),
    };
    let addable: Vec<_> = (shown.iter())
        .filter(|(_, song)| song.found)
        .map(|(_, song)| {
            let mut track = song.track.clone();
            track.source = Some(from.clone());
            track
        })
        .collect();
    let to = view
        .list(browse.target)
        .map_or(String::new(), |list| format!(" to “{}”", list.name));
    let what = match addable.len() {
        1 => "the song shown".to_string(),
        n => format!("the {n} songs shown"),
    };
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), SMALL_HEIGHT),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.add_space(3.0);
            let any = !addable.is_empty();
            let button = small_action(ui, &palette, "+ ADD ALL", any);
            if tip(button, &format!("Adds {what}{to}")).clicked() {
                actions.push(BrowseAction::AddTracks(addable.clone()));
            }
            let button = small_action(ui, &palette, "+ NEW PLAYLIST", any);
            let made = format!("Makes a playlist “{}” of {what}", playlist.name);
            if tip(button, &made).clicked() {
                actions.push(PlaylistsAction::NewNamedPlaylist(
                    playlist.name.clone(),
                    addable.clone(),
                ));
            }
        },
    );
    ui.add_space(4.0);
    if shown.is_empty() {
        nav.found_none = true;
        let typed = browse.find.as_deref().unwrap_or_default().trim();
        return note(ui, view, &format!("No song here has “{typed}”."));
    }
    for (i, song) in shown {
        if song.found {
            song_from(
                ui,
                view,
                nav,
                i + 1,
                &song.track,
                true,
                None,
                Some(from.clone()),
                actions,
            );
        } else {
            missing_song(ui, view, nav, i + 1, song);
        }
    }
}

/// A song the M3U names that isn't in the folder: dim, with nothing to
/// add or play.
fn missing_song(ui: &mut Ui, view: &View, nav: &mut Nav, number: usize, song: &FolderSong) {
    let palette = view.palette;
    let track = &song.track;
    let number = format!("{number:>2}");
    let seconds = track.duration_ms / 1000;
    let duration = format!("{}:{:02}", seconds / 60, seconds % 60);
    let artists = (track.artists.iter().map(|a| a.name.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let row = row(ui, &palette, nav, TRACK_HEIGHT, None, |painter, rect| {
        let right = painter.text(
            rect.right_center() - vec2(4.0, 0.0),
            Align2::RIGHT_CENTER,
            &duration,
            mono(),
            palette.dim,
        );
        let label = painter.text(
            pos2(right.left() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "not in the folder",
            mono(),
            palette.dim,
        );
        let text = Rect::from_min_max(rect.min, pos2(label.left() - 8.0, rect.bottom()));
        let mut parts = vec![(number.as_str(), mono(), palette.dim)];
        if !artists.is_empty() {
            parts.push((artists.as_str(), body(), palette.dim));
            parts.push(("-", body(), palette.dim));
        }
        parts.push((track.title.as_str(), body(), palette.dim));
        spans(painter, text, &parts);
    });
    tip(
        row.response,
        "The folder's M3U names this song, but its file isn't in the folder",
    );
}
