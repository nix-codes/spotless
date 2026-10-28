//! The catalog: search, results and artist pages on the left, and the slim
//! column with the playlist + ADD adds to on the right.
//!
//! Tab and Shift+Tab move between three areas: the search field, the
//! results and the slim column. In the results, the arrows move a
//! highlighted row; Enter opens it (an artist, an album's or a playlist's
//! songs) or plays it (a song), Insert or A adds it. In the slim column, Enter plays and Delete
//! removes. Alt+1…4 pick the release tab, Alt+5 shows similar artists,
//! Ctrl+S the order, Ctrl+E shows or hides all songs, C the pics. Esc goes
//! back to the playlists, Backspace or Alt+Left back a page. Ctrl+F (in `keys.rs`) jumps to the
//! search field.

use std::collections::BTreeSet;

use egui::{
    Align2, Color32, FocusDirection, FontId, Id, Key, Painter, Rect, Response, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::scrollbar::{self, Scrollbar};
use super::settings_view::Category;
use super::theme::{self, Palette};
use super::widgets::{
    BUTTON_HEIGHT, SMALL_HEIGHT, field, small_button, small_button_width, text_button,
};
use super::{Inputs, View};
use crate::action::{Action, Actions, Desktop};
use crate::backend::SearchState;
use crate::browse::{BrowseTab, Loadable, Page};
use crate::lastfm;
use crate::model::catalog::{Release, ReleaseTab};
use crate::model::{ArtistRef, EntryId, TrackRef, playlist::Playlist};

mod spotify;

const SLIM_WIDTH: f32 = 250.0;
const ROW_HEIGHT: f32 = 22.0;
const TRACK_HEIGHT: f32 = 20.0;
const PAD: f32 = 10.0;
/// Album rows with covers shown, and their thumbnails.
const COVER_ROW_HEIGHT: f32 = 34.0;
const THUMB: f32 = 28.0;
/// The artist's picture beside the name on its page.
const PORTRAIT: f32 = 64.0;
/// The cover beside an opened album's songs.
const BIG_COVER: f32 = 140.0;

/// An album cover, by image URL, as the interface holds it.
#[derive(Clone)]
pub enum Cover {
    Loading,
    Ready(egui::TextureHandle),
    Failed,
}

/// PageUp and PageDown move this many rows.
const PAGE_ROWS: usize = 10;
const SEARCH_ID: &str = "catalog-search";

/// Where the keyboard is in Browse, kept between frames.
#[derive(Default)]
pub struct Nav {
    area: Area,
    /// The keyboard was used last, not the mouse: the active area is framed.
    keyboard: bool,
    /// The highlighted result, counted in drawing order, on the page `page`.
    row: Option<usize>,
    page: String,
    /// Result rows drawn last frame, and so far this frame.
    rows: usize,
    drawn: usize,
    /// Set by the keys for the highlighted row: scroll to it, act on it.
    reveal: bool,
    act: Option<Act>,
    /// Ctrl+E, for the artist page.
    expand_all: bool,
    /// The highlighted song in the slim column, and the one to scroll to.
    slim: Option<EntryId>,
    slim_reveal: Option<usize>,
    slim_offset: f32,
    /// The songs selected on a Spotify playlist's page, by index, and the
    /// one Shift-clicks reach from.
    marked: BTreeSet<usize>,
    anchor: Option<usize>,
    /// What YOUR SPOTIFY's list of playlists is narrowed to, and whether
    /// its field should take the keyboard.
    filter: String,
    focus_filter: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Area {
    Search,
    /// YOUR SPOTIFY's list of playlists, with the field that finds one.
    Lists,
    #[default]
    Results,
    Slim,
}

/// The areas of each tab, in Tab's order.
fn areas(tab: BrowseTab) -> [Area; 3] {
    match tab {
        BrowseTab::Search => [Area::Search, Area::Results, Area::Slim],
        BrowseTab::Spotify => [Area::Lists, Area::Results, Area::Slim],
    }
}

#[derive(Clone, Copy)]
enum Act {
    /// Enter: open an artist or album, play a song.
    Open,
    /// Insert or A: add to the playlist.
    Add,
}

/// Songs or an album dragged from the catalog to the slim column.
enum CatalogDrag {
    Tracks(Vec<TrackRef>),
    /// An album or a Spotify playlist, by URI.
    Album {
        uri: String,
        title: String,
    },
}

impl CatalogDrag {
    fn label(&self) -> String {
        match self {
            Self::Tracks(tracks) if tracks.len() == 1 => tracks[0].title.clone(),
            Self::Tracks(tracks) => format!("{} songs", tracks.len()),
            Self::Album { title, .. } => title.clone(),
        }
    }
}

pub fn draw(ui: &mut Ui, rect: Rect, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let slim_width = SLIM_WIDTH.min(rect.width() * 0.35);
    let catalog = Rect::from_min_max(
        rect.min,
        pos2(rect.right() - slim_width - PAD, rect.bottom()),
    );
    let slim = Rect::from_min_max(pos2(catalog.right() + PAD, rect.top()), rect.max);
    theme::panel(ui.painter(), catalog, &palette);
    theme::panel(ui.painter(), slim, &palette);

    let page = view.browse.page();
    let searching = view.browse.shown == BrowseTab::Search;
    let page_key = match &view.browse.spotify_open {
        _ if searching => page_id(page),
        Some(uri) => format!("spotify-tab:{uri}"),
        None => "spotify-tab".into(),
    };
    let nav = &mut inputs.nav;
    // A new page starts at its top, highlighted only when the keyboard got
    // there (Enter on a search or an artist).
    if nav.page != page_key {
        nav.page = page_key.clone();
        nav.row = nav.keyboard.then_some(0);
        nav.rows = 0;
        nav.marked.clear();
        nav.anchor = None;
    }
    if ui.input(|i| i.pointer.any_pressed()) {
        nav.keyboard = false;
    }
    // An area of the other tab: this one's like it.
    nav.area = match nav.area {
        Area::Search if !searching => Area::Lists,
        Area::Lists if searching => Area::Results,
        area => area,
    };
    switch_area(ui, view, inputs);
    keyboard(ui, view, inputs, actions);
    match inputs.nav.area {
        Area::Results if inputs.nav.keyboard && searching => {
            theme::focus_ring(ui.painter(), catalog, &palette)
        }
        Area::Slim if inputs.nav.keyboard => theme::focus_ring(ui.painter(), slim, &palette),
        _ => {}
    }
    // The search prompt's button, to set search up in the settings.
    let mut set_up = false;
    ui.scope_builder(UiBuilder::new().max_rect(catalog.shrink(PAD)), |ui| {
        tabs(ui, view, inputs, actions);
        ui.add_space(PAD);
        if !searching {
            return spotify::draw(ui, view, inputs, actions);
        }
        search_bar(ui, view, inputs, actions);
        ui.add_space(PAD);
        let nav = &mut inputs.nav;
        nav.drawn = 0;
        let id = Id::new(("browse-scrollbar", page_id(page)));
        let area = ScrollArea::vertical().id_salt(page_id(page));
        scrollbar::show(ui, view, id, area, |area, ui| {
            area.show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                match page {
                    Page::Home => home(ui, view, nav, &mut set_up, actions),
                    Page::Search(query) => {
                        search_page(ui, view, nav, query, &mut set_up, actions);
                    }
                    Page::Artist(uri) => artist_page(ui, view, nav, uri, actions),
                }
            })
        });
        // Keys for rows that aren't there are dropped.
        nav.rows = nav.drawn;
        nav.row = nav.row.map(|row| row.min(nav.rows.saturating_sub(1)));
        nav.act = None;
        nav.reveal = false;
        nav.expand_all = false;
    });
    ui.scope_builder(UiBuilder::new().max_rect(slim.shrink(8.0)), |ui| {
        slim_column(ui, view, &mut inputs.nav, actions);
    });
    if set_up {
        inputs.settings_category = Category::Account;
        super::open_settings(view, inputs, actions);
    }
}

