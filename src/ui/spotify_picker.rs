//! Choosing the Spotify playlist songs are copied to: one of the account's
//! own, found by typing part of its name, or a new one, whose name is
//! asked next (the songs' tab's to start with). The arrows move the
//! highlight, Enter copies, Esc cancels.

use egui::text::{CCursor, CCursorRange};
use egui::{Align2, Id, Key, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, pos2, vec2};

use super::widgets::{field, small_button};
use super::{Inputs, View};
use crate::action::{Action, Actions};
use crate::browse::Loadable;
use crate::model::TrackRef;
use crate::model::catalog::{SpotifyPlaylist, SpotifyTarget};
use crate::spotify::catalog::LIKED_SONGS;

const WIDTH: f32 = 340.0;
const ROW_HEIGHT: f32 = 20.0;
const LIST_HEIGHT: f32 = 240.0;

/// The picker while it is open.
pub struct Picker {
    pub tracks: Vec<TrackRef>,
    /// A new playlist's name when nothing is typed: the songs' tab's.
    pub name: String,
    pub query: String,
    /// The highlighted choice; 0 is the new playlist.
    pub row: usize,
    /// The highlight moved by key: scroll to it.
    reveal: bool,
    /// The field has had the keyboard once.
    focused: bool,
    /// The new playlist's name, while it is asked for.
    naming: Option<String>,
}

impl Picker {
    pub fn new(tracks: Vec<TrackRef>, name: String) -> Self {
        Self {
            tracks,
            name,
            query: String::new(),
            row: 0,
            reveal: false,
            focused: false,
            naming: None,
        }
    }

    /// Asks for a new playlist's name at once, `name` to start with.
    pub fn naming(tracks: Vec<TrackRef>, name: String) -> Self {
        Self {
            naming: Some(name.clone()),
            ..Self::new(tracks, name)
        }
    }
}

