//! Browse's YOUR SPOTIFY tab: the account's Spotify playlists on the
//! left, Liked Songs and its own first, then those it follows; the songs
//! of the one chosen beside them. Arrows in the list (or its field, which
//! finds a playlist by name) show the next one at once, Enter goes to its
//! songs. The songs are selected as in a tab (click, Ctrl, Shift, Ctrl+A)
//! and added; on the account's own playlists and Liked Songs, Delete or
//! REM takes them out on Spotify, and UNDO or Ctrl+Z puts them back. A playlist
//! itself is deleted, or unfollowed if it's someone else's, from its
//! menu, the button beside its name or Shift+Delete in the list; UNDO
//! puts it back.

use std::collections::BTreeSet;

use egui::{
    Align2, Event, Id, Key, Modifiers, Rect, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui,
    UiBuilder, pos2, vec2,
};

use super::{Act, Area, CatalogDrag, Nav, PAD, TRACK_HEIGHT, body, mono, note, row, spans};
use crate::action::{Action, Actions};
use crate::browse::Loadable;
use crate::model::TrackRef;
use crate::model::catalog::SpotifyPlaylist;
use crate::ui::scrollbar;
use crate::ui::theme;
use crate::ui::widgets::{field, small_button, tip};
use crate::ui::{Inputs, View};

pub(super) const FILTER_ID: &str = "spotify-filter";
/// The list of playlists at its widest.
const LIST_WIDTH: f32 = 210.0;
const LIST_ROW: f32 = 20.0;

pub(super) fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let full = ui.available_rect_before_wrap();
    let playlists = match &view.browse.spotify_playlists {
        Some(Loadable::Ready(playlists)) => playlists.as_slice(),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        None | Some(Loadable::Loading) => return note(ui, view, "Reading your playlists…"),
    };
    // The playlist left open, else the first; its songs read if they
    // aren't (after a restart).
    let open = view
        .browse
        .spotify_open
        .as_deref()
        .filter(|uri| playlists.iter().any(|p| p.uri == *uri));
    match open {
        None => {
            if let Some(first) = playlists.first() {
                actions.push(Action::OpenSpotifyPlaylist(first.uri.clone()));
            }
        }
        Some(uri) if !view.browse.albums.contains_key(uri) => {
            actions.push(Action::OpenSpotifyPlaylist(uri.to_string()));
        }
        Some(_) => {}
    }
    let width = (full.width() * 0.34).clamp(140.0, LIST_WIDTH);
    let list = Rect::from_min_max(full.min, pos2(full.left() + width, full.bottom()));
    let songs = Rect::from_min_max(pos2(list.right() + PAD, full.top()), full.max);
    let x = list.right() + PAD / 2.0;
    ui.painter().line_segment(
        [pos2(x, full.top()), pos2(x, full.bottom())],
        Stroke::new(1.0, palette.lo),
    );
    let nav = &mut inputs.nav;
    if nav.keyboard {
        match nav.area {
            Area::Lists => theme::focus_ring(ui.painter(), list.expand(4.0), &palette),
            Area::Results => theme::focus_ring(ui.painter(), songs.expand(4.0), &palette),
            _ => {}
        }
    }
    ui.scope_builder(UiBuilder::new().max_rect(list), |ui| {
        playlist_list(ui, view, nav, playlists, open, actions);
    });
    ui.scope_builder(UiBuilder::new().max_rect(songs), |ui| match open {
        Some(uri) => songs_pane(ui, view, nav, uri, actions),
        None => note(ui, view, "No playlists."),
    });
}