/// Browse is opening (Ctrl+B, BROWSE, + ADD): the keyboard goes to the
/// field of the tab it opens on, to type at once.
pub(super) fn opening(view: &View, inputs: &mut Inputs) {
    if view.browse.open {
        return;
    }
    match view.browse.shown {
        BrowseTab::Search => inputs.focus_search = true,
        BrowseTab::Spotify => focus_spotify(&mut inputs.nav),
    }
}

/// Puts the keyboard in YOUR SPOTIFY's field that finds a playlist.
pub(super) fn focus_spotify(nav: &mut Nav) {
    nav.area = Area::Lists;
    nav.focus_filter = true;
    nav.keyboard = true;
}

/// Tab and Shift+Tab: the next or previous area. Into the search field
/// puts the keyboard there; out of it takes the keyboard away.
fn switch_area(ui: &Ui, view: &View, inputs: &mut Inputs) {
    if inputs.tab == 0 {
        return;
    }
    let nav = &mut inputs.nav;
    nav.keyboard = true;
    // The search field is passed over while search isn't set up.
    let areas: Vec<Area> = areas(view.browse.shown)
        .into_iter()
        .filter(|&a| a != Area::Search || searchable(view))
        .collect();
    let at = areas.iter().position(|&a| a == nav.area).unwrap_or(0) as isize;
    nav.area = areas[(at + inputs.tab).rem_euclid(areas.len() as isize) as usize];
    inputs.tab = 0;
    if nav.area == Area::Search {
        inputs.focus_search = true;
    } else {
        ui.ctx()
            .memory_mut(|m| m.surrender_focus(Id::new(SEARCH_ID)));
    }
    if nav.area == Area::Lists {
        nav.focus_filter = true;
    } else {
        ui.ctx()
            .memory_mut(|m| m.surrender_focus(Id::new(spotify::FILTER_ID)));
    }
    if nav.area == Area::Results && nav.row.is_none() {
        nav.row = Some(0);
    }
    nav.reveal = true;
}

fn page_id(page: &Page) -> String {
    match page {
        Page::Home => "home".into(),
        Page::Search(query) => format!("search:{query}"),
        Page::Artist(uri) => uri.clone(),
    }
}

