//! The worker's part in reading Spotify's catalogue: links, search (with
//! its own approval), artists, albums and covers, and similar artists
//! from Last.fm.

use super::Worker;
use crate::backend::{Command, Event};
use crate::backend::{Emit, SearchState};
use crate::browser::Browser;
use crate::lastfm;
use crate::spotify::{catalog, code};
use crate::spotify::webapi::WebApi;
use std::sync::Arc;

impl Worker {
    /// Opens a Spotify Code: its number from `reading` (a picture read,
    /// which takes a moment for a big one, or a number already read), then
    /// one lookup, never retried, and what it points to.
    fn open_code(&self, id: u64, reading: impl FnOnce() -> anyhow::Result<u64> + Send + 'static) {
        let Some(session) = self.session.clone() else {
            let error = "Sign in to Spotify to read a Spotify Code.".to_string();
            return (self.emit)(Event::CodeRead(id, Err(error)));
        };
        let http = self.http.clone();
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            let result = async {
                let reference = tokio::task::spawn_blocking(reading).await??;
                let found = code::look_up(&session, &http, reference).await?;
                let target = code::target(&session, &found).await?;
                Ok((reference, target))
            }
            .await
            .map_err(|e: anyhow::Error| format!("{e:#}"));
            emit(Event::CodeRead(id, result));
        });
    }

    /// Reads links, searches, artists, albums and covers.
    pub(super) fn handle_catalog(&mut self, command: Command) {
        match command {
            Command::AddLink { text, playlist } => {
                self.on_session(move |session| async move {
                    match catalog::link_tracks(&session, &text).await {
                        Ok(tracks) => Event::Tracks { playlist, tracks },
                        Err(error) => Event::Error(format!("{error:#}")),
                    }
                });
            }
            Command::SearchApp(client_id) => {
                // The saved approval belongs to the old app.
                let _ = std::fs::remove_file(self.paths.search_credentials());
                self.set_search_app(client_id);
                // Signed in already: approve now, as part of setting it up.
                if self.session.is_some() {
                    self.approve_search_now();
                }
            }
            Command::Search(query) => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    // A saved approval Spotify no longer takes is dropped by
                    // the search; approve again and retry once.
                    let mut results = Err(String::new());
                    for _ in 0..2 {
                        if let Err(error) = approve_search(&web, &approval, &browser, &emit).await {
                            results = Err(error);
                            break;
                        }
                        let mut web = web.lock().await;
                        results = web.search(&query).await.map_err(|e| format!("{e:#}"));
                        if results.is_ok() || web.signed_in() {
                            break;
                        }
                        emit(Event::Search(SearchState::SignedOut));
                    }
                    emit(Event::SearchResults { query, results });
                });
            }
            Command::MoreSongs { query, offset } => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match approve_search(&web, &approval, &browser, &emit).await {
                        Ok(()) => web
                            .lock()
                            .await
                            .search_songs(&query, offset)
                            .await
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error),
                    };
                    emit(Event::MoreSongs { query, result });
                });
            }
            Command::MoreArtists { query, offset } => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match approve_search(&web, &approval, &browser, &emit).await {
                        Ok(()) => web
                            .lock()
                            .await
                            .search_artists(&query, offset)
                            .await
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error),
                    };
                    emit(Event::MoreArtists { query, result });
                });
            }
            Command::Discography(artist) => {
                self.on_session(move |session| async move {
                    let result = match catalog::parse_uri(&artist) {
                        Ok(uri) => catalog::discography(&session, &uri).await,
                        Err(error) => Err(error),
                    };
                    let result = result.map_err(|e| format!("{e:#}"));
                    Event::Discography { artist, result }
                });
            }
            Command::SimilarArtists { artist, api_key } => {
                let http = self.http.clone();
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = lastfm::api::similar_artists(&http, &api_key, &artist.name)
                        .await
                        .map_err(|e| e.to_string());
                    emit(Event::SimilarArtists {
                        artist: artist.uri,
                        result,
                    });
                });
            }
            Command::FindArtist(name) => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match approve_search(&web, &approval, &browser, &emit).await {
                        Ok(()) => web
                            .lock()
                            .await
                            .find_artist(&name)
                            .await
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error),
                    };
                    emit(Event::ArtistFound { name, result });
                });
            }
            Command::FindSongs { id, songs } => {
                let Some(web) = self.web.clone() else {
                    let result = Err("Search isn't set up.".into());
                    return (self.emit)(Event::SongsFound { id, result });
                };
                let found = Arc::clone(&self.found);
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match approve_search(&web, &approval, &browser, &emit).await {
                        Ok(()) => lastfm::found::find(&web, &found, &songs)
                            .await
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error),
                    };
                    emit(Event::SongsFound { id, result });
                });
            }
            Command::Cover(url) => {
                let http = self.http.clone();
                let dir = self.paths.covers();
                let slots = Arc::clone(&self.cover_slots);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let _slot = slots.acquire().await;
                    let result = crate::images::cover(&http, &dir, &url)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    if let Err(error) = &result {
                        log::warn!("cover {url}: {error}");
                    }
                    emit(Event::Cover { url, result });
                });
            }
            Command::ReadCode(id, image) => self.open_code(id, move || code::read(&image)),
            Command::LookUpCode(id, reference) => self.open_code(id, move || Ok(reference)),
            Command::ReadLink(id, link) => {
                let reading = self.on_session(move |session| async move {
                    let result = code::target(&session, &link).await;
                    Event::LinkRead(id, result.map_err(|e| format!("{e:#}")))
                });
                if !reading {
                    let error = "Sign in to Spotify to open a Spotify link.".to_string();
                    (self.emit)(Event::LinkRead(id, Err(error)));
                }
            }
            Command::Album(album) => {
                self.on_session(move |session| async move {
                    let result = catalog::songs(&session, &album)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    Event::Album { album, result }
                });
            }
            _ => {}
        }
    }

    /// Search signs in with this personal app from now on.
    pub(super) fn set_search_app(&mut self, client_id: Option<String>) {
        let web = client_id.map(|id| WebApi::new(id, self.paths.search_credentials()));
        (self.emit)(Event::Search(match &web {
            None => SearchState::NoApp,
            Some(web) if web.signed_in() => SearchState::Ready,
            Some(_) => SearchState::SignedOut,
        }));
        self.web = web.map(|web| Arc::new(tokio::sync::Mutex::new(web)));
    }

    /// Runs search's browser approval now, unless it was given already.
    pub(super) fn approve_search_now(&self) {
        let Some(web) = self.web.clone() else { return };
        let approval = Arc::clone(&self.search_approval);
        let browser = Arc::clone(&self.browser);
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            if let Err(error) = approve_search(&web, &approval, &browser, &emit).await {
                emit(Event::Error(error));
            }
        });
    }
}

/// Search signs in with the personal app, a second approval besides
/// playback's (see `spotify/webapi.rs`). It runs when Spotless signs in, or
/// on the first search if search was never approved. The browser is already
/// signed in to Spotify by then, so it usually passes without a question.
/// One at a time: a second caller waits, then finds it done.
async fn approve_search(
    web: &tokio::sync::Mutex<WebApi>,
    approval: &tokio::sync::Mutex<()>,
    browser: &Browser,
    emit: &Emit,
) -> Result<(), String> {
    let _one = approval.lock().await;
    if web.lock().await.signed_in() {
        return Ok(());
    }
    emit(Event::Search(SearchState::SigningIn));
    // Not holding the client: the approval waits as long as the user takes.
    let client = web
        .lock()
        .await
        .approval(browser)
        .map_err(|e| format!("{e:#}"));
    let result = match client {
        Ok(client) => match client.get_access_token_async().await {
            Ok(token) => web.lock().await.keep(token).map_err(|e| format!("{e:#}")),
            Err(error) => Err(format!("search sign-in failed: {error}")),
        },
        Err(error) => Err(error),
    };
    emit(Event::Search(if result.is_ok() {
        SearchState::Ready
    } else {
        SearchState::SignedOut
    }));
    result
}
