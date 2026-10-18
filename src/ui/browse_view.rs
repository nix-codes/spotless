//! The catalog: search, results and artist pages on the left, and the slim
//! column with the playlist + ADD adds to on the right.
//!
//! Enter searches, Esc goes back to the playlists, Alt+Left goes back a
//! page, Ctrl+F jumps to the search field.

use egui::{
    Align2, Color32, FontId, Id, Key, Painter, Rect, Response, RichText, ScrollArea, Sense,
    TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::theme::{self, Palette};
use super::widgets::{BUTTON_HEIGHT, SMALL_HEIGHT, small_button, small_button_width, text_button};
use super::{Inputs, View};
use crate::action::Action;
use crate::backend::SearchState;
use crate::browse::{Loadable, Page};
use crate::model::catalog::{Release, ReleaseTab};
use crate::model::{TrackRef, playlist::Playlist};

const SLIM_WIDTH: f32 = 250.0;
const ROW_HEIGHT: f32 = 22.0;
const TRACK_HEIGHT: f32 = 20.0;
const PAD: f32 = 10.0;

pub fn draw(ui: &mut Ui, rect: Rect, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let slim_width = SLIM_WIDTH.min(rect.width() * 0.35);
    let catalog = Rect::from_min_max(
        rect.min,
        pos2(rect.right() - slim_width - PAD, rect.bottom()),
    );
    let slim = Rect::from_min_max(pos2(catalog.right() + PAD, rect.top()), rect.max);
    theme::inset(ui.painter(), catalog, palette.list_bg, &palette);
    theme::inset(ui.painter(), slim, palette.list_bg, &palette);

    keyboard(ui, view, inputs, actions);
    ui.scope_builder(UiBuilder::new().max_rect(catalog.shrink(PAD)), |ui| {
        search_bar(ui, view, inputs, actions);
        ui.add_space(PAD);
        let page = view.browse.page();
        ScrollArea::vertical()
            .id_salt(page_id(page))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 0.0);
                match page {
                    Page::Home => home(ui, view, actions),
                    Page::Search(query) => search_page(ui, view, query, actions),
                    Page::Artist(uri) => artist_page(ui, view, uri, actions),
                }
            });
    });
    ui.scope_builder(UiBuilder::new().max_rect(slim.shrink(8.0)), |ui| {
        slim_column(ui, view, actions);
    });
}

fn page_id(page: &Page) -> String {
    match page {
        Page::Home => "home".into(),
        Page::Search(query) => format!("search:{query}"),
        Page::Artist(uri) => uri.clone(),
    }
}

fn keyboard(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let typing = ui.ctx().memory(|m| m.focused()).is_some();
    ui.input(|i| {
        if i.modifiers.command && i.key_pressed(Key::F) {
            inputs.focus_search = true;
        }
        if typing {
            return;
        }
        if i.key_pressed(Key::Escape) {
            actions.push(Action::CloseBrowse);
        }
        let back =
            (i.modifiers.alt && i.key_pressed(Key::ArrowLeft)) || i.key_pressed(Key::Backspace);
        if back && view.browse.can_go_back() {
            actions.push(Action::Back);
        }
    });
}

fn search_bar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    ui.horizontal(|ui| {
        if text_button(ui, &palette, "BACK", false, view.browse.can_go_back()).clicked() {
            actions.push(Action::Back);
        }
        let close_width = 128.0;
        let field = ui.add(
            TextEdit::singleline(&mut inputs.query)
                .id(Id::new("catalog-search"))
                .hint_text("Search artists, albums, songs…")
                .desired_width((ui.available_width() - close_width).max(80.0))
                .min_size(vec2(0.0, BUTTON_HEIGHT)),
        );
        if std::mem::take(&mut inputs.focus_search) {
            field.request_focus();
        }
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            actions.push(Action::Search(inputs.query.clone()));
        }
        if text_button(ui, &palette, "ESC · PLAYLISTS", false, true).clicked() {
            actions.push(Action::CloseBrowse);
        }
    });
}

