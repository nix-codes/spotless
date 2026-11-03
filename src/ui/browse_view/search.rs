//! The SEARCH tab: the search field, the page's find, and the results.

use egui::{Align2, Id, Key, TextEdit, Ui, vec2};

use super::album::{album, song};
use super::artist::artist_row;
use super::{
    Act, Area, FIND_ID, Nav, ROW_HEIGHT, SEARCH_ID, TRACK_HEIGHT, body, mono, note, row, section,
    spans,
};
use crate::action::{Actions, BrowseAction};
use crate::ui::widgets::{BUTTON_HEIGHT, field, text_button, text_button_width};
use crate::ui::{Inputs, View};
use spotless_core::browse::{Loadable, Page};
use spotless_core::session::SearchState;

pub(super) fn search_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    ui.horizontal(|ui| {
        let back = text_button(ui, &palette, "BACK", false, view.browse.can_go_back());
        if crate::ui::widgets::tip(back, "Backspace or Alt+←").clicked() {
            actions.push(BrowseAction::Back);
        }
        let enabled = searchable(view);
        let hint = if enabled {
            "Search artists, albums, songs…"
        } else {
            "Search isn't set up yet"
        };
        // FIND at the right end; the field takes what is left.
        let open = view.browse.find.is_some();
        let find_width = text_button_width(ui, "FIND") + ui.spacing().item_spacing.x;
        let width = (ui.available_width() - find_width).max(80.0);
        let field = ui
            .add_enabled_ui(enabled, |ui| {
                field(
                    ui,
                    TextEdit::singleline(&mut inputs.query)
                        .id(Id::new(SEARCH_ID))
                        .hint_text(hint)
                        .desired_width(width)
                        .min_size(vec2(0.0, BUTTON_HEIGHT)),
                )
            })
            .inner;
        if std::mem::take(&mut inputs.focus_search) && enabled {
            field.request_focus();
        }
        if field.has_focus() {
            inputs.nav.area = Area::Search;
            // ↓ goes down to the results, the first highlighted.
            let nav = &mut inputs.nav;
            if nav.rows > 0 && ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::ArrowDown))
            {
                field.surrender_focus();
                nav.area = Area::Results;
                nav.row = Some(0);
                nav.keyboard = true;
                nav.reveal = true;
            }
        }
        // Enter searches, Esc just leaves: either way the results are next.
        if field.lost_focus() {
            inputs.nav.area = Area::Results;
            if ui.input(|i| i.key_pressed(Key::Enter)) {
                actions.push(BrowseAction::Search(inputs.query.clone()));
                inputs.nav.keyboard = true;
            }
        }
        let find = ui.add_enabled_ui(findable(view), |ui| {
            text_button(ui, &palette, "FIND", open, true)
        });
        let find = crate::ui::widgets::tip(find.inner, "Find in the results or the songs here (/)");
        if find.clicked() {
            if open {
                actions.push(BrowseAction::Find(None));
            } else {
                actions.push(BrowseAction::Find(Some(String::new())));
                inputs.nav.area = Area::Find;
                inputs.nav.focus_find = true;
            }
        }
    });
}

/// The page has something to find in: search results or a discography.
pub(super) fn findable(view: &View) -> bool {
    matches!(view.browse.page(), Page::Search(_) | Page::Artist(_))
}

/// The page's find, under the search field while it is open. The page
/// narrows as it is typed in; ↓ goes to what's left; Enter too, or, when
/// nothing in a search's results has it, searches Spotify with it added.
pub(super) fn find_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let hint = match view.browse.page() {
        Page::Artist(_) => "Find a song or a release here",
        _ => "Find in these results",
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let close_width = text_button_width(ui, "CLOSE") + ui.spacing().item_spacing.x;
        let nav = &mut inputs.nav;
        let field = field(
            ui,
            TextEdit::singleline(&mut nav.find)
                .id(Id::new(FIND_ID))
                .hint_text(hint)
                .desired_width((ui.available_width() - close_width).max(80.0))
                .min_size(vec2(0.0, BUTTON_HEIGHT)),
        );
        if std::mem::take(&mut nav.focus_find) {
            field.request_focus();
        }
        if field.changed() {
            actions.push(BrowseAction::Find(Some(nav.find.clone())));
        }
        if field.has_focus() {
            nav.area = Area::Find;
            if nav.rows > 0 && ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::ArrowDown))
            {
                field.surrender_focus();
                to_results(nav);
            }
        }
        if field.lost_focus() {
            let (enter, escape) =
                ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
            if escape {
                actions.push(BrowseAction::Find(None));
            } else if enter && nav.found_none {
                if let Page::Search(query) = view.browse.page() {
                    actions.push(BrowseAction::Search(wider(query, &nav.find)));
                }
            } else if enter && nav.rows > 0 {
                to_results(nav);
            }
            if nav.area == Area::Find {
                nav.area = Area::Results;
            }
        }
        let close = text_button(ui, &palette, "CLOSE", false, true);
        if crate::ui::widgets::tip(close, "Esc").clicked() {
            actions.push(BrowseAction::Find(None));
        }
    });
}

/// The keyboard goes to the first of the results.
fn to_results(nav: &mut Nav) {
    nav.area = Area::Results;
    nav.row = Some(0);
    nav.keyboard = true;
    nav.reveal = true;
}

/// A search with what's typed in the find added to it.
fn wider(query: &str, find: &str) -> String {
    format!("{query} {}", find.trim())
}

