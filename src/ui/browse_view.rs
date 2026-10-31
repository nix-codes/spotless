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
    Align2, Color32, FontId, Id, Painter, Rect, Response, RichText, ScrollArea, Sense, Ui,
    UiBuilder, WidgetInfo, WidgetType, pos2, vec2,
};

use super::scrollbar;
use super::settings_view::Category;
use super::theme::{self, Palette};
use super::widgets::{BUTTON_HEIGHT, SMALL_HEIGHT, small_button, small_button_width, text_button};
use super::{Inputs, View};
use crate::action::{Actions, BrowseAction};
use spotless_core::browse::{BrowseTab, Page};
use spotless_core::lastfm;
use spotless_core::model::{EntryId, TrackRef};

mod album;
mod artist;
mod keyboard;
mod lastfm_tab;
mod search;
mod slim;
mod spotify;

pub(super) use artist::{add_all_pressed, adds_all};

use artist::artist_page;
use keyboard::{enter_area, keyboard, page_id, switch_area};
use search::{find_bar, home, search_bar, search_page, searchable};
use slim::slim_column;

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
                actions.push(BrowseAction::ShowBrowseTab(tab));
                inputs.nav.area = match tab {
                    BrowseTab::Search | BrowseTab::LastFm => Area::Results,
                    BrowseTab::Spotify => Area::Lists,
                };
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if text_button(ui, &palette, "ESC · PLAYLISTS", false, true).clicked() {
                actions.push(BrowseAction::CloseBrowse);
            }
            if text_button(ui, &palette, "PICS", view.browse.show_covers, true)
                .on_hover_text("Show album covers and artist pictures (C)")
                .clicked()
            {
                actions.push(BrowseAction::ToggleCovers);
            }
        });
    });
    super::playlist_view::tab_baseline(ui, &palette, shown_rect, row.response.rect.bottom());
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