/// The field that finds a playlist, and the list, mine then followed.
fn playlist_list(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    playlists: &[SpotifyPlaylist],
    open: Option<&str>,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let width = ui.available_width();
    // Shift+Delete in the list or its field takes the playlist shown out,
    // taken before the field would delete text with it. On Windows it
    // arrives as Cut (Ctrl+X too, but with Ctrl held).
    let in_field = ui.memory(|m| m.has_focus(Id::new(FILTER_ID)));
    let unlist = nav.area == Area::Lists
        && (in_field || !ui.ctx().text_edit_focused())
        && ui.input_mut(|i| {
            let key = i.consume_key(Modifiers::SHIFT, Key::Delete);
            let cuts = i.events.len();
            if i.modifiers.matches_exact(Modifiers::SHIFT) {
                i.events.retain(|e| !matches!(e, Event::Cut));
            }
            key || i.events.len() < cuts
        });
    let edit = field(
        ui,
        TextEdit::singleline(&mut nav.filter)
            .id(Id::new(FILTER_ID))
            .hint_text("Find a playlist")
            .desired_width(width),
    );
    if std::mem::take(&mut nav.focus_filter) {
        edit.request_focus();
    }
    let finding = nav.filter.trim().to_lowercase();
    let shown: Vec<&SpotifyPlaylist> = playlists
        .iter()
        .filter(|p| p.name.to_lowercase().contains(&finding))
        .collect();
    // Mine first, as the list draws them.
    let order: Vec<&SpotifyPlaylist> = shown
        .iter()
        .filter(|p| p.owner.is_none())
        .chain(shown.iter().filter(|p| p.owner.is_some()))
        .copied()
        .collect();
    // Enter leaves the field (it lets go of the keyboard itself) for the
    // songs, the first highlighted.
    if edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
        nav.area = Area::Results;
        nav.keyboard = true;
        nav.row = Some(0);
        nav.reveal = true;
    }
    if unlist
        && let Some(uri) = open.filter(|uri| order.iter().any(|p| p.uri == *uri))
        && view.browse.unlistable(uri)
    {
        actions.push(Action::UnlistSpotifyPlaylist(uri.to_string()));
    }
    // The arrows and PgUp / PgDn in the field, or in the list after a
    // click took the keyboard from the field; Home / End too out of it.
    let typing = edit.has_focus();
    // The playlist moved to, scrolled into view now: it's shown from the
    // next frame.
    let mut reveal = None;
    if typing {
        nav.area = Area::Lists;
    }
    if typing || (nav.area == Area::Lists && !ui.ctx().text_edit_focused()) {
        let at = order.iter().position(|p| Some(p.uri.as_str()) == open);
        let last = order.len().saturating_sub(1);
        let next = ui.input(|i| {
            if !i.modifiers.is_none() {
                return None;
            }
            let from = at.unwrap_or(0);
            if i.key_pressed(Key::ArrowDown) {
                Some(at.map_or(0, |at| at + 1))
            } else if i.key_pressed(Key::ArrowUp) {
                Some(from.saturating_sub(1))
            } else if i.key_pressed(Key::PageDown) {
                Some(from + super::PAGE_ROWS)
            } else if i.key_pressed(Key::PageUp) {
                Some(from.saturating_sub(super::PAGE_ROWS))
            } else if !typing && i.key_pressed(Key::Home) {
                Some(0)
            } else if !typing && i.key_pressed(Key::End) {
                Some(last)
            } else {
                None
            }
        });
        if let Some(playlist) = next.and_then(|n| order.get(n.min(last))) {
            actions.push(Action::OpenSpotifyPlaylist(playlist.uri.clone()));
            nav.keyboard = true;
            reveal = Some(playlist.uri.as_str());
        }
    }
    ui.add_space(6.0);
    let id = Id::new("spotify-list-scrollbar");
    let area = ScrollArea::vertical().id_salt("spotify-list");
    scrollbar::show(ui, view, id, area, |area, ui| {
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
            let mine = order.iter().take_while(|p| p.owner.is_none());
            let followed = order.iter().skip_while(|p| p.owner.is_none());
            for (title, group) in [
                ("YOURS", mine.collect::<Vec<_>>()),
                ("FOLLOWED", followed.collect::<Vec<_>>()),
            ] {
                if group.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                let heading = ui
                    .label(
                        RichText::new(title)
                            .monospace()
                            .size(10.0)
                            .color(palette.chrome),
                    )
                    .rect;
                ui.add_space(2.0);
                for (at, playlist) in group.into_iter().enumerate() {
                    let chosen = open == Some(playlist.uri.as_str());
                    let (rect, response) = ui
                        .allocate_exact_size(vec2(ui.available_width(), LIST_ROW), Sense::click());
                    let painter = ui.painter();
                    // The first of a group with its heading.
                    if reveal == Some(playlist.uri.as_str()) {
                        let shown = if at == 0 { rect.union(heading) } else { rect };
                        ui.scroll_to_rect(shown, None);
                    }
                    if chosen {
                        painter.rect_filled(rect, 0.0, palette.selected_bg);
                    } else if response.hovered() {
                        painter.rect_filled(rect, 0.0, palette.hi.gamma_multiply(0.35));
                    }
                    let count = painter.text(
                        rect.right_center() - vec2(4.0, 0.0),
                        Align2::RIGHT_CENTER,
                        playlist.tracks.to_string(),
                        mono(),
                        palette.dim,
                    );
                    let clip =
                        Rect::from_min_max(rect.min, pos2(count.left() - 6.0, rect.bottom()));
                    let color = if chosen {
                        palette.current
                    } else {
                        palette.list_fg
                    };
                    painter.with_clip_rect(clip).text(
                        pos2(rect.left() + 4.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &playlist.name,
                        body(),
                        color,
                    );
                    if response.clicked() {
                        actions.push(Action::OpenSpotifyPlaylist(playlist.uri.clone()));
                        nav.area = Area::Lists;
                    }
                    if view.browse.unlistable(&playlist.uri) {
                        response.context_menu(|ui| {
                            let label =
                                RichText::new(unlist_label(playlist)).monospace().size(11.0);
                            let button = egui::Button::new(label).frame_when_inactive(false);
                            if ui.add(button).clicked() {
                                actions.push(Action::UnlistSpotifyPlaylist(playlist.uri.clone()));
                            }
                        });
                    }
                }
            }
            if order.is_empty() {
                note(ui, view, "No playlist by that name.");
            }
        })
    });
}