fn keyboard(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    if ui.ctx().text_edit_focused() {
        return;
    }
    let searching = view.browse.shown == BrowseTab::Search;
    let artist_page = searching && matches!(view.browse.page(), Page::Artist(_));
    let to_field = searching && searchable(view);
    let nav = &mut inputs.nav;
    let focus_search = &mut inputs.focus_search;
    let mut up_to_field = false;
    ui.input(|i| {
        let plain = i.modifiers.is_none();
        if i.key_pressed(Key::Escape) {
            actions.push(Action::CloseBrowse);
        }
        let back = (i.modifiers.alt && i.key_pressed(Key::ArrowLeft))
            || (plain && i.key_pressed(Key::Backspace));
        if back && searching && view.browse.can_go_back() {
            actions.push(Action::Back);
        }
        if plain && i.key_pressed(Key::C) {
            actions.push(Action::ToggleCovers);
        }
        if artist_page {
            let numbers = [Key::Num1, Key::Num2, Key::Num3, Key::Num4];
            for (tab, key) in ReleaseTab::ALL.into_iter().zip(numbers) {
                if i.modifiers.alt && i.key_pressed(key) {
                    actions.push(Action::SetTab(tab));
                }
            }
            if i.modifiers.alt
                && i.key_pressed(Key::Num5)
                && let Some(artist) = page_artist(view)
            {
                actions.push(Action::ShowSimilar(artist));
            }
            if i.modifiers.command && i.key_pressed(Key::S) {
                actions.push(Action::ToggleSort);
            }
            if i.modifiers.command && i.key_pressed(Key::E) {
                nav.expand_all = true;
            }
        }
        if nav.area != Area::Results || !plain {
            return;
        }
        // ↑ from the first row goes back up to the search field.
        if to_field && i.key_pressed(Key::ArrowUp) && nav.row.unwrap_or(0) == 0 {
            nav.area = Area::Search;
            nav.row = None;
            nav.keyboard = true;
            *focus_search = true;
            up_to_field = true;
            return;
        }
        let last = nav.rows.saturating_sub(1);
        let from = nav.row.unwrap_or(0);
        let to = if i.key_pressed(Key::ArrowDown) {
            Some(if nav.row.is_some() { from + 1 } else { 0 })
        } else if i.key_pressed(Key::ArrowUp) {
            Some(from.saturating_sub(1))
        } else if i.key_pressed(Key::PageDown) {
            Some(from + PAGE_ROWS)
        } else if i.key_pressed(Key::PageUp) {
            Some(from.saturating_sub(PAGE_ROWS))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(last)
        } else {
            None
        };
        if let Some(to) = to {
            nav.row = Some(to.min(last));
            nav.reveal = true;
            nav.keyboard = true;
        }
        if i.key_pressed(Key::Enter) {
            nav.act = Some(Act::Open);
        } else if i.key_pressed(Key::Insert) || i.key_pressed(Key::A) {
            nav.act = Some(Act::Add);
        }
    });
    // egui would also walk its focus up from the field, to the tabs.
    if up_to_field {
        ui.ctx().memory_mut(|m| m.move_focus(FocusDirection::None));
    }
}

fn search_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    ui.horizontal(|ui| {
        let back = text_button(ui, &palette, "BACK", false, view.browse.can_go_back());
        if super::widgets::tip(back, "Backspace or Alt+←").clicked() {
            actions.push(Action::Back);
        }
        let enabled = searchable(view);
        let hint = if enabled {
            "Search artists, albums, songs…"
        } else {
            "Search isn't set up yet"
        };
        let width = ui.available_width().max(80.0);
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
                actions.push(Action::Search(inputs.query.clone()));
                inputs.nav.keyboard = true;
            }
        }
    });
}

/// SEARCH and YOUR SPOTIFY on the left; PICS and the way back to the
/// playlists on the right.
fn tabs(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let shown = view.browse.shown;
    ui.horizontal(|ui| {
        for (tab, label, key) in [
            (BrowseTab::Search, "SEARCH", "Search Spotify (F3 or Ctrl+F)"),
            (
                BrowseTab::Spotify,
                "YOUR SPOTIFY",
                "Your Spotify playlists (F4)",
            ),
        ] {
            if text_button(ui, &palette, label, shown == tab, true)
                .on_hover_text(key)
                .clicked()
                && shown != tab
            {
                actions.push(Action::ShowBrowseTab(tab));
                inputs.nav.area = match tab {
                    BrowseTab::Search => Area::Results,
                    BrowseTab::Spotify => Area::Lists,
                };
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if text_button(ui, &palette, "ESC · PLAYLISTS", false, true).clicked() {
                actions.push(Action::CloseBrowse);
            }
            if text_button(ui, &palette, "PICS", view.browse.show_covers, true)
                .on_hover_text("Show album covers and artist pictures (C)")
                .clicked()
            {
                actions.push(Action::ToggleCovers);
            }
        });
    });
}