fn home(ui: &mut Ui, view: &View, actions: &mut Vec<Action>) {
    if !search_prompt(ui, view) {
        note(ui, view, "Type an artist, album or song and press Enter.");
    }
    if !view.browse.recent.is_empty() {
        section(ui, view, "RECENTLY VIEWED");
        for artist in &view.browse.recent {
            let row = row(ui, &view.palette, ROW_HEIGHT, None, |painter, rect| {
                spans(
                    painter,
                    rect,
                    &[(&artist.name, body(), view.palette.list_fg)],
                );
            });
            if row.response.clicked() {
                actions.push(Action::OpenArtist(artist.clone()));
            }
        }
    }
}

/// Explains what stands in the way of search. False when nothing does.
fn search_prompt(ui: &mut Ui, view: &View) -> bool {
    match view.search {
        SearchState::Ready | SearchState::SignedOut => return false,
        SearchState::NoApp => note(
            ui,
            view,
            "Search needs your personal Spotify app. Add \"web_client_id\": \"<its Client ID>\" \
             to ~/.config/spotless/settings.json and restart Spotless.",
        ),
        SearchState::SigningIn => note(
            ui,
            view,
            "Spotify is confirming search in your browser. If it asks, approve Spotless \
             (with spotifast closed: both use the same sign-in port).",
        ),
    }
    true
}