pub fn draw(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some(picker) = inputs.spotify_pick.as_mut() else {
        return;
    };
    if picker.naming.is_some() {
        return name(ui, view, inputs, actions);
    }
    let palette = view.palette;
    let query = picker.query.trim().to_lowercase();
    let own: Vec<&SpotifyPlaylist> = match &view.browse.spotify_playlists {
        Some(Loadable::Ready(playlists)) => playlists
            .iter()
            .filter(|p| p.owner.is_none() && p.uri != LIKED_SONGS)
            .filter(|p| p.name.to_lowercase().contains(&query))
            .collect(),
        _ => Vec::new(),
    };
    let choices = 1 + own.len();
    let (up, down, enter, escape) = ui.input(|i| {
        (
            i.key_pressed(Key::ArrowUp),
            i.key_pressed(Key::ArrowDown),
            i.key_pressed(Key::Enter),
            i.key_pressed(Key::Escape),
        )
    });
    if up || down {
        picker.row = if up {
            picker.row.saturating_sub(1)
        } else {
            picker.row + 1
        };
        picker.reveal = true;
    }
    picker.row = picker.row.min(choices - 1);
    // None: the new playlist, named next.
    let target = |row: usize| match row {
        0 => None,
        n => Some(SpotifyTarget::Playlist {
            uri: own[n - 1].uri.clone(),
            name: own[n - 1].name.clone(),
        }),
    };

    let mut chosen: Option<Option<SpotifyTarget>> = None;
    let mut cancel = escape;
    let modal = egui::Modal::new(Id::new("spotify-picker"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            let songs = match picker.tracks.len() {
                1 => "1 song".to_string(),
                n => format!("{n} songs"),
            };
            title(ui, view, &format!("Copy {songs} to Spotify"));
            ui.add_space(6.0);
            let edit = field(
                ui,
                TextEdit::singleline(&mut picker.query)
                    .id(Id::new("spotify-picker-field"))
                    .hint_text("Find a playlist")
                    .desired_width(WIDTH),
            );
            if !picker.focused {
                edit.request_focus();
                picker.focused = true;
            }
            if edit.changed() {
                picker.row = 0;
            }
            ui.add_space(6.0);
            ScrollArea::vertical()
                .max_height(LIST_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in 0..choices {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(WIDTH, ROW_HEIGHT), Sense::click());
                        let painter = ui.painter();
                        if row == picker.row {
                            painter.rect_filled(rect, 0.0, palette.selected_bg);
                            if picker.reveal {
                                ui.scroll_to_rect(rect, None);
                            }
                        } else if response.hovered() {
                            painter.rect_filled(rect, 0.0, palette.hi.gamma_multiply(0.35));
                        }
                        let left = pos2(rect.left() + 4.0, rect.center().y);
                        let body = egui::FontId::proportional(12.5);
                        let mono = egui::FontId::monospace(11.0);
                        if row == 0 {
                            painter.text(
                                left,
                                Align2::LEFT_CENTER,
                                "+ New playlist…",
                                body,
                                palette.accent,
                            );
                        } else {
                            let playlist = own[row - 1];
                            let count = painter.text(
                                rect.right_center() - vec2(4.0, 0.0),
                                Align2::RIGHT_CENTER,
                                playlist.tracks.to_string(),
                                mono,
                                palette.dim,
                            );
                            let clip = egui::Rect::from_min_max(
                                rect.min,
                                pos2(count.left() - 8.0, rect.bottom()),
                            );
                            painter.with_clip_rect(clip).text(
                                left,
                                Align2::LEFT_CENTER,
                                &playlist.name,
                                body,
                                palette.list_fg,
                            );
                        }
                        if response.clicked() {
                            picker.row = row;
                        }
                        if response.double_clicked() {
                            chosen = Some(target(row));
                        }
                    }
                    picker.reveal = false;
                });
            let note = match &view.browse.spotify_playlists {
                Some(Loadable::Loading) | None => Some("Reading your playlists…".to_string()),
                Some(Loadable::Failed(error)) => Some(error.clone()),
                Some(Loadable::Ready(_)) => None,
            };
            if let Some(note) = note {
                ui.add_space(4.0);
                ui.label(RichText::new(note).color(palette.dim));
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if small_button(ui, &palette, "COPY", false)
                    .on_hover_text("Enter")
                    .clicked()
                {
                    chosen = Some(target(picker.row));
                }
                if small_button(ui, &palette, "CANCEL", false)
                    .on_hover_text("Esc")
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
    if enter && chosen.is_none() {
        chosen = Some(target(picker.row));
    }
    if modal.backdrop_response.clicked() {
        cancel = true;
    }
    match chosen {
        Some(Some(target)) => {
            actions.push(Action::CopyToSpotify(target, picker.tracks.clone()));
            cancel = true;
        }
        Some(None) => {
            picker.naming = Some(picker.name.clone());
            picker.focused = false;
        }
        None => {}
    }
    if cancel {
        inputs.spotify_pick = None;
    }
}

/// Asks for the new playlist's name, the tab's selected to type over.
fn name(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some(picker) = inputs.spotify_pick.as_mut() else {
        return;
    };
    let palette = view.palette;
    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    let mut save = enter;
    let mut cancel = escape;
    let modal = egui::Modal::new(Id::new("spotify-picker"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.set_width(WIDTH);
            let songs = match picker.tracks.len() {
                1 => "1 song".to_string(),
                n => format!("{n} songs"),
            };
            title(ui, view, &format!("Copy {songs} to a new Spotify playlist"));
            ui.add_space(6.0);
            let id = Id::new("spotify-picker-name");
            let name = picker.naming.get_or_insert_default();
            let edit = field(
                ui,
                TextEdit::singleline(name)
                    .id(id)
                    .hint_text("Its name")
                    .desired_width(WIDTH),
            );
            if !picker.focused {
                edit.request_focus();
                if let Some(mut state) = TextEdit::load_state(ui.ctx(), id) {
                    let end = CCursor::new(name.chars().count());
                    state
                        .cursor
                        .set_char_range(Some(CCursorRange::two(CCursor::new(0), end)));
                    state.store(ui.ctx(), id);
                }
                picker.focused = true;
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if small_button(ui, &palette, "SAVE", false)
                    .on_hover_text("Enter")
                    .clicked()
                {
                    save = true;
                }
                if small_button(ui, &palette, "CANCEL", false)
                    .on_hover_text("Esc")
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
    if modal.backdrop_response.clicked() {
        cancel = true;
    }
    let name = picker.naming.as_deref().unwrap_or_default().trim();
    if save && !name.is_empty() {
        let target = SpotifyTarget::New(name.to_string());
        actions.push(Action::CopyToSpotify(target, picker.tracks.clone()));
        cancel = true;
    }
    if cancel {
        inputs.spotify_pick = None;
    }
}

fn title(ui: &mut Ui, view: &View, text: &str) {
    ui.label(RichText::new(text).size(15.0).color(view.palette.current));
}
