//! The catalog: search, results and artist pages on the left, and the slim
//! column with the playlist + ADD adds to on the right.
//!
//! Tab and Shift+Tab move between three areas: the search field, the
//! results and the slim column. In the results, the arrows move a
//! highlighted row; Enter opens it (an artist, an album's or a playlist's
//! songs) or plays it (a song), Insert or A adds it. In the slim column,
//! Enter plays and Delete removes. On an artist's page Alt+1…4 pick the
//! release tab, Alt+5 shows similar artists, Alt+6 the popular songs,
//! Shift+Insert adds all the page shows, Ctrl+S sets the order and Ctrl+E
//! shows or hides all songs. C turns the pics on and off. Esc goes back to
//! the playlists, Backspace or Alt+Left back a page. Ctrl+F (in `keys.rs`)
//! jumps to the search field. / opens the page's find, under the search
//! field: what's typed in it narrows the results or the discography
//! shown, and Esc closes it.

use std::collections::BTreeSet;

use egui::{
    Align2, Color32, FocusDirection, FontId, Id, InputState, Key, Modifiers, Painter, Rect,
    Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, TextEdit, Ui, UiBuilder, WidgetInfo,
    WidgetType, pos2, vec2,
};

use super::scrollbar::{self, Scrollbar};
use super::settings_view::Category;
use super::theme::{self, Palette};
use super::widgets::{
    BUTTON_HEIGHT, SMALL_HEIGHT, field, share_menu, small_action, small_button, small_button_width,
    text_button, text_button_width,
};
use super::{Inputs, View};
use crate::action::{Action, Actions, Desktop};
use crate::backend::SearchState;
use crate::browse::{AddAll, BrowseTab, Find, Loadable, Page};
use crate::lastfm;
use crate::model::catalog::{Discography, Release, ReleaseTab};
use crate::model::{ArtistRef, EntryId, Source, TrackRef, playlist::Playlist};

mod lastfm_tab;
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
const FIND_ID: &str = "catalog-find";

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
    /// Ctrl+E and Shift+Insert, for the artist page.
    expand_all: bool,
    add_all: bool,
    /// What's typed in the page's find, and whether it should take the
    /// keyboard; nothing on the page had it, last frame.
    find: String,
    focus_find: bool,
    found_none: bool,
    /// The highlighted song in the slim column, and the one to scroll to.
    slim: Option<EntryId>,
    slim_reveal: Option<usize>,
    slim_offset: f32,
    /// The ADD TO box is highlighted in the slim column, not a song; its
    /// list was open last frame, and the playlist highlighted in it.
    on_box: bool,
    box_open: bool,
    pick: Option<usize>,
    /// Alt+↓ or ↑: open the list, or choose from it.
    toggle_box: bool,
    /// The songs selected on a Spotify playlist's page, by index, and the
    /// one Shift-clicks reach from.
    marked: BTreeSet<usize>,
    anchor: Option<usize>,
    /// What YOUR SPOTIFY's list of playlists is narrowed to, and whether
    /// its field should take the keyboard.
    filter: String,
    focus_filter: bool,
    /// Its caret was at the end last frame: → leaves it for the songs.
    filter_at_end: bool,
    /// How the LAST.FM tab's songs are sorted.
    loved_order: lastfm_tab::Order,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Area {
    Search,
    /// The page's find, while it's open.
    Find,
    /// YOUR SPOTIFY's list of playlists, with the field that finds one.
    Lists,
    #[default]
    Results,
    Slim,
}

/// The areas of each tab, in Tab's order.
fn areas(tab: BrowseTab) -> &'static [Area] {
    match tab {
        BrowseTab::Search => &[Area::Search, Area::Results, Area::Slim],
        BrowseTab::Spotify => &[Area::Lists, Area::Results, Area::Slim],
        BrowseTab::LastFm => &[Area::Results, Area::Slim],
    }
}

/// LAST.FM is there while Last.fm is connected and search set up, which
/// finds its songs on Spotify.
pub(super) fn lastfm_shown(view: &View) -> bool {
    let connected = matches!(view.lastfm.status, lastfm::LastFmStatus::Connected { .. });
    connected && searchable(view)
}

