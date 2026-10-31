//! Albums and songs as the results show them, with their covers.

use egui::{Align2, Color32, Painter, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{
    Act, BIG_COVER, COVER_ROW_HEIGHT, CatalogDrag, Cover, Nav, ROW_HEIGHT, THUMB, TRACK_HEIGHT,
    body, chevron, indented_note, mono, row, spans, tag,
};
use crate::action::{Action, Actions, Desktop};
use crate::ui::View;
use crate::ui::widgets::share_menu;
use spotless_core::browse::{Find, Loadable};
use spotless_core::model::catalog::Release;
use spotless_core::model::{Source, TrackRef};

/// An album row that opens to show its songs. With a find, only an album
/// with it in its title or its songs' shows, with those songs open:
/// whether it showed.
pub(super) fn album(
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

/// Draws a cover or an artist's picture in `rect`, asking for it when it
/// comes into view; a framed square stands in until it arrives.
pub(super) fn cover(
    painter: &Painter,
    view: &View,
    url: Option<&String>,
    rect: Rect,
    actions: &mut Actions,
) {
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
pub(super) fn song(
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
pub(super) fn share(response: &Response, copy: &str, uri: &str, actions: &mut Actions) {
    if spotless_core::spotify::link(uri).is_none() {
        return;
    }
    response.context_menu(|ui| {
        if share_menu(ui, copy) {
            actions.push(Desktop::CopyLinks(vec![uri.to_string()]));
        }
    });
}
