//! Someone's Spotify playlist, opened from a Spotify Code: its name, whose
//! it is, ADD ALL and its songs.

use egui::{Align, Layout, RichText, Ui};

use super::album::song_from;
use super::code::opened_code;
use super::{Nav, note};
use crate::action::{Actions, BrowseAction};
use crate::ui::View;
use crate::ui::widgets::{SMALL_HEIGHT, small_action, tip};
use spotless_core::browse::Loadable;
use spotless_core::model::Source;

pub(super) fn playlist_page(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    uri: &str,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let browse = view.browse;
    let Some(playlist) = browse.any_playlist(uri) else {
        return note(ui, view, "This playlist isn't open any more.");
    };
    let mut about = vec!["PLAYLIST".to_string()];
    if let Some(owner) = &playlist.owner {
        about.push(format!("by {owner}"));
    }
    about.push(match playlist.tracks {
        1 => "1 song".into(),
        n => format!("{n} songs"),
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&playlist.name)
                .size(22.0)
                .color(palette.current),
        );
        ui.label(
            RichText::new(about.join(" · "))
                .monospace()
                .size(10.0)
                .color(palette.dim),
        );
    });
    ui.add_space(8.0);
    if browse.code_here().is_some() {
        opened_code(ui, view, None, actions);
        ui.add_space(10.0);
    }

    let tracks = match browse.albums.get(uri) {
        None | Some(Loadable::Loading) => return note(ui, view, "Reading the songs…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(tracks)) => tracks,
    };
    let find = browse.finder();
    let shown: Vec<_> = (tracks.iter().enumerate())
        .filter(|(_, track)| find.as_ref().is_none_or(|find| find.track(track)))
        .collect();
    let from = Source {
        uri: uri.to_string(),
        name: playlist.name.clone(),
    };
    let to = view
        .list(browse.target)
        .map_or(String::new(), |list| format!(" to “{}”", list.name));
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), SMALL_HEIGHT),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.add_space(3.0);
            let what = match shown.len() {
                1 => "the song shown".to_string(),
                n => format!("the {n} songs shown"),
            };
            let button = small_action(ui, &palette, "+ ADD ALL", !shown.is_empty());
            if tip(button, &format!("Adds {what}{to}")).clicked() {
                actions.push(if find.is_some() {
                    let mut songs: Vec<_> = shown.iter().map(|(_, t)| (*t).clone()).collect();
                    for song in &mut songs {
                        song.source = Some(from.clone());
                    }
                    BrowseAction::AddTracks(songs)
                } else {
                    BrowseAction::AddAlbum(uri.to_string())
                });
            }
        },
    );
    ui.add_space(4.0);
    if shown.is_empty() {
        nav.found_none = true;
        let typed = browse.find.as_deref().unwrap_or_default().trim();
        return note(ui, view, &format!("No song here has “{typed}”."));
    }
    for (i, track) in shown {
        song_from(
            ui,
            view,
            nav,
            i + 1,
            track,
            true,
            None,
            Some(from.clone()),
            actions,
        );
    }
}