fn home(ui: &mut Ui, view: &View, nav: &mut Nav, set_up: &mut bool, actions: &mut Actions) {
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
                actions.push(Action::ForgetSearch(Some(query.clone())));
            } else if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
                actions.push(Action::Search(query.clone()));
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
fn searchable(view: &View) -> bool {
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

fn search_page(
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
                actions.push(Action::Search(query.to_string()));
            }
            return;
        }
        Some(Loadable::Ready(results)) => results,
    };
    if results.is_empty() {
        return note(ui, view, &format!("No matches for “{query}”."));
    }
    if !results.artists.is_empty() {
        section(ui, view, "ARTISTS");
        for artist in &results.artists {
            artist_row(ui, view, nav, artist, Some("ARTIST"), actions);
        }
    }
    if !results.albums.is_empty() {
        section(ui, view, "ALBUMS & EPS");
        for release in &results.albums {
            album(ui, view, nav, release, true, actions);
        }
    }
    if !results.songs.is_empty() {
        section(ui, view, "SONGS");
        for (i, track) in results.songs.iter().enumerate() {
            song(ui, view, nav, i + 1, track, true, actions);
        }
    }
}

fn artist_page(ui: &mut Ui, view: &View, nav: &mut Nav, uri: &str, actions: &mut Actions) {
    let discography = match view.browse.artists.get(uri) {
        None | Some(Loadable::Loading) => return note(ui, view, "Reading the discography…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(discography)) => discography,
    };
    let palette = view.palette;
    let browse = view.browse;
    ui.horizontal(|ui| {
        if browse.show_covers {
            let (rect, _) = ui.allocate_exact_size(vec2(PORTRAIT, PORTRAIT), Sense::hover());
            cover(
                ui.painter(),
                view,
                discography.artist.image.as_ref(),
                rect,
                actions,
            );
            ui.add_space(6.0);
        }
        ui.label(
            RichText::new(&discography.artist.name)
                .size(22.0)
                .color(palette.current),
        );
        ui.label(
            RichText::new("DISCOGRAPHY")
                .monospace()
                .size(10.0)
                .color(palette.dim),
        );
    });
    ui.add_space(8.0);

    let similar_tab =
        lastfm::api::is_key(&view.lastfm.api_key) || browse.similar_artists.contains_key(uri);
    let similar = browse.similar && similar_tab;
    let mut releases: Vec<&Release> = discography
        .releases
        .iter()
        .filter(|r| browse.tab.includes(r.group))
        .collect();
    if browse.newest_first {
        releases.reverse();
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (n, tab) in ReleaseTab::ALL.into_iter().enumerate() {
            let count = discography
                .releases
                .iter()
                .filter(|r| tab.includes(r.group))
                .count();
            let label = format!("{} {count}", tab.label());
            if small_button(ui, &palette, &label, !similar && tab == browse.tab)
                .on_hover_text(format!("Alt+{}", n + 1))
                .clicked()
            {
                actions.push(Action::SetTab(tab));
            }
        }
        if similar_tab
            && small_button(ui, &palette, "SIMILAR", similar)
                .on_hover_text("Artists Last.fm's listeners play alongside this one (Alt+5)")
                .clicked()
        {
            actions.push(Action::ShowSimilar(discography.artist.clone()));
        }
        if similar {
            return;
        }
        ui.add_space(12.0);
        let sort = if browse.newest_first {
            "NEWEST FIRST"
        } else {
            "OLDEST FIRST"
        };
        if small_button(ui, &palette, sort, false)
            .on_hover_text("Oldest / newest first (Ctrl+S)")
            .clicked()
        {
            actions.push(Action::ToggleSort);
        }
        let all_open =
            !releases.is_empty() && releases.iter().all(|r| browse.expanded.contains(&r.uri));
        let label = if all_open {
            "HIDE SONGS"
        } else {
            "SHOW ALL SONGS"
        };
        if small_button(ui, &palette, label, false)
            .on_hover_text("Show / hide every album's songs (Ctrl+E)")
            .clicked()
            || nav.expand_all
        {
            actions.push(Action::ExpandAlbums(if all_open {
                Vec::new()
            } else {
                releases.iter().map(|r| r.uri.clone()).collect()
            }));
        }
    });
    ui.add_space(8.0);
    if similar {
        return similar_artists(ui, view, nav, uri, actions);
    }
    if releases.is_empty() {
        note(ui, view, "Nothing here.");
    }
    for release in releases {
        album(
            ui,
            view,
            nav,
            release,
            browse.tab == ReleaseTab::AppearsOn,
            actions,
        );
    }
}

/// An artist's row, with its picture when covers are shown; opens the
/// artist's page.
fn artist_row(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    artist: &ArtistRef,
    label: Option<&str>,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let covers = view.browse.show_covers;
    let height = if covers { COVER_ROW_HEIGHT } else { ROW_HEIGHT };
    let row = row(ui, &palette, nav, height, None, |painter, rect| {
        let mut left = rect.left();
        if covers {
            let thumb = Rect::from_min_size(
                pos2(left, rect.center().y - THUMB / 2.0),
                vec2(THUMB, THUMB),
            );
            cover(painter, view, artist.image.as_ref(), thumb, actions);
            left = thumb.right() + 10.0;
        }
        let text = Rect::from_min_max(pos2(left, rect.top()), rect.max);
        let mut parts = vec![(artist.name.as_str(), body(), palette.list_fg)];
        if let Some(label) = label {
            parts.push((label, tag(), palette.dim));
        }
        spans(painter, text, &parts);
    });
    if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::OpenArtist(artist.clone()));
    }
}