fn search_page(ui: &mut Ui, view: &View, query: &str, actions: &mut Vec<Action>) {
    if search_prompt(ui, view) {
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
    let palette = view.palette;
    if !results.artists.is_empty() {
        section(ui, view, "ARTISTS");
        for artist in &results.artists {
            let row = row(ui, &palette, ROW_HEIGHT, None, |painter, rect| {
                spans(
                    painter,
                    rect,
                    &[
                        (&artist.name, body(), palette.list_fg),
                        ("ARTIST", tag(), palette.dim),
                    ],
                );
            });
            if row.response.clicked() {
                actions.push(Action::OpenArtist(artist.clone()));
            }
        }
    }
    if !results.albums.is_empty() {
        section(ui, view, "ALBUMS & EPS");
        for release in &results.albums {
            album(ui, view, release, true, actions);
        }
    }
    if !results.songs.is_empty() {
        section(ui, view, "SONGS");
        for (i, track) in results.songs.iter().enumerate() {
            song(ui, view, i + 1, track, true, actions);
        }
    }
}

fn artist_page(ui: &mut Ui, view: &View, uri: &str, actions: &mut Vec<Action>) {
    let discography = match view.browse.artists.get(uri) {
        None | Some(Loadable::Loading) => return note(ui, view, "Reading the discography…"),
        Some(Loadable::Failed(error)) => return note(ui, view, error),
        Some(Loadable::Ready(discography)) => discography,
    };
    let palette = view.palette;
    let browse = view.browse;
    ui.horizontal(|ui| {
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
        for tab in ReleaseTab::ALL {
            let count = discography
                .releases
                .iter()
                .filter(|r| tab.includes(r.group))
                .count();
            let label = format!("{} {count}", tab.label());
            if small_button(ui, &palette, &label, tab == browse.tab).clicked() {
                actions.push(Action::SetTab(tab));
            }
        }
        ui.add_space(12.0);
        let sort = if browse.newest_first {
            "NEWEST FIRST"
        } else {
            "OLDEST FIRST"
        };
        if small_button(ui, &palette, sort, false).clicked() {
            actions.push(Action::ToggleSort);
        }
        let all_open =
            !releases.is_empty() && releases.iter().all(|r| browse.expanded.contains(&r.uri));
        let label = if all_open {
            "HIDE SONGS"
        } else {
            "SHOW ALL SONGS"
        };
        if small_button(ui, &palette, label, false).clicked() {
            actions.push(Action::ExpandAlbums(if all_open {
                Vec::new()
            } else {
                releases.iter().map(|r| r.uri.clone()).collect()
            }));
        }
    });
    ui.add_space(8.0);
    if releases.is_empty() {
        note(ui, view, "Nothing here.");
    }
    for release in releases {
        album(
            ui,
            view,
            release,
            browse.tab == ReleaseTab::AppearsOn,
            actions,
        );
    }
}

/// An album row that opens to show its songs.
fn album(ui: &mut Ui, view: &View, release: &Release, by: bool, actions: &mut Vec<Action>) {
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
    let row = row(ui, &palette, ROW_HEIGHT, Some("+ ADD"), |painter, rect| {
        chevron(
            painter,
            pos2(rect.left() + 5.0, rect.center().y),
            expanded,
            palette.dim,
        );
        let text = Rect::from_min_max(pos2(rect.left() + 16.0, rect.top()), rect.max);
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
    if row.button {
        actions.push(Action::AddAlbum(release.uri.clone()));
    } else if row.response.clicked() {
        actions.push(Action::ToggleAlbum(release.uri.clone()));
    }
    if !expanded {
        return;
    }
    match view.browse.albums.get(&release.uri) {
        None | Some(Loadable::Loading) => indented_note(ui, view, "Reading the songs…"),
        Some(Loadable::Failed(error)) => indented_note(ui, view, error),
        Some(Loadable::Ready(tracks)) => {
            for (i, track) in tracks.iter().enumerate() {
                song(ui, view, i + 1, track, false, actions);
            }
            ui.add_space(4.0);
        }
    }
}

/// A song row: double-click plays it now, + adds it. `full` shows the
/// artist and album, for search results.
fn song(
    ui: &mut Ui,
    view: &View,
    number: usize,
    track: &TrackRef,
    full: bool,
    actions: &mut Vec<Action>,
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
    let row = row(ui, &palette, TRACK_HEIGHT, Some("+"), |painter, rect| {
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
    });
    if row.button {
        actions.push(Action::AddTracks(vec![track.clone()]));
    } else if row.response.double_clicked() {
        actions.push(Action::PlayNow(track.clone()));
    }
}

fn slim_column(ui: &mut Ui, view: &View, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let browse = view.browse;
    let Some(target) = view.library.get(browse.target) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("ADD TO")
                .monospace()
                .size(10.0)
                .color(palette.chrome),
        );
        egui::ComboBox::from_id_salt("add-target")
            .selected_text(&target.name)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for list in view.library.playlists() {
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
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        slim_rows(ui, view, target);
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

fn slim_rows(ui: &mut Ui, view: &View, list: &Playlist) {
    let palette = view.palette;
    if list.is_empty() {
        ui.label(RichText::new("Empty. Use + ADD on the left.").color(palette.dim));
        return;
    }
    let height = 17.0;
    let mut area = ScrollArea::vertical().auto_shrink([false, false]);
    // Bring the songs just added into view.
    if let Some(last) = list
        .entries()
        .iter()
        .rposition(|entry| view.browse.glow(entry.id) > 0.9)
    {
        let view_height = ui.available_height();
        area = area.vertical_scroll_offset(((last + 1) as f32 * height - view_height).max(0.0));
    }
    let current = list.cursor().filter(|_| !list.detached());
    area.show_rows(ui, height, list.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for index in range {
            let entry = &list.entries()[index];
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
            let painter = ui.painter_at(rect);
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
}

/// A full-width clickable row, painted by `paint` left of an optional
/// button at its right end.
fn row(
    ui: &mut Ui,
    palette: &Palette,
    height: f32,
    button: Option<&str>,
    paint: impl FnOnce(&Painter, Rect),
) -> Row {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if response.hovered() {
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
        small_button(&mut child, palette, label, false).clicked()
    });
    Row {
        response,
        button: clicked,
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