pub(super) fn home(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    set_up: &mut bool,
    actions: &mut Actions,
) {
    if !search_prompt(ui, view, set_up) {
        note(ui, view, "Type an artist, album or song and press Enter.");
    }
    if searchable(view) && !view.browse.recent_searches.is_empty() {
        section(ui, view, "RECENT SEARCHES");
        for query in &view.browse.recent_searches {
            let row = row(
                ui,
                &view.palette,
                nav,
                ROW_HEIGHT,
                Some("FORGET"),
                |painter, rect| {
                    spans(painter, rect, &[(query, body(), view.palette.list_fg)]);
                },
            );
            if row.button {
                actions.push(BrowseAction::ForgetSearch(Some(query.clone())));
            } else if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
                actions.push(BrowseAction::Search(query.clone()));
            }
        }
    }
    if !view.browse.recent.is_empty() {
        section(ui, view, "RECENTLY VIEWED");
        for artist in &view.browse.recent {
            artist_row(ui, view, nav, artist, None, actions);
        }
    }
}

/// Whether search can be used: the field is disabled until it is set up.
pub(super) fn searchable(view: &View) -> bool {
    *view.search != SearchState::NoApp
}

/// Explains what stands in the way of search, with a button to set it up
/// when that is what it needs. False when nothing does.
fn search_prompt(ui: &mut Ui, view: &View, set_up: &mut bool) -> bool {
    match view.search {
        SearchState::Ready | SearchState::SignedOut => return false,
        SearchState::NoApp => {
            note(
                ui,
                view,
                "Search isn't set up yet: besides signing in, it needs a Spotify app of \
                 your own, which takes a minute to make.",
            );
            ui.add_space(6.0);
            if text_button(ui, &view.palette, "SET UP SEARCH", false, true).clicked() {
                *set_up = true;
            }
        }
        SearchState::SigningIn => note(
            ui,
            view,
            "Spotify is confirming search in your browser. If it asks, approve Spotless \
             (with spotifast closed: both use the same sign-in port).",
        ),
    }
    true
}

pub(super) fn search_page(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    query: &str,
    set_up: &mut bool,
    actions: &mut Actions,
) {
    if search_prompt(ui, view, set_up) {
        return;
    }
    let results = match view.browse.searches.get(query) {
        None => return note(ui, view, "Press Enter to search."),
        Some(Loadable::Loading) => return note(ui, view, "Searching…"),
        Some(Loadable::Failed(error)) => {
            note(ui, view, error);
            if text_button(ui, &view.palette, "TRY AGAIN", false, true).clicked() {
                actions.push(BrowseAction::Search(query.to_string()));
            }
            return;
        }
        Some(Loadable::Ready(results)) => results,
    };
    if results.is_empty() {
        return note(ui, view, &format!("No matches for “{query}”."));
    }
    // The find narrows them to what has every word typed in it.
    let find = view.browse.finder();
    let shows = |names: &[&str]| find.as_ref().is_none_or(|find| find.matches(names));
    let artists: Vec<_> = (results.shown_artists().iter())
        .filter(|artist| shows(&[&artist.name]))
        .collect();
    let albums: Vec<_> = (results.albums.iter())
        .filter(|release| shows(&[&release.title, &release.artists]))
        .collect();
    let songs: Vec<_> = (results.songs.iter().enumerate())
        .filter(|(_, track)| find.as_ref().is_none_or(|find| find.track(track)))
        .collect();
    if find.is_some() && artists.is_empty() && albums.is_empty() && songs.is_empty() {
        nav.found_none = true;
        let typed = view.browse.find.as_deref().unwrap_or_default().trim();
        note(ui, view, &format!("Nothing found here has “{typed}”."));
        ui.add_space(6.0);
        let wider = wider(query, typed);
        let button = text_button(
            ui,
            &view.palette,
            &format!("SEARCH SPOTIFY FOR “{wider}”"),
            false,
            true,
        );
        if crate::ui::widgets::tip(button, "Enter, in the find").clicked() {
            actions.push(BrowseAction::Search(wider));
        }
        return;
    }
    if !artists.is_empty() {
        section(ui, view, "ARTISTS");
        for artist in artists {
            artist_row(ui, view, nav, artist, Some("ARTIST"), actions);
        }
        if results.reading_artists {
            note(ui, view, "Reading more artists…");
        } else if results.has_more_artists() {
            more_row(
                ui,
                view,
                nav,
                "MORE ARTISTS",
                "The next artists Spotify found (Enter)",
            )
            .then(|| actions.push(BrowseAction::MoreArtists(query.to_string())));
        }
    }
    if !albums.is_empty() {
        section(ui, view, "ALBUMS & EPS");
        for release in albums {
            album(ui, view, nav, release, true, None, actions);
        }
    }
    if !songs.is_empty() {
        section(ui, view, "SONGS");
        for (i, track) in songs {
            song(ui, view, nav, i + 1, track, true, None, actions);
        }
        if results.reading_songs {
            note(ui, view, "Reading more songs…");
        } else if results.more_songs {
            more_row(
                ui,
                view,
                nav,
                "MORE SONGS",
                "The next songs Spotify found (Enter)",
            )
            .then(|| actions.push(BrowseAction::MoreSongs(query.to_string())));
        }
    }
}

/// The row under a search's artists or songs that shows more of them:
/// whether it was pressed.
fn more_row(ui: &mut Ui, view: &View, nav: &mut Nav, label: &str, tip: &str) -> bool {
    let palette = view.palette;
    let row = row(ui, &palette, nav, TRACK_HEIGHT, None, |painter, rect| {
        painter.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            mono(),
            palette.accent,
        );
    });
    let open = matches!(row.act, Some(Act::Open));
    row.response.on_hover_text(tip).clicked() || open
}