/// The artist page's similar artists, each opened on Spotify by name.
fn similar_artists(ui: &mut Ui, view: &View, nav: &mut Nav, uri: &str, actions: &mut Actions) {
    let names = match view.browse.similar_artists.get(uri) {
        None | Some(Loadable::Loading) => return note(ui, view, "Asking Last.fm…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(names)) => names,
    };
    if names.is_empty() {
        return note(ui, view, "Last.fm knows no artists like this one.");
    }
    let palette = view.palette;
    let finding = view.browse.finding.as_ref().map(|(name, _)| name.as_str());
    for name in names {
        let row = row(ui, &palette, nav, ROW_HEIGHT, None, |painter, rect| {
            let state = if finding == Some(name.as_str()) {
                "FINDING…"
            } else {
                ""
            };
            spans(
                painter,
                rect,
                &[(name, body(), palette.list_fg), (state, tag(), palette.dim)],
            );
        });
        if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
            actions.push(Action::OpenSimilar(name.clone()));
        }
    }
    ui.add_space(8.0);
    note(
        ui,
        view,
        "From Last.fm: what its listeners play alongside this artist.",
    );
}

/// The artist whose page is open, once its discography is read.
fn page_artist(view: &View) -> Option<ArtistRef> {
    let Page::Artist(uri) = view.browse.page() else {
        return None;
    };
    match view.browse.artists.get(uri) {
        Some(Loadable::Ready(discography)) => Some(discography.artist.clone()),
        _ => None,
    }
}

/// An album row that opens to show its songs.
fn album(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    release: &Release,
    by: bool,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let expanded = view.browse.expanded.contains(&release.uri);
    let year = if release.year > 0 {
        release.year.to_string()
    } else {
        "----".into()
    };
    let count = match release.tracks {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    };
    let covers = view.browse.show_covers;
    let height = if covers { COVER_ROW_HEIGHT } else { ROW_HEIGHT };
    let row = row(ui, &palette, nav, height, Some("+ ADD"), |painter, rect| {
        chevron(
            painter,
            pos2(rect.left() + 5.0, rect.center().y),
            expanded,
            palette.dim,
        );
        let mut left = rect.left() + 16.0;
        if covers {
            let thumb = Rect::from_min_size(
                pos2(left, rect.center().y - THUMB / 2.0),
                vec2(THUMB, THUMB),
            );
            cover(painter, view, release.cover.as_ref(), thumb, actions);
            left = thumb.right() + 10.0;
        }
        let text = Rect::from_min_max(pos2(left, rect.top()), rect.max);
        let mut parts = vec![
            (year.as_str(), mono(), palette.dim),
            (release.title.as_str(), body(), palette.list_fg),
        ];
        if by && !release.artists.is_empty() {
            parts.push((release.artists.as_str(), body(), palette.dim));
        }
        parts.push((release.kind.as_str(), tag(), palette.dim));
        parts.push((count.as_str(), mono(), palette.dim));
        spans(painter, text, &parts);
    });
    if row.response.drag_started() {
        row.response.dnd_set_drag_payload(CatalogDrag::Album {
            uri: release.uri.clone(),
            title: release.title.clone(),
        });
    }
    if row.button || matches!(row.act, Some(Act::Add)) {
        actions.push(Action::AddAlbum(release.uri.clone()));
    } else if row.response.clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::ToggleAlbum(release.uri.clone()));
    }
    if !expanded {
        return;
    }
    match view.browse.albums.get(&release.uri) {
        None | Some(Loadable::Loading) => indented_note(ui, view, "Reading the songs…"),
        Some(Loadable::Failed(error)) => indented_note(ui, view, error),
        Some(Loadable::Ready(tracks)) if covers => {
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.add_space(16.0);
                let (rect, _) = ui.allocate_exact_size(vec2(BIG_COVER, BIG_COVER), Sense::hover());
                cover(ui.painter(), view, release.cover.as_ref(), rect, actions);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, track) in tracks.iter().enumerate() {
                        song(ui, view, nav, i + 1, track, false, actions);
                    }
                });
            });
            ui.add_space(8.0);
        }
        Some(Loadable::Ready(tracks)) => {
            for (i, track) in tracks.iter().enumerate() {
                song(ui, view, nav, i + 1, track, false, actions);
            }
            ui.add_space(4.0);
        }
    }
}

/// Draws a cover or an artist's picture in `rect`, asking for it when it
/// comes into view; a framed square stands in until it arrives.
fn cover(painter: &Painter, view: &View, url: Option<&String>, rect: Rect, actions: &mut Actions) {
    let palette = view.palette;
    let texture = url.and_then(|url| match view.covers.get(url) {
        Some(Cover::Ready(texture)) => Some(texture.id()),
        Some(_) => None,
        None => {
            if painter.clip_rect().intersects(rect) {
                actions.push(Desktop::LoadCover(url.clone()));
            }
            None
        }
    });
    match texture {
        Some(id) => {
            painter.image(
                id,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            painter.rect_filled(rect, 0.0, palette.frame);
        }
    }
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, palette.lo), StrokeKind::Outside);
}