/// What taking a playlist out of the account's list does to it.
fn unlist_label(playlist: &SpotifyPlaylist) -> &'static str {
    match playlist.owner {
        None => "Delete playlist",
        Some(_) => "Unfollow playlist",
    }
}

/// The playlist chosen: its name, whose it is, DELETE or UNFOLLOW, + ADD
/// ALL and REM, and its songs.
fn songs_pane(ui: &mut Ui, view: &View, nav: &mut Nav, uri: &str, actions: &mut Actions) {
    let palette = view.palette;
    let browse = view.browse;
    let playlist = browse.spotify_playlist(uri);
    let editable = browse.editable(uri);
    ui.horizontal(|ui| {
        let name = playlist.map_or("Spotify playlist", |p| p.name.as_str());
        ui.label(RichText::new(name).size(18.0).color(palette.current));
        let tag = match playlist.and_then(|p| p.owner.as_ref()) {
            Some(owner) => format!("BY {} · READ ONLY", owner.to_uppercase()),
            None => "YOURS".into(),
        };
        ui.label(RichText::new(tag).monospace().size(10.0).color(palette.dim));
        if let Some(playlist) = playlist.filter(|p| browse.unlistable(&p.uri)) {
            let (label, hint) = match playlist.owner {
                None => ("DELETE", "Deletes this playlist on Spotify"),
                Some(_) => (
                    "UNFOLLOW",
                    "Takes this playlist out of your Spotify playlists",
                ),
            };
            ui.add_space(4.0);
            if small_button(ui, &palette, label, false)
                .on_hover_text(hint)
                .clicked()
            {
                actions.push(Action::UnlistSpotifyPlaylist(playlist.uri.clone()));
            }
        }
    });
    ui.add_space(6.0);
    let tracks = match browse.albums.get(uri) {
        None | Some(Loadable::Loading) => return note(ui, view, "Reading the songs…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(tracks)) => tracks,
    };
    nav.marked.retain(|&i| i < tracks.len());
    // Arrows move the selection with the highlight.
    if nav.reveal
        && nav.keyboard
        && let Some(row) = nav.row
    {
        nav.marked = BTreeSet::from([row]);
        nav.anchor = Some(row);
    }
    let keys = nav.area == Area::Results && !ui.ctx().text_edit_focused();
    let (delete, all) = ui.input(|i| {
        (
            keys && i.key_pressed(Key::Delete),
            keys && i.modifiers.command && i.key_pressed(Key::A),
        )
    });
    if all {
        nav.marked = (0..tracks.len()).collect();
    }
    if keys {
        extend(ui, nav, tracks.len());
    }
    // The selection, else the highlighted song.
    let chosen = |nav: &Nav| -> Vec<usize> {
        match (nav.marked.is_empty(), nav.row) {
            (false, _) => nav.marked.iter().copied().collect(),
            (true, Some(row)) if row < tracks.len() => vec![row],
            _ => Vec::new(),
        }
    };
    let mut remove = delete && editable;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        if small_button(ui, &palette, "+ ADD ALL", false).clicked() {
            actions.push(Action::AddAlbum(uri.to_string()));
        }
        if editable {
            let rem = ui.add_enabled_ui(!chosen(nav).is_empty(), |ui| {
                small_button(ui, &palette, "REM", false)
            });
            let hint = "Removes the selected songs from this playlist on Spotify (Delete)";
            if tip(rem.inner, hint).clicked() {
                remove = true;
            }
        }
        ui.add_space(8.0);
        let count = match (tracks.len(), nav.marked.len()) {
            (1, 0) => "1 song".to_string(),
            (n, 0) => format!("{n} songs"),
            (n, marked) => format!("{marked} of {n} selected"),
        };
        ui.label(
            RichText::new(count)
                .monospace()
                .size(11.0)
                .color(palette.dim),
        );
    });
    ui.add_space(6.0);
    if remove {
        let rows = chosen(nav);
        if !rows.is_empty() {
            actions.push(Action::RemoveFromSpotify(uri.to_string(), rows.clone()));
            // The highlight stays where the first song was.
            nav.row = rows
                .first()
                .map(|&r| r.min(tracks.len().saturating_sub(rows.len() + 1)));
            nav.marked.clear();
            nav.anchor = None;
        }
    }
    if tracks.is_empty() {
        return note(ui, view, "No songs.");
    }
    nav.drawn = 0;
    let id = Id::new(("spotify-songs-scrollbar", uri));
    let area = ScrollArea::vertical().id_salt(("spotify-songs", uri));
    scrollbar::show(ui, view, id, area, |area, ui| {
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
            for (i, track) in tracks.iter().enumerate() {
                playlist_song(ui, view, nav, i, track, tracks, editable, uri, actions);
            }
        })
    });
    // Keys for rows that aren't there are dropped.
    nav.rows = nav.drawn;
    nav.row = nav.row.map(|row| row.min(nav.rows.saturating_sub(1)));
    nav.act = None;
    nav.reveal = false;
}