/// The tab Browse shows: SEARCH in place of LAST.FM while it isn't there.
fn shown(view: &View) -> BrowseTab {
    match view.browse.shown {
        BrowseTab::LastFm if !lastfm_shown(view) => BrowseTab::Search,
        tab => tab,
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
    /// Songs loved on Last.fm, to find on Spotify.
    Loved(Vec<lastfm::LovedSong>),
}

impl CatalogDrag {
    fn label(&self) -> String {
        match self {
            Self::Tracks(tracks) if tracks.len() == 1 => tracks[0].title.clone(),
            Self::Tracks(tracks) => format!("{} songs", tracks.len()),
            Self::Album { title, .. } => title.clone(),
            Self::Loved(songs) if songs.len() == 1 => songs[0].title.clone(),
            Self::Loved(songs) => format!("{} songs", songs.len()),
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
    let tab = shown(view);
    let searching = tab == BrowseTab::Search;
    let page_key = match &view.browse.spotify_open {
        _ if searching => page_id(page),
        _ if tab == BrowseTab::LastFm => "lastfm-tab".into(),
        Some(uri) => format!("spotify-tab:{uri}"),
        None => "spotify-tab".into(),
    };
    // The field shows the search shown, however it was made.
    if inputs.nav.page != page_key
        && let Page::Search(query) = page
        && searching
    {
        inputs.query = query.clone();
    }
    if view.browse.find.is_none() {
        inputs.nav.find.clear();
    }
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
    nav.area = match (nav.area, tab) {
        (Area::Search, BrowseTab::Spotify) => Area::Lists,
        (Area::Search | Area::Lists, BrowseTab::LastFm) => Area::Results,
        (Area::Lists, BrowseTab::Search) => Area::Results,
        (area, _) => area,
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
        match tab {
            BrowseTab::Spotify => return spotify::draw(ui, view, inputs, actions),
            BrowseTab::LastFm => return lastfm_tab::draw(ui, view, &mut inputs.nav, actions),
            BrowseTab::Search => {}
        }
        search_bar(ui, view, inputs, actions);
        if view.browse.find.is_some() {
            find_bar(ui, view, inputs, actions);
        }
        ui.add_space(PAD);
        let nav = &mut inputs.nav;
        nav.drawn = 0;
        nav.found_none = false;
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
        nav.add_all = false;
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
    focus_tab(shown(view), inputs);
}

/// The keyboard goes to `tab`'s field, or its list when it has none.
pub(super) fn focus_tab(tab: BrowseTab, inputs: &mut Inputs) {
    match tab {
        BrowseTab::Search => inputs.focus_search = true,
        BrowseTab::Spotify => focus_spotify(&mut inputs.nav),
        BrowseTab::LastFm => inputs.nav.area = Area::Results,
    }
}

/// The tab `step` tabs on from the one shown, round from the last to the
/// first and back.
pub(super) fn step_tab(view: &View, step: isize) -> BrowseTab {
    let mut tabs = vec![BrowseTab::Search, BrowseTab::Spotify];
    if lastfm_shown(view) {
        tabs.push(BrowseTab::LastFm);
    }
    let at = tabs.iter().position(|&tab| tab == shown(view)).unwrap_or(0);
    tabs[(at as isize + step).rem_euclid(tabs.len() as isize) as usize]
}

/// Alt+↓ or ↑, from anywhere in Browse: opens the ADD TO list, as in a
/// Windows box, or chooses the playlist highlighted in it.
pub(super) fn toggle_box(ui: &Ui, inputs: &mut Inputs) {
    enter_area(ui, inputs, Area::Slim);
    let nav = &mut inputs.nav;
    nav.on_box = true;
    nav.slim = None;
    nav.keyboard = true;
    nav.toggle_box = true;
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
    let mut areas: Vec<Area> = areas(shown(view))
        .iter()
        .copied()
        .filter(|&a| a != Area::Search || searchable(view))
        .collect();
    // The find comes after the search field, while it's open.
    if view.browse.find.is_some() && shown(view) == BrowseTab::Search {
        let at = areas.iter().position(|&a| a == Area::Results).unwrap_or(0);
        areas.insert(at, Area::Find);
    }
    let at = areas.iter().position(|&a| a == nav.area).unwrap_or(0) as isize;
    let to = areas[(at + inputs.tab).rem_euclid(areas.len() as isize) as usize];
    inputs.tab = 0;
    enter_area(ui, inputs, to);
}

/// ← and →: the panel to the left or right, stopping at the ends. The
/// search field sits over the results, so it counts as them.
fn move_panel(ui: &Ui, view: &View, inputs: &mut Inputs, step: isize) {
    let panels: Vec<Area> = areas(shown(view))
        .iter()
        .copied()
        .filter(|&a| a != Area::Search)
        .collect();
    let area = match inputs.nav.area {
        Area::Search | Area::Find => Area::Results,
        area => area,
    };
    let at = panels.iter().position(|&a| a == area).unwrap_or(0) as isize;
    let to = (at + step).clamp(0, panels.len() as isize - 1) as usize;
    if panels[to] != inputs.nav.area {
        inputs.nav.keyboard = true;
        enter_area(ui, inputs, panels[to]);
    }
    // egui would also walk its focus sideways from a field given it.
    ui.ctx().memory_mut(|m| m.move_focus(FocusDirection::None));
}

/// Puts the keyboard in `area`: in its field, if it has one, out of the
/// others'.
fn enter_area(ui: &Ui, inputs: &mut Inputs, area: Area) {
    let nav = &mut inputs.nav;
    nav.area = area;
    if nav.area == Area::Search {
        inputs.focus_search = true;
    } else {
        ui.ctx()
            .memory_mut(|m| m.surrender_focus(Id::new(SEARCH_ID)));
    }
    if nav.area == Area::Find {
        nav.focus_find = true;
    } else {
        ui.ctx().memory_mut(|m| m.surrender_focus(Id::new(FIND_ID)));
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
    // The ADD TO list's keys are its own: Esc closes it, not Browse.
    if ui.ctx().text_edit_focused() || inputs.nav.box_open {
        return;
    }
    let searching = shown(view) == BrowseTab::Search;
    let artist_page = searching && matches!(view.browse.page(), Page::Artist(_));
    let to_field = searching && searchable(view);
    let finding = searching && view.browse.find.is_some();
    let findable = searching && findable(view);
    let nav = &mut inputs.nav;
    let focus_search = &mut inputs.focus_search;
    let mut up_to_field = false;
    let mut panel = 0;
    ui.input(|i| {
        let plain = i.modifiers.is_none();
        // Esc closes the find first, then Browse.
        if i.key_pressed(Key::Escape) {
            actions.push(if finding {
                Action::Find(None)
            } else {
                Action::CloseBrowse
            });
        }
        if findable && !i.modifiers.command && !i.modifiers.alt && i.key_pressed(Key::Slash) {
            actions.push(Action::Find(Some(nav.find.clone())));
            nav.area = Area::Find;
            nav.focus_find = true;
            nav.keyboard = true;
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
            if i.modifiers.alt && i.key_pressed(Key::Num6) {
                actions.push(Action::ShowPopular);
            }
            if i.modifiers.command && i.key_pressed(Key::S) {
                actions.push(Action::ToggleSort);
            }
            if i.modifiers.command && i.key_pressed(Key::E) {
                nav.expand_all = true;
            }
            if add_all_pressed(i) {
                nav.add_all = true;
            }
        }
        if plain && i.key_pressed(Key::ArrowLeft) {
            panel = -1;
        } else if plain && i.key_pressed(Key::ArrowRight) {
            panel = 1;
        }
        if nav.area != Area::Results || !plain {
            return;
        }
        // ↑ from the first row goes back up to the find, or the search
        // field.
        if (to_field || finding) && i.key_pressed(Key::ArrowUp) && nav.row.unwrap_or(0) == 0 {
            if finding {
                nav.area = Area::Find;
                nav.focus_find = true;
            } else {
                nav.area = Area::Search;
                *focus_search = true;
            }
            nav.row = None;
            nav.keyboard = true;
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
    if panel != 0 {
        move_panel(ui, view, inputs, panel);
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
                actions.push(Action::Search(inputs.query.clone()));
                inputs.nav.keyboard = true;
            }
        }
        let find = ui.add_enabled_ui(findable(view), |ui| {
            text_button(ui, &palette, "FIND", open, true)
        });
        let find = super::widgets::tip(find.inner, "Find in the results or the songs here (/)");
        if find.clicked() {
            if open {
                actions.push(Action::Find(None));
            } else {
                actions.push(Action::Find(Some(String::new())));
                inputs.nav.area = Area::Find;
                inputs.nav.focus_find = true;
            }
        }
    });
}

/// The page has something to find in: search results or a discography.
fn findable(view: &View) -> bool {
    matches!(view.browse.page(), Page::Search(_) | Page::Artist(_))
}

/// The page's find, under the search field while it is open. The page
/// narrows as it is typed in; ↓ goes to what's left; Enter too, or, when
/// nothing in a search's results has it, searches Spotify with it added.
fn find_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
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
            actions.push(Action::Find(Some(nav.find.clone())));
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
                actions.push(Action::Find(None));
            } else if enter && nav.found_none {
                if let Page::Search(query) = view.browse.page() {
                    actions.push(Action::Search(wider(query, &nav.find)));
                }
            } else if enter && nav.rows > 0 {
                to_results(nav);
            }
            if nav.area == Area::Find {
                nav.area = Area::Results;
            }
        }
        let close = text_button(ui, &palette, "CLOSE", false, true);
        if super::widgets::tip(close, "Esc").clicked() {
            actions.push(Action::Find(None));
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

/// SEARCH and YOUR SPOTIFY on the left; PICS and the way back to the
/// playlists on the right.
fn tabs(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let shown = shown(view);
    let mut shown_rect = None;
    let lastfm_tab = lastfm_shown(view).then_some((
        BrowseTab::LastFm,
        "LAST.FM",
        "Songs you loved on Last.fm (F5)",
    ));
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (tab, label, key) in [
            (BrowseTab::Search, "SEARCH", "Search Spotify (F3 or Ctrl+F)"),
            (
                BrowseTab::Spotify,
                "YOUR SPOTIFY",
                "Your Spotify playlists (F4)",
            ),
        ]
        .into_iter()
        .chain(lastfm_tab)
        {
            let font = FontId::monospace(11.0);
            let width = ui
                .painter()
                .layout_no_wrap(label.into(), font.clone(), palette.dim)
                .size()
                .x;
            let (rect, response) =
                ui.allocate_exact_size(vec2(width + 24.0, BUTTON_HEIGHT), Sense::click());
            response.widget_info(|| {
                WidgetInfo::selected(WidgetType::SelectableLabel, true, shown == tab, label)
            });
            let color =
                super::playlist_view::paint_tab(ui, &palette, rect, &response, shown == tab);
            ui.painter()
                .text(rect.center(), Align2::CENTER_CENTER, label, font, color);
            if shown == tab {
                shown_rect = Some(rect);
            }
            if response.on_hover_text(key).clicked() && shown != tab {
                actions.push(Action::ShowBrowseTab(tab));
                inputs.nav.area = match tab {
                    BrowseTab::Search | BrowseTab::LastFm => Area::Results,
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
    super::playlist_view::tab_baseline(ui, &palette, shown_rect, row.response.rect.bottom());
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
    // The find narrows them to what has every word typed in it.
    let find = view.browse.finder();
    let shows = |names: &[&str]| find.as_ref().is_none_or(|find| find.matches(names));
    let artists: Vec<_> = (results.artists.iter())
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
        if super::widgets::tip(button, "Enter, in the find").clicked() {
            actions.push(Action::Search(wider));
        }
        return;
    }
    if !artists.is_empty() {
        section(ui, view, "ARTISTS");
        for artist in artists {
            artist_row(ui, view, nav, artist, Some("ARTIST"), actions);
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
            more_songs(ui, view, nav, query, actions);
        }
    }
}

/// The row under a search's songs that reads the next page of them.
fn more_songs(ui: &mut Ui, view: &View, nav: &mut Nav, query: &str, actions: &mut Actions) {
    let palette = view.palette;
    let row = row(ui, &palette, nav, TRACK_HEIGHT, None, |painter, rect| {
        painter.text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            "MORE SONGS",
            mono(),
            palette.accent,
        );
    });
    let open = matches!(row.act, Some(Act::Open));
    let tip = "The next songs Spotify found (Enter)";
    if row.response.on_hover_text(tip).clicked() || open {
        actions.push(Action::MoreSongs(query.to_string()));
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
    let popular = browse.popular_shown();
    let releases = browse.releases_shown();
    // Where the row's last button ended, for ADD ALL to go after it.
    let end = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (n, tab) in ReleaseTab::ALL.into_iter().enumerate() {
            let count = discography
                .releases
                .iter()
                .filter(|r| tab.includes(r.group))
                .count();
            let label = format!("{} {count}", tab.label());
            let on = !similar && !popular && tab == browse.tab;
            if small_button(ui, &palette, &label, on)
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
        if !discography.popular.is_empty()
            && small_button(ui, &palette, "POPULAR", popular)
                .on_hover_text("Its most played songs, as Spotify ranks them (Alt+6)")
                .clicked()
        {
            actions.push(Action::ShowPopular);
        }
        if similar || popular {
            return ui.cursor().min;
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
        ui.cursor().min
    });
    if !similar {
        add_all(ui, view, (end.inner, nav.add_all), actions);
    }
    ui.add_space(8.0);
    if similar {
        return similar_artists(ui, view, nav, uri, actions);
    }
    let find = browse.finder();
    if popular {
        return popular_songs(ui, view, nav, discography, find.as_ref(), actions);
    }
    if releases.is_empty() {
        note(ui, view, "Nothing here.");
    }
    if let Some(find) = &find {
        let unread = releases
            .iter()
            .filter(|r| matches!(browse.albums.get(&r.uri), None | Some(Loadable::Loading)))
            .count();
        if unread > 0 {
            note(
                ui,
                view,
                &format!("Reading the songs, {unread} releases to go…"),
            );
            ui.add_space(4.0);
        }
        let mut any = false;
        for release in releases {
            let by = browse.tab == ReleaseTab::AppearsOn;
            any |= album(ui, view, nav, release, by, Some(find), actions);
        }
        if !any && unread == 0 {
            nav.found_none = true;
            let typed = browse.find.as_deref().unwrap_or_default().trim();
            note(ui, view, &format!("No song or release here has “{typed}”."));
        }
        return;
    }
    for release in releases {
        album(
            ui,
            view,
            nav,
            release,
            browse.tab == ReleaseTab::AppearsOn,
            None,
            actions,
        );
    }
}

/// The artist's most played songs, Spotify's pick, in its order: what
/// POPULAR shows in place of the releases.
fn popular_songs(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    discography: &Discography,
    find: Option<&Find>,
    actions: &mut Actions,
) {
    let artist = Some(discography.artist.uri.as_str());
    let mut any = false;
    for (i, track) in discography.popular.iter().enumerate() {
        if find.is_none_or(|find| find.track(track)) {
            song(ui, view, nav, i + 1, track, true, artist, actions);
            any = true;
        }
    }
    if !any {
        nav.found_none = true;
        let typed = view.browse.find.as_deref().unwrap_or_default().trim();
        note(ui, view, &format!("No popular song has “{typed}”."));
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
    share(&row.response, "Copy artist link", &artist.uri, actions);
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

/// An album row that opens to show its songs. With a find, only an album
/// with it in its title or its songs' shows, with those songs open:
/// whether it showed.
fn album(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    release: &Release,
    by: bool,
    find: Option<&Find>,
    actions: &mut Actions,
) -> bool {
    let palette = view.palette;
    let read = match view.browse.albums.get(&release.uri) {
        Some(Loadable::Ready(tracks)) => Some(tracks),
        _ => None,
    };
    let found = match find.map(|find| view.browse.found_in(release, find)) {
        Some(None) => return false,
        Some(Some(found)) => found,
        None => Vec::new(),
    };
    let expanded = view.browse.expanded.contains(&release.uri) || !found.is_empty();
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
    share(&row.response, "Copy album link", &release.uri, actions);
    if !expanded {
        return true;
    }
    let tracks: Vec<(usize, &TrackRef)> = match (read, found.is_empty()) {
        (Some(tracks), true) => tracks.iter().enumerate().collect(),
        _ => found,
    };
    match view.browse.albums.get(&release.uri) {
        None | Some(Loadable::Loading) => indented_note(ui, view, "Reading the songs…"),
        Some(Loadable::Failed(error)) => indented_note(ui, view, error),
        Some(Loadable::Ready(_)) if covers => {
            ui.add_space(4.0);
            ui.horizontal_top(|ui| {
                ui.add_space(16.0);
                let (rect, _) = ui.allocate_exact_size(vec2(BIG_COVER, BIG_COVER), Sense::hover());
                cover(ui.painter(), view, release.cover.as_ref(), rect, actions);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (i, track) in tracks {
                        song(ui, view, nav, i + 1, track, false, None, actions);
                    }
                });
            });
            ui.add_space(8.0);
        }
        Some(Loadable::Ready(_)) => {
            for (i, track) in tracks {
                song(ui, view, nav, i + 1, track, false, None, actions);
            }
            ui.add_space(4.0);
        }
    }
    true
}

/// Shift+Insert, ADD ALL's key on an artist page. On Windows it arrives as
/// a paste, of whatever the clipboard holds, with Shift held (Ctrl+V's has
/// Ctrl).
pub(super) fn add_all_pressed(i: &InputState) -> bool {
    let shift = i.modifiers.matches_exact(Modifiers::SHIFT);
    shift
        && (i.key_pressed(Key::Insert)
            || i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))))
}

/// An artist's page shows on the search tab: Shift+Insert is ADD ALL's.
pub(super) fn adds_all(view: &View) -> bool {
    view.browse.open
        && shown(view) == BrowseTab::Search
        && matches!(view.browse.page(), Page::Artist(_))
}

/// The ADD ALL button, at the right end of an artist page's tabs: adds
/// what the page shows to the playlist + ADD adds to. The tab's releases, in the
/// order shown; the popular songs; with a find, what it found: the songs
/// found, and whole the releases found by their title.
fn add_all(ui: &mut Ui, view: &View, (end, pressed): (egui::Pos2, bool), actions: &mut Actions) {
    let browse = view.browse;
    let (what, reading) = match browse.add_all() {
        None => return,
        Some(AddAll::Reading) => (String::new(), true),
        Some(AddAll::Songs(songs)) if songs.len() == 1 => ("the song shown".into(), false),
        Some(AddAll::Songs(songs)) => (format!("the {} songs shown", songs.len()), false),
        Some(AddAll::Releases(releases)) if releases.len() == 1 => {
            ("the release shown".into(), false)
        }
        Some(AddAll::Releases(releases)) => {
            (format!("the {} releases shown", releases.len()), false)
        }
    };
    let to = view
        .list(browse.target)
        .map_or(String::new(), |list| format!(" to “{}”", list.name));
    // At the right end, over the rows' own + buttons (3 points in from
    // the edge): on the tabs' last line, else on a line of its own.
    let label = "+ ADD ALL";
    let size = vec2(small_button_width(ui, label), SMALL_HEIGHT);
    let right = ui.max_rect().right() - 3.0;
    let top = if right - size.x - 12.0 >= end.x {
        end.y
    } else {
        let (line, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), SMALL_HEIGHT + 4.0),
            Sense::hover(),
        );
        line.bottom() - SMALL_HEIGHT
    };
    let rect = Rect::from_min_size(pos2(right - size.x, top), size);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    let button = small_action(&mut child, &view.palette, label, !reading);
    let tip = if reading {
        "Waiting for the songs to be read".to_string()
    } else {
        format!("Adds {what}{to} (Shift+Insert)")
    };
    if super::widgets::tip(button, &tip).clicked() || (pressed && !reading) {
        actions.push(Action::AddAll);
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
/// artists and album, for search results and an artist's popular songs;
/// `page_artist`, the artist whose page it is, goes without saying.
#[allow(clippy::too_many_arguments)]
fn song(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    number: usize,
    track: &TrackRef,
    full: bool,
    page_artist: Option<&str>,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let number = format!("{number:>2}");
    let seconds = track.duration_ms / 1000;
    let duration = format!("{}:{:02}", seconds / 60, seconds % 60);
    let artists = track
        .artists
        .iter()
        .filter(|a| Some(a.uri.as_str()) != page_artist)
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
            if full && !artists.is_empty() {
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
    // A song of an album's comes from that album; a search's, alone.
    let added = || {
        let mut track = track.clone();
        if !full {
            track.source = Some(Source::album(&track.album));
        }
        vec![track]
    };
    if row.response.drag_started() {
        row.response
            .dnd_set_drag_payload(CatalogDrag::Tracks(added()));
    }
    if row.button || matches!(row.act, Some(Act::Add)) {
        actions.push(Action::AddTracks(added()));
    } else if row.response.double_clicked() || matches!(row.act, Some(Act::Open)) {
        actions.push(Action::PlayNow(track.clone()));
    }
    share(&row.response, "Copy song link", &track.uri, actions);
}

/// A right-click menu of only Share, for what has a Spotify link.
fn share(response: &Response, copy: &str, uri: &str, actions: &mut Actions) {
    if crate::spotify::link(uri).is_none() {
        return;
    }
    response.context_menu(|ui| {
        if share_menu(ui, copy) {
            actions.push(Desktop::CopyLinks(vec![uri.to_string()]));
        }
    });
}

fn slim_column(ui: &mut Ui, view: &View, nav: &mut Nav, actions: &mut Actions) {
    let palette = view.palette;
    let browse = view.browse;
    let Some(target) = view.list(browse.target) else {
        return;
    };
    // ↓ from the box is the box's: the songs don't take it too.
    let box_keyed = ui.horizontal(|ui| {
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
        let shown = egui::ComboBox::from_id_salt("add-target")
            .selected_text(&target.name)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                *ui.visuals_mut() = window;
                for (index, &id) in view.tabs.iter().enumerate() {
                    let Some(list) = view.list(id) else { continue };
                    let mut item = ui.selectable_label(list.id == target.id, &list.name);
                    if nav.pick == Some(index) {
                        item = item.highlight();
                        item.scroll_to_me(None);
                    }
                    if item.clicked() {
                        actions.push(Action::SetTarget(list.id));
                    }
                }
            });
        let response = shown.response.on_hover_text(
            "The playlist + ADD adds to (Alt+↓ to choose; Ctrl+Tab / Ctrl+Shift+Tab, \
             Ctrl+1 … 9)",
        );
        if nav.on_box && nav.area == Area::Slim && nav.keyboard {
            theme::focus_ring(ui.painter(), response.rect.expand(3.0), &palette);
        }
        box_keys(ui, view, target, nav, &response, actions)
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
        if !box_keyed.inner {
            slim_keys(ui, target, nav, actions);
        }
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
/// The ADD TO box's keys, while it is highlighted: Enter (or Alt+↓ / ↑)
/// opens its list; in the list, ↑ and ↓ highlight a playlist, Enter (or
/// Alt+↓ / ↑) chooses it and Esc closes it (egui's own). ↓ goes back to the songs. Says whether it
/// took a key.
fn box_keys(
    ui: &Ui,
    view: &View,
    target: &Playlist,
    nav: &mut Nav,
    response: &egui::Response,
    actions: &mut Actions,
) -> bool {
    let popup = egui::Popup::default_response_id(response);
    let open = egui::Popup::is_id_open(ui.ctx(), popup);
    if !open {
        nav.pick = None;
    }
    nav.box_open = open;
    if nav.area != Area::Slim || !(nav.on_box || open) || ui.ctx().text_edit_focused() {
        return false;
    }
    let (up, down, mut enter) = ui.input(|i| {
        let plain = i.modifiers.is_none();
        (
            plain && i.key_pressed(Key::ArrowUp),
            plain && i.key_pressed(Key::ArrowDown),
            plain && i.key_pressed(Key::Enter),
        )
    });
    enter |= std::mem::take(&mut nav.toggle_box);
    if !open {
        if enter {
            egui::Popup::open_id(ui.ctx(), popup);
            nav.pick = view.tabs.iter().position(|&id| id == target.id);
            nav.keyboard = true;
        } else if down {
            nav.on_box = false;
            nav.slim = target.entries().first().map(|entry| entry.id);
            nav.slim_reveal = nav.slim.map(|_| 0);
            nav.keyboard = true;
        }
        return up || down || enter;
    }
    let last = view.tabs.len().saturating_sub(1);
    let at = nav
        .pick
        .or_else(|| view.tabs.iter().position(|&id| id == target.id));
    if up || down {
        nav.pick = Some(match at {
            Some(at) if up => at.saturating_sub(1),
            Some(at) => (at + 1).min(last),
            None => 0,
        });
        nav.keyboard = true;
    }
    if enter && let Some(&id) = at.and_then(|at| view.tabs.get(at)) {
        actions.push(Action::SetTarget(id));
        egui::Popup::close_id(ui.ctx(), popup);
        nav.box_open = false;
        nav.pick = None;
    }
    up || down || enter
}

fn slim_keys(ui: &Ui, list: &Playlist, nav: &mut Nav, actions: &mut Actions) {
    nav.slim = nav.slim.filter(|&id| list.index_of(id).is_some());
    if nav.area != Area::Slim || nav.on_box || nav.box_open || ui.ctx().text_edit_focused() {
        return;
    }
    let from = nav.slim.or(list.cursor()).and_then(|id| list.index_of(id));
    // ↑ from the first song, or in an empty playlist, goes up to ADD TO.
    let to_box = ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::ArrowUp))
        && from.is_none_or(|at| at == 0);
    if to_box {
        nav.on_box = true;
        nav.slim = None;
        nav.keyboard = true;
        return;
    }
    if list.is_empty() {
        return;
    }
    let last = list.len() - 1;
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
                nav.on_box = false;
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
        CatalogDrag::Loved(songs) => Action::DropLoved(songs.clone(), index),
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

#[cfg(test)]
mod tests {
    use egui::{Event, RawInput};

    use super::*;

    /// Whether these events, with these keys held, press ADD ALL.
    fn presses(modifiers: Modifiers, events: Vec<Event>) -> bool {
        let ctx = egui::Context::default();
        let mut events = events;
        events.insert(0, Event::ModifiersChanged(modifiers));
        let raw = RawInput {
            events,
            ..RawInput::default()
        };
        let mut pressed = false;
        let _ = ctx.run_ui(raw, |ui| pressed = ui.input(add_all_pressed));
        pressed
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    #[test]
    fn shift_insert_is_add_all_as_a_key_or_as_windows_paste() {
        let paste = || Event::Paste("spotify:track:x".into());
        assert!(presses(
            Modifiers::SHIFT,
            vec![key(Key::Insert, Modifiers::SHIFT)]
        ));
        assert!(presses(Modifiers::SHIFT, vec![paste()]));
        // Ctrl+V's paste, and Insert alone, aren't.
        assert!(!presses(Modifiers::COMMAND, vec![paste()]));
        assert!(!presses(
            Modifiers::NONE,
            vec![key(Key::Insert, Modifiers::NONE)]
        ));
    }
}