/// A song row: double-click plays it now, + adds it. `full` shows the
/// artist and album, for search results.
fn song(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    number: usize,
    track: &TrackRef,
    full: bool,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let number = format!("{number:>2}");
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
            let indent = if full { 0.0 } else { 16.0 };
            let right = painter.text(
                rect.right_center() - vec2(4.0, 0.0),
                Align2::RIGHT_CENTER,
                &duration,
                mono(),
                palette.dim,
            );
            let text = Rect::from_min_max(
                pos2(rect.left() + indent, rect.top()),
                pos2(right.left() - 8.0, rect.bottom()),
            );
            let mut parts = vec![(number.as_str(), mono(), palette.dim)];
            if full {
                parts.push((artists.as_str(), body(), palette.list_fg));
                parts.push(("-", body(), palette.dim));
            }
            parts.push((track.title.as_str(), body(), palette.list_fg));
            if full {
                parts.push((track.album.name.as_str(), body(), palette.dim));
            }
            spans(painter, text, &parts);
        },
    );
    if row.response.drag_started() {
        row.response
            .dnd_set_drag_payload(CatalogDrag::Tracks(vec![track.clone()]));
    }
    if row.button || matches!(row.act, Some(Act::Add)) {
        actions.push(Action::AddTracks(vec![track.clone()]));
    } else if row.response.double_clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::PlayNow(track.clone()));
    }
}

fn slim_column(ui: &mut Ui, view: &View, nav: &mut Nav, actions: &mut Actions) {
    let palette = view.palette;
    let browse = view.browse;
    let Some(target) = view.list(browse.target) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("ADD TO")
                .monospace()
                .size(10.0)
                .color(palette.chrome),
        );
        // The box in the buttons' colours, which may be light; its list
        // in the window's.
        let window = ui.visuals().clone();
        let visuals = ui.visuals_mut();
        visuals.override_text_color = Some(palette.button_text);
        for (widget, fill) in [
            (&mut visuals.widgets.inactive, palette.button),
            (&mut visuals.widgets.hovered, palette.button_hi),
            (&mut visuals.widgets.active, palette.button_hi),
            (&mut visuals.widgets.open, palette.button_hi),
        ] {
            widget.bg_fill = fill;
            widget.weak_bg_fill = fill;
            widget.fg_stroke.color = palette.button_text;
        }
        egui::ComboBox::from_id_salt("add-target")
            .selected_text(&target.name)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                *ui.visuals_mut() = window;
                for &id in view.tabs {
                    let Some(list) = view.list(id) else { continue };
                    if ui
                        .selectable_label(list.id == target.id, &list.name)
                        .clicked()
                    {
                        actions.push(Action::SetTarget(list.id));
                    }
                }
            });
    });
    ui.add_space(6.0);
    let footer = 16.0;
    let rows = Rect::from_min_max(
        ui.cursor().min,
        pos2(ui.max_rect().right(), ui.max_rect().bottom() - footer),
    );
    let bar = Scrollbar::new(ui, view, Id::new("slim-scrollbar"));
    let (rows, slot) = bar.split(rows);
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        slim_keys(ui, target, nav, actions);
        slim_rows(ui, view, nav, target, (&bar, slot), actions);
    });
    ui.painter().text(
        pos2(ui.max_rect().left(), ui.max_rect().bottom()),
        Align2::LEFT_BOTTOM,
        summary(target),
        FontId::monospace(10.0),
        palette.dim,
    );
    if browse.flashing() {
        ui.ctx().request_repaint();
    }
}

/// The slim column's keys, when it is the active area: the arrows move the
/// highlighted song, Enter plays it, Delete removes it.
fn slim_keys(ui: &Ui, list: &Playlist, nav: &mut Nav, actions: &mut Actions) {
    nav.slim = nav.slim.filter(|&id| list.index_of(id).is_some());
    if nav.area != Area::Slim || list.is_empty() || ui.ctx().text_edit_focused() {
        return;
    }
    let last = list.len() - 1;
    let from = nav.slim.or(list.cursor()).and_then(|id| list.index_of(id));
    let (to, enter, delete) = ui.input(|i| {
        if !i.modifiers.is_none() {
            return (None, false, false);
        }
        let at = from.unwrap_or(0);
        let to = if i.key_pressed(Key::ArrowDown) {
            Some(from.map_or(0, |at| at + 1))
        } else if i.key_pressed(Key::ArrowUp) {
            Some(at.saturating_sub(1))
        } else if i.key_pressed(Key::PageDown) {
            Some(at + PAGE_ROWS)
        } else if i.key_pressed(Key::PageUp) {
            Some(at.saturating_sub(PAGE_ROWS))
        } else if i.key_pressed(Key::Home) {
            Some(0)
        } else if i.key_pressed(Key::End) {
            Some(last)
        } else {
            None
        };
        (to, i.key_pressed(Key::Enter), i.key_pressed(Key::Delete))
    });
    if let Some(to) = to.map(|to| to.min(last)) {
        nav.slim = Some(list.entries()[to].id);
        nav.slim_reveal = Some(to);
        nav.keyboard = true;
    }
    let Some(id) = nav.slim else { return };
    if enter {
        actions.push(Action::PlayEntry(list.id, id));
    }
    if delete && let Some(index) = list.index_of(id) {
        // The next song takes the highlight, or the one before the last.
        let next = list
            .entries()
            .get(index + 1)
            .or(index.checked_sub(1).and_then(|i| list.entries().get(i)));
        nav.slim = next.map(|entry| entry.id);
        actions.push(Action::Remove(list.id, vec![id]));
    }
}

