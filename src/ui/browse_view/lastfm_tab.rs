//! Browse's LAST.FM tab, while Last.fm and search are connected: the
//! songs loved on Last.fm, newest first, with when. They are selected as
//! in a tab (click, Ctrl, Shift, Ctrl+A) and added, dragged to the slim
//! column or played; Spotless finds them on Spotify first, by artist and
//! title, and says which it couldn't.

use std::collections::BTreeSet;

use egui::{Align2, Id, Key, Rect, RichText, ScrollArea, Ui, pos2, vec2};

use super::spotify::extend;
use super::{Act, Area, CatalogDrag, Nav, TRACK_HEIGHT, body, mono, note, row, spans};
use crate::action::{Action, Actions};
use crate::lastfm::{LastFmStatus, LovedSong};
use crate::ui::scrollbar;
use crate::ui::widgets::small_button;
use crate::ui::{View, theme};

pub(super) fn draw(ui: &mut Ui, view: &View, nav: &mut Nav, actions: &mut Actions) {
    let palette = view.palette;
    let full = ui.available_rect_before_wrap();
    if nav.keyboard && nav.area == Area::Results {
        theme::focus_ring(ui.painter(), full.expand(4.0), &palette);
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("Loved on Last.fm")
                .size(18.0)
                .color(palette.current),
        );
        if let LastFmStatus::Connected { user } = &view.lastfm.status {
            ui.label(
                RichText::new(format!("AS {}", user.to_uppercase()))
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        }
    });
    ui.add_space(6.0);
    let songs = view.loved.songs();
    nav.marked.retain(|&i| i < songs.len());
    // Arrows move the selection with the highlight.
    if nav.reveal
        && nav.keyboard
        && let Some(row) = nav.row
    {
        nav.marked = BTreeSet::from([row]);
        nav.anchor = Some(row);
    }
    let keys = nav.area == Area::Results && !ui.ctx().text_edit_focused();
    if keys && ui.input(|i| i.modifiers.command && i.key_pressed(Key::A)) {
        nav.marked = (0..songs.len()).collect();
    }
    if keys {
        extend(ui, nav, songs.len());
    }
    // The selection, else the highlighted song.
    let chosen = match (nav.marked.is_empty(), nav.row) {
        (false, _) => nav.marked.iter().filter_map(|&i| songs.get(i)).collect(),
        (true, Some(row)) => songs.get(row).into_iter().collect(),
        _ => Vec::new(),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        let add = ui.add_enabled_ui(!chosen.is_empty(), |ui| {
            small_button(ui, &palette, "+ ADD", false)
        });
        let hint = "Finds the selected songs on Spotify and adds them (Insert or A)";
        if add.inner.on_hover_text(hint).clicked() {
            actions.push(Action::AddLoved(chosen.into_iter().cloned().collect()));
        }
        ui.add_space(8.0);
        let count = match (songs.len(), nav.marked.len()) {
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
    if songs.is_empty() {
        return note(
            ui,
            view,
            "No loved songs yet: love some with the heart by the song playing.",
        );
    }
    nav.drawn = 0;
    let id = Id::new("lastfm-loved-scrollbar");
    let area = ScrollArea::vertical().id_salt("lastfm-loved");
    scrollbar::show(ui, view, id, area, |area, ui| {
        area.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
            for (i, song) in songs.iter().enumerate() {
                loved_song(ui, view, nav, i, song, songs, actions);
            }
        })
    });
    // Keys for rows that aren't there are dropped.
    nav.rows = nav.drawn;
    nav.row = nav.row.map(|row| row.min(nav.rows.saturating_sub(1)));
    nav.act = None;
    nav.reveal = false;
}

/// A song loved, with the day it was.
fn loved_song(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    index: usize,
    song: &LovedSong,
    songs: &[LovedSong],
    actions: &mut Actions,
) {
    let palette = view.palette;
    let marked = nav.marked.contains(&index);
    let number = format!("{:>4}", index + 1);
    let date = day(song.date);
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
                &date,
                mono(),
                palette.dim,
            );
            let text = Rect::from_min_max(rect.min, pos2(right.left() - 8.0, rect.bottom()));
            spans(
                painter,
                text,
                &[
                    (number.as_str(), mono(), palette.dim),
                    (song.artist.as_str(), body(), palette.list_fg),
                    ("-", body(), palette.dim),
                    (song.title.as_str(), body(), palette.list_fg),
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
    let these = |nav: &Nav| -> Vec<LovedSong> {
        if nav.marked.contains(&index) {
            nav.marked
                .iter()
                .filter_map(|&i| songs.get(i))
                .cloned()
                .collect()
        } else {
            vec![song.clone()]
        }
    };
    if response.drag_started() {
        response.dnd_set_drag_payload(CatalogDrag::Loved(these(nav)));
    }
    if row.button {
        actions.push(Action::AddLoved(vec![song.clone()]));
    } else if matches!(row.act, Some(Act::Add)) {
        actions.push(Action::AddLoved(these(nav)));
    } else if response.double_clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::PlayLoved(song.clone()));
    }
    let target = view
        .library
        .get(view.browse.target)
        .map_or("the playlist", |list| list.name.as_str());
    response.context_menu(|ui| {
        let item = |ui: &mut Ui, name: &str| {
            let label = RichText::new(name).monospace().size(11.0);
            ui.add(egui::Button::new(label).frame_when_inactive(false))
        };
        if item(ui, &format!("Add to {target}")).clicked() {
            actions.push(Action::AddLoved(these(nav)));
        }
        if item(ui, "Play now").clicked() {
            actions.push(Action::PlayLoved(song.clone()));
        }
        if item(ui, "Unlove on Last.fm").clicked() {
            for song in these(nav) {
                actions.push(Action::SetLoved {
                    artist: song.artist,
                    title: song.title,
                    loved: false,
                });
            }
            nav.marked.clear();
            nav.anchor = None;
        }
    });
}

/// A Unix time's day, as "29 Oct 2025" (UTC); blank when not known.
fn day(unix: u64) -> String {
    if unix == 0 {
        return String::new();
    }
    // Howard Hinnant's days-to-civil, from 1970-01-01.
    let days = (unix / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!("{day} {} {year}", MONTHS[(month - 1) as usize])
}

#[cfg(test)]
mod tests {
    use super::day;

    #[test]
    fn days_read_as_the_calendar_has_them() {
        assert_eq!(day(0), "");
        assert_eq!(day(86_399), "1 Jan 1970");
        assert_eq!(day(951_782_400), "29 Feb 2000");
        assert_eq!(day(1_761_700_000), "29 Oct 2025");
        assert_eq!(day(1_798_761_599), "31 Dec 2026");
    }
}
