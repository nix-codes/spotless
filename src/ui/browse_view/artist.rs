//! An artist's page: the releases, popular songs and similar artists,
//! and ADD ALL.

use egui::{InputState, Key, Modifiers, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};

use super::album::{album, cover, share, song};
use super::{
    Act, COVER_ROW_HEIGHT, Nav, PORTRAIT, ROW_HEIGHT, THUMB, body, note, row, shown, spans, tag,
};
use crate::action::{Actions, BrowseAction};
use crate::ui::View;
use crate::ui::widgets::{SMALL_HEIGHT, small_action, small_button, small_button_width};
use spotless_core::browse::{AddAll, BrowseTab, Find, Loadable, Page};
use spotless_core::lastfm;
use spotless_core::model::ArtistRef;
use spotless_core::model::catalog::{Discography, ReleaseTab};

pub(super) fn artist_page(
    ui: &mut Ui,
    view: &View,
    nav: &mut Nav,
    uri: &str,
    actions: &mut Actions,
) {
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
        lastfm::is_key(&view.lastfm.api_key) || browse.similar_artists.contains_key(uri);
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
                actions.push(BrowseAction::SetTab(tab));
            }
        }
        if similar_tab
            && small_button(ui, &palette, "SIMILAR", similar)
                .on_hover_text("Artists Last.fm's listeners play alongside this one (Alt+5)")
                .clicked()
        {
            actions.push(BrowseAction::ShowSimilar(discography.artist.clone()));
        }
        if !discography.popular.is_empty()
            && small_button(ui, &palette, "POPULAR", popular)
                .on_hover_text("Its most played songs, as Spotify ranks them (Alt+6)")
                .clicked()
        {
            actions.push(BrowseAction::ShowPopular);
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
            actions.push(BrowseAction::ToggleSort);
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
            actions.push(BrowseAction::ExpandAlbums(if all_open {
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
pub(super) fn artist_row(
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
        actions.push(BrowseAction::OpenArtist(artist.clone()));
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
            actions.push(BrowseAction::OpenSimilar(name.clone()));
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
pub(super) fn page_artist(view: &View) -> Option<ArtistRef> {
    let Page::Artist(uri) = view.browse.page() else {
        return None;
    };
    match view.browse.artists.get(uri) {
        Some(Loadable::Ready(discography)) => Some(discography.artist.clone()),
        _ => None,
    }
}

/// Shift+Insert, ADD ALL's key on an artist page. On Windows it arrives as
/// a paste, of whatever the clipboard holds, with Shift held (Ctrl+V's has
/// Ctrl).
pub(crate) fn add_all_pressed(i: &InputState) -> bool {
    let shift = i.modifiers.matches_exact(Modifiers::SHIFT);
    shift
        && (i.key_pressed(Key::Insert)
            || i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))))
}

/// An artist's page shows on the search tab: Shift+Insert is ADD ALL's.
pub(crate) fn adds_all(view: &View) -> bool {
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
    if crate::ui::widgets::tip(button, &tip).clicked() || (pressed && !reading) {
        actions.push(BrowseAction::AddAll);
    }
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