fn slim_rows(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    list: &Playlist,
    (bar, slot): (&Scrollbar, Rect),
    actions: &mut Actions,
) {
    let palette = view.palette;
    let dragging = egui::DragAndDrop::payload::<CatalogDrag>(ui.ctx());
    if let Some(drag) = &dragging {
        super::widgets::drag_ghost(ui, &palette, &drag.label());
    }
    let pointer = ui.ctx().pointer_interact_pos();
    let area_rect = ui.max_rect();
    let hovered = pointer.is_some_and(|p| area_rect.contains(p));
    if list.is_empty() {
        let text = if dragging.is_some() {
            "Drop here to add."
        } else {
            "Empty. Use + ADD or drag songs here."
        };
        ui.label(RichText::new(text).color(palette.dim));
        if dragging.is_some() && hovered {
            ui.painter().rect_stroke(
                area_rect,
                0.0,
                Stroke::new(1.0, palette.accent),
                StrokeKind::Inside,
            );
        }
        drop(ui, dragging.is_some() && hovered, 0, actions);
        bar.paint(ui, slot, 0.0, 0.0, false);
        return;
    }
    let height = 17.0;
    // Named, so it doesn't take the scroll of the playlist drawn here
    // before Browse opened.
    let mut area = ScrollArea::vertical()
        .id_salt("slim-rows")
        .auto_shrink([false, false])
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden);
    let view_height = ui.available_height();
    let furthest = (list.len() as f32 * height - view_height).max(0.0);
    let thumb_to = bar.interact(ui, slot, nav.slim_offset, furthest);
    if let Some(index) = nav.slim_reveal.take() {
        let top = index as f32 * height;
        let offset = if top < nav.slim_offset {
            top
        } else if top + height > nav.slim_offset + view_height {
            top + height - view_height
        } else {
            nav.slim_offset
        };
        area = area.vertical_scroll_offset(offset);
    // Bring the songs just added into view.
    } else if let Some(last) = list
        .entries()
        .iter()
        .rposition(|entry| view.browse.glow(entry.id) > 0.9)
    {
        area = area.vertical_scroll_offset(((last + 1) as f32 * height - view_height).max(0.0));
    }
    if let Some(offset) = thumb_to {
        area = area.vertical_scroll_offset(offset);
    }
    let current = list.cursor().filter(|_| !list.detached());
    let mut drop_at: Option<(usize, f32)> = None;
    // Before show_rows, which counts the spacing into each row's height.
    ui.spacing_mut().item_spacing.y = 0.0;
    let output = area.show_rows(ui, height, list.len(), |ui, range| {
        for index in range {
            let entry = &list.entries()[index];
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
            if response.clicked() {
                nav.slim = Some(entry.id);
                nav.area = Area::Slim;
            }
            if response.double_clicked() {
                actions.push(Action::PlayEntry(list.id, entry.id));
            }
            if dragging.is_some()
                && let Some(p) = pointer
                && hovered
                && rect.y_range().contains(p.y)
            {
                drop_at = Some(if p.y < rect.center().y {
                    (index, rect.top())
                } else {
                    (index + 1, rect.bottom())
                });
            }
            let painter = ui.painter_at(rect);
            if nav.area == Area::Slim && nav.slim == Some(entry.id) {
                painter.rect_filled(rect, 0.0, palette.selected_bg);
            }
            let glow = view.browse.glow(entry.id);
            if glow > 0.0 {
                painter.rect_filled(rect, 0.0, palette.accent.gamma_multiply(0.45 * glow));
            }
            let color = if Some(entry.id) == current {
                palette.current
            } else {
                palette.list_fg
            };
            let seconds = entry.track.duration_ms / 1000;
            let duration = painter.text(
                rect.right_center() - vec2(2.0, 0.0),
                Align2::RIGHT_CENTER,
                format!("{}:{:02}", seconds / 60, seconds % 60),
                FontId::monospace(10.0),
                palette.dim,
            );
            let text = Rect::from_min_max(rect.min, pos2(duration.left() - 6.0, rect.bottom()));
            let number = format!("{}.", index + 1);
            spans(
                &painter,
                text,
                &[
                    (number.as_str(), FontId::monospace(10.0), palette.dim),
                    (
                        entry.track.title.as_str(),
                        FontId::proportional(12.0),
                        color,
                    ),
                ],
            );
        }
    });
    nav.slim_offset = output.state.offset.y;
    bar.paint(ui, slot, nav.slim_offset, furthest, thumb_to.is_some());
    if dragging.is_none() || !hovered {
        return;
    }
    // Below the last row: the end.
    let (index, y) = drop_at.unwrap_or((
        list.len(),
        (output.inner_rect.top() - output.state.offset.y + list.len() as f32 * height)
            .min(output.inner_rect.bottom() - 1.0),
    ));
    ui.painter().with_clip_rect(output.inner_rect).hline(
        output.inner_rect.x_range(),
        y,
        Stroke::new(2.0, palette.accent),
    );
    drop(ui, true, index, actions);
}