/// Shift with the arrows, PgUp / PgDn or Home / End: the highlight moves
/// and the selection stretches to it from where it started, as in a tab.
fn extend(ui: &Ui, nav: &mut Nav, len: usize) {
    let last = len.saturating_sub(1);
    let from = nav.row.unwrap_or(0);
    let to = ui.input(|i| {
        if !(i.modifiers.shift && !i.modifiers.command && !i.modifiers.alt) {
            return None;
        }
        if i.key_pressed(Key::ArrowDown) {
            Some(from + 1)
        } else if i.key_pressed(Key::ArrowUp) {
            Some(from.saturating_sub(1))
        } else if i.key_pressed(Key::PageDown) {
            Some(from + super::PAGE_ROWS)
        } else if i.key_pressed(Key::PageUp) {
            Some(from.saturating_sub(super::PAGE_ROWS))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(last)
        } else {
            None
        }
    });
    let Some(to) = to.map(|to| to.min(last)) else {
        return;
    };
    let anchor = *nav.anchor.get_or_insert(from);
    nav.marked = (anchor.min(to)..=anchor.max(to)).collect();
    nav.row = Some(to);
    nav.reveal = true;
    nav.keyboard = true;
}

/// A song of the playlist shown.
#[allow(clippy::too_many_arguments)]
fn playlist_song(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    index: usize,
    track: &TrackRef,
    tracks: &[TrackRef],
    editable: bool,
    uri: &str,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let marked = nav.marked.contains(&index);
    let number = format!("{:>3}", index + 1);
    let seconds = track.duration_ms / 1000;
    let duration = format!("{}:{:02}", seconds / 60, seconds % 60);
    let artists = track
        .artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let row = row(
        ui,
        &palette,
        nav,
        TRACK_HEIGHT,
        Some("+"),
        |painter, rect| {
            if marked {
                painter.rect_filled(rect.expand2(vec2(4.0, 0.0)), 0.0, palette.selected_bg);
            }
            let right = painter.text(
                rect.right_center() - vec2(4.0, 0.0),
                Align2::RIGHT_CENTER,
                &duration,
                mono(),
                palette.dim,
            );
            let text = Rect::from_min_max(rect.min, pos2(right.left() - 8.0, rect.bottom()));
            spans(
                painter,
                text,
                &[
                    (number.as_str(), mono(), palette.dim),
                    (artists.as_str(), body(), palette.list_fg),
                    ("-", body(), palette.dim),
                    (track.title.as_str(), body(), palette.list_fg),
                    (track.album.name.as_str(), body(), palette.dim),
                ],
            );
        },
    );
    let response = &row.response;
    if response.clicked() || (response.secondary_clicked() && !marked) {
        let modifiers = ui.input(|i| i.modifiers);
        if response.clicked() && modifiers.command {
            if !nav.marked.remove(&index) {
                nav.marked.insert(index);
            }
        } else if response.clicked() && modifiers.shift {
            let from = nav.anchor.unwrap_or(index);
            nav.marked = (from.min(index)..=from.max(index)).collect();
        } else {
            nav.marked = BTreeSet::from([index]);
        }
        if !(response.clicked() && modifiers.shift) {
            nav.anchor = Some(index);
        }
    }
    // What a drag, a key or the menu takes: the selection, if this song
    // is in it.
    let these = |nav: &Nav| -> Vec<TrackRef> {
        if nav.marked.contains(&index) {
            nav.marked
                .iter()
                .filter_map(|&i| tracks.get(i))
                .cloned()
                .collect()
        } else {
            vec![track.clone()]
        }
    };
    if response.drag_started() {
        response.dnd_set_drag_payload(CatalogDrag::Tracks(these(nav)));
    }
    if row.button {
        actions.push(Action::AddTracks(vec![track.clone()]));
    } else if matches!(row.act, Some(Act::Add)) {
        actions.push(Action::AddTracks(these(nav)));
    } else if response.double_clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::PlayNow(track.clone()));
    }
    let target = view
        .library
        .get(view.browse.target)
        .map_or("the playlist", |list| list.name.as_str());
    response.context_menu(|ui| {
        let item = |ui: &mut Ui, name: &str, enabled: bool| {
            let label = RichText::new(name).monospace().size(11.0);
            ui.add_enabled(enabled, egui::Button::new(label).frame_when_inactive(false))
        };
        if item(ui, &format!("Add to {target}"), true).clicked() {
            actions.push(Action::AddTracks(these(nav)));
        }
        let remove = item(ui, "Remove from this playlist", editable)
            .on_disabled_hover_text("Only your own playlists can be changed");
        if remove.clicked() {
            let rows = if nav.marked.contains(&index) {
                nav.marked.iter().copied().collect()
            } else {
                vec![index]
            };
            actions.push(Action::RemoveFromSpotify(uri.to_string(), rows));
            nav.marked.clear();
            nav.anchor = None;
        }
    });
}