/// Takes a catalog drag released over the slim column, to add before the
/// row at `index`.
fn drop(ui: &Ui, over: bool, index: usize, actions: &mut Actions) {
    if !over || !ui.input(|i| i.pointer.any_released()) {
        return;
    }
    let Some(drag) = egui::DragAndDrop::take_payload::<CatalogDrag>(ui.ctx()) else {
        return;
    };
    actions.push(match &*drag {
        CatalogDrag::Tracks(tracks) => Action::DropTracks(tracks.clone(), index),
        CatalogDrag::Album { uri, .. } => Action::DropAlbum(uri.clone(), index),
    });
}

fn summary(list: &Playlist) -> String {
    let total: u64 = list
        .entries()
        .iter()
        .map(|e| u64::from(e.track.duration_ms))
        .sum::<u64>()
        / 1000;
    let time = if total >= 3600 {
        format!("{}:{:02}:{:02}", total / 3600, total / 60 % 60, total % 60)
    } else {
        format!("{}:{:02}", total / 60, total % 60)
    };
    match list.len() {
        1 => format!("1 song · {time}"),
        n => format!("{n} songs · {time}"),
    }
}

struct Row {
    response: Response,
    /// The row's button was clicked.
    button: bool,
    /// A key pressed for this row, the highlighted one.
    act: Option<Act>,
}

/// A full-width clickable row, painted by `paint` left of an optional
/// button at its right end. Clicking it highlights it.
fn row(
    ui: &mut Ui,
    palette: &Palette,
    nav: &mut Nav,
    height: f32,
    button: Option<&str>,
    paint: impl FnOnce(&Painter, Rect),
) -> Row {
    let index = nav.drawn;
    nav.drawn += 1;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    if response.clicked() || response.drag_started() {
        nav.row = Some(index);
        nav.area = Area::Results;
    }
    let lit = nav.row == Some(index) && nav.area == Area::Results;
    if lit {
        ui.painter().rect_filled(rect, 0.0, palette.selected_bg);
        if nav.reveal {
            // The first row brings the page's top along: the artist's name
            // and picture, a section's title.
            let mut reveal = rect;
            if index == 0 {
                reveal.min.y = ui.min_rect().top();
            }
            ui.scroll_to_rect(reveal, None);
        }
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, palette.hi.gamma_multiply(0.35));
    }
    let button_width = button.map_or(0.0, |label| small_button_width(ui, label) + 6.0);
    let content = Rect::from_min_max(
        rect.min + vec2(4.0, 0.0),
        pos2(rect.right() - button_width - 4.0, rect.bottom()),
    );
    paint(&ui.painter_at(content), content);
    let clicked = button.is_some_and(|label| {
        let at = Rect::from_center_size(
            pos2(rect.right() - button_width / 2.0, rect.center().y),
            vec2(button_width - 6.0, SMALL_HEIGHT),
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(at));
        let response = small_button(&mut child, palette, label, false);
        // + and + ADD have a key; FORGET hasn't.
        if label.starts_with('+') {
            response.on_hover_text("Add (Insert or A)").clicked()
        } else {
            response.clicked()
        }
    });
    Row {
        response,
        button: clicked,
        act: if lit { nav.act.take() } else { None },
    }
}

/// Paints text pieces left to right with a gap, clipped to `rect`.
fn spans(painter: &Painter, rect: Rect, parts: &[(&str, FontId, Color32)]) {
    let painter = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    let mut x = rect.left();
    for (text, font, color) in parts {
        if text.is_empty() {
            continue;
        }
        let drawn = painter.text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            *text,
            font.clone(),
            *color,
        );
        x = drawn.right() + 8.0;
        if x > rect.right() {
            break;
        }
    }
}

fn chevron(painter: &Painter, c: egui::Pos2, open: bool, color: Color32) {
    let points = if open {
        vec![
            pos2(c.x - 4.0, c.y - 2.0),
            pos2(c.x + 4.0, c.y - 2.0),
            pos2(c.x, c.y + 3.0),
        ]
    } else {
        vec![
            pos2(c.x - 2.0, c.y - 4.0),
            pos2(c.x + 3.0, c.y),
            pos2(c.x - 2.0, c.y + 4.0),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points,
        color,
        egui::Stroke::NONE,
    ));
}

fn section(ui: &mut Ui, view: &View, title: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title)
            .monospace()
            .size(10.0)
            .color(view.palette.chrome),
    );
    ui.add_space(4.0);
}

fn note(ui: &mut Ui, view: &View, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(view.palette.dim)).wrap());
}

fn indented_note(ui: &mut Ui, view: &View, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(22.0);
        note(ui, view, text);
    });
}

fn body() -> FontId {
    FontId::proportional(12.5)
}

fn mono() -> FontId {
    FontId::monospace(11.0)
}

fn tag() -> FontId {
    FontId::monospace(9.0)
}
