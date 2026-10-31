//! The controller's part in Browse: search, artists' pages and their
//! albums, and the songs added from them to the target playlist.

use super::Controller;
use crate::action::Action;
use crate::backend::{Command, SearchState};
use crate::browse::{AddAll, Loadable, Page};
use crate::model::catalog::{Discography, SearchResults, Songs};
use crate::model::playlist::from_source;
use crate::model::{ArtistRef, PlaylistId, Source, TrackRef};
use crate::spotify::webapi::SongsPage;

impl Controller {
    /// Opens Browse and moves through it, and adds what it shows.
    pub(super) fn apply_browse(&mut self, action: Action) {
        let browse = &mut self.state.browse;
        match action {
            Action::ToggleBrowse => {
                browse.open = !browse.open;
                if browse.open {
                    browse.target = self.state.viewed;
                    self.read_spotify_playlists();
                }
            }
            Action::CloseBrowse => browse.open = false,
            Action::Back => browse.back(),
            Action::Search(query) => {
                let query = query.trim().to_string();
                if query.is_empty() {
                    return;
                }
                // A pasted Spotify link adds its songs instead.
                if crate::spotify::catalog::parse_uri(&query).is_ok() {
                    return self.apply(Action::AddLink(query));
                }
                browse.go(Page::Search(query.clone()));
                if self.state.search == SearchState::NoApp {
                    return;
                }
                browse.searched(&query);
                let cached = matches!(browse.searches.get(&query), Some(Loadable::Ready(_)));
                // Unapproved search asks for its approval first, by itself.
                if !cached {
                    browse.searches.insert(query.clone(), Loadable::Loading);
                    self.send(Command::Search(query));
                }
                self.mark_unsaved();
            }
            Action::MoreSongs(query) => {
                if let Some(Loadable::Ready(results)) = browse.searches.get_mut(&query)
                    && results.more_songs
                    && !results.reading_songs
                {
                    results.reading_songs = true;
                    let offset = results.songs_read;
                    self.send(Command::MoreSongs { query, offset });
                }
            }
            Action::ForgetSearch(query) => {
                match query {
                    Some(query) => browse.forget_search(&query),
                    None => browse.recent_searches.clear(),
                }
                self.mark_unsaved();
            }
            Action::OpenArtist(artist) => {
                browse.go(Page::Artist(artist.uri.clone()));
                match browse.artists.get(&artist.uri) {
                    Some(Loadable::Ready(_) | Loadable::Loading) => {
                        browse.visited(artist);
                        self.mark_unsaved();
                    }
                    _ => {
                        browse.artists.insert(artist.uri.clone(), Loadable::Loading);
                        self.send(Command::Discography(artist.uri));
                    }
                }
            }
            Action::ToggleAlbum(album) => {
                if !browse.expanded.remove(&album) {
                    browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::ExpandAlbums(albums) => {
                if albums.is_empty() {
                    browse.expanded.clear();
                }
                for album in albums {
                    self.state.browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::SetTab(tab) => {
                browse.tab = tab;
                browse.similar = false;
                browse.popular = false;
                self.read_for_find();
            }
            Action::ShowPopular => {
                browse.popular = true;
                browse.similar = false;
            }
            Action::Find(find) => {
                browse.find = find;
                self.read_for_find();
            }
            Action::ShowSimilar(artist) => {
                browse.similar = true;
                browse.popular = false;
                if matches!(
                    browse.similar_artists.get(&artist.uri),
                    Some(Loadable::Ready(_) | Loadable::Loading)
                ) {
                    return;
                }
                let api_key = self.state.lastfm.api_key.clone();
                if !crate::lastfm::api::is_key(&api_key) {
                    browse.similar_artists.insert(
                        artist.uri,
                        Loadable::Failed("Similar artists come from Last.fm: add its API key in Settings, Last.fm.".into()),
                    );
                    return;
                }
                browse
                    .similar_artists
                    .insert(artist.uri.clone(), Loadable::Loading);
                self.send(Command::SimilarArtists { artist, api_key });
            }
            Action::OpenSimilar(name) => {
                // Without search, the search page says how to set it up.
                if self.state.search == SearchState::NoApp {
                    return self.apply(Action::Search(name));
                }
                if browse.finding.as_ref().is_some_and(|(n, _)| *n == name) {
                    return;
                }
                browse.finding = Some((name.clone(), browse.page().clone()));
                self.send(Command::FindArtist(name));
            }
            Action::ToggleSort => browse.newest_first = !browse.newest_first,
            Action::SetTarget(list) => browse.target = list,
            Action::ToggleCovers => {
                browse.show_covers = !browse.show_covers;
                self.state.settings.covers = browse.show_covers;
                self.mark_unsaved();
            }
            Action::AddTracks(tracks) => {
                self.insert_tracks(self.state.browse.target, None, tracks);
            }
            Action::DropTracks(tracks, at) => {
                self.insert_tracks(self.state.browse.target, Some(at), tracks);
            }
            Action::AddAlbum(album) => self.add_album(album, None),
            Action::AddAll => match self.state.browse.add_all() {
                Some(AddAll::Releases(albums)) => self.apply(Action::AddAlbums(albums)),
                Some(AddAll::Songs(songs)) => self.apply(Action::AddTracks(songs)),
                Some(AddAll::Reading) | None => {}
            },
            Action::AddAlbums(albums) => {
                let target = self.state.browse.target;
                for album in &albums {
                    self.read_album(album.clone());
                }
                self.pending_batches.push((albums, target));
                self.add_batches_read();
            }
            Action::DropAlbum(album, at) => self.add_album(album, Some(at)),
            Action::ShowBrowseTab(tab) => {
                let browse = &mut self.state.browse;
                browse.shown = tab;
                if !browse.open {
                    browse.open = true;
                    browse.target = self.state.viewed;
                    self.read_spotify_playlists();
                }
                self.mark_unsaved();
            }
            _ => {}
        }
    }

    pub(super) fn search_results(&mut self, query: String, results: Result<SearchResults, String>) {
        let loaded = match results {
            Ok(results) => Loadable::Ready(results),
            Err(error) => Loadable::Failed(error),
        };
        self.state.browse.searches.insert(query, loaded);
    }

    pub(super) fn more_songs_read(&mut self, query: String, result: Result<SongsPage, String>) {
        let Some(Loadable::Ready(results)) = self.state.browse.searches.get_mut(&query) else {
            return;
        };
        results.reading_songs = false;
        match result {
            Ok(page) => {
                // Spotify's pages can overlap: a song shows once.
                for song in page.songs {
                    if !results.songs.iter().any(|s| s.uri == song.uri) {
                        results.songs.push(song);
                    }
                }
                results.songs_read += page.read;
                results.more_songs = page.more;
            }
            Err(error) => self.state.error = Some(error),
        }
    }

    pub(super) fn discography_read(&mut self, artist: String, result: Result<Discography, String>) {
        let loaded = match result {
            Ok(discography) => {
                self.state.browse.visited(discography.artist.clone());
                self.mark_unsaved();
                Loadable::Ready(discography)
            }
            Err(error) => Loadable::Failed(error),
        };
        self.state.browse.artists.insert(artist, loaded);
        self.read_for_find();
    }

    pub(super) fn similar_artists_read(
        &mut self,
        artist: String,
        result: Result<Vec<String>, String>,
    ) {
        let loaded = match result {
            Ok(names) => Loadable::Ready(names),
            Err(error) => Loadable::Failed(error),
        };
        self.state.browse.similar_artists.insert(artist, loaded);
    }

    /// An album's or a Spotify playlist's songs read: those waiting for
    /// them go to their playlists.
    pub(super) fn album_read(&mut self, album: String, result: Result<Songs, String>) {
        let pending: Vec<(PlaylistId, Option<usize>)> = self
            .pending_adds
            .iter()
            .filter(|(uri, _, _)| *uri == album)
            .map(|(_, list, at)| (*list, *at))
            .collect();
        self.pending_adds.retain(|(uri, _, _)| *uri != album);
        match result {
            Ok(songs) => {
                for (list, at) in pending {
                    let tracks = self.added_from(&album, songs.tracks.clone());
                    self.insert_tracks(list, at, tracks);
                }
                let browse = &mut self.state.browse;
                if songs.places.is_empty() {
                    browse.places.remove(&album);
                } else {
                    browse
                        .places
                        .insert(album.clone(), (songs.revision, songs.places));
                }
                browse.albums.insert(album, Loadable::Ready(songs.tracks));
            }
            Err(error) => {
                if !pending.is_empty() {
                    self.state.error = Some(error.clone());
                }
                self.state
                    .browse
                    .albums
                    .insert(album, Loadable::Failed(error));
            }
        }
        self.add_batches_read();
    }

    /// Opens the similar artist found, unless the user has gone elsewhere
    /// since. Spotify knowing no artist by that name searches for it.
    pub(super) fn artist_found(&mut self, name: String, result: Result<Option<ArtistRef>, String>) {
        let browse = &mut self.state.browse;
        match browse.finding.take() {
            Some((wanted, from)) if wanted == name => {
                if *browse.page() != from {
                    return;
                }
            }
            other => {
                browse.finding = other;
                return;
            }
        }
        match result {
            Ok(Some(artist)) => self.apply(Action::OpenArtist(artist)),
            Ok(None) => self.apply(Action::Search(name)),
            Err(error) => self.state.error = Some(error),
        }
    }

    fn add_album(&mut self, album: String, at: Option<usize>) {
        let target = self.state.browse.target;
        match self.state.browse.albums.get(&album) {
            Some(Loadable::Ready(tracks)) => {
                let tracks = self.added_from(&album, tracks.clone());
                self.insert_tracks(target, at, tracks);
            }
            _ => {
                self.pending_adds.push((album.clone(), target, at));
                self.state.notice = Some("Reading the songs…".into());
                self.read_album(album);
            }
        }
    }

    /// Adds the batches of albums whose songs are all read now (those that
    /// couldn't be, left out), each in its order.
    fn add_batches_read(&mut self) {
        let albums = &self.state.browse.albums;
        let (read, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_batches)
            .into_iter()
            .partition(|(batch, _)| {
                batch
                    .iter()
                    .all(|album| !matches!(albums.get(album), None | Some(Loadable::Loading)))
            });
        self.pending_batches = waiting;
        for (batch, list) in read {
            let mut tracks = Vec::new();
            let mut unread = 0;
            for album in batch {
                match self.state.browse.albums.get(&album) {
                    Some(Loadable::Ready(songs)) => {
                        tracks.extend(self.added_from(&album, songs.clone()));
                    }
                    _ => unread += 1,
                }
            }
            if !tracks.is_empty() {
                self.insert_tracks(list, None, tracks);
            }
            if unread > 0 {
                self.state.error = Some(match unread {
                    1 => "One of the releases couldn't be read.".into(),
                    n => format!("{n} of the releases couldn't be read."),
                });
            }
        }
        if !self.pending_batches.is_empty() {
            self.state.notice = Some("Reading the songs…".into());
        }
    }

    /// An album's or a Spotify playlist's songs, as added from it.
    fn added_from(&self, uri: &str, tracks: Vec<TrackRef>) -> Vec<TrackRef> {
        let name = match self.state.browse.spotify_playlist(uri) {
            Some(playlist) => playlist.name.clone(),
            None => match tracks.first() {
                Some(track) if track.album.uri == uri => track.album.name.clone(),
                _ => return tracks,
            },
        };
        let source = Source {
            uri: uri.to_string(),
            name,
        };
        from_source(tracks, &source)
    }

    /// Reads an album's songs unless they are read or being read.
    /// With the find open on an artist's page, its tab's releases' songs
    /// are read, for the find to look through.
    fn read_for_find(&mut self) {
        let browse = &self.state.browse;
        let Page::Artist(uri) = browse.page() else {
            return;
        };
        let Some(Loadable::Ready(discography)) = browse.artists.get(uri) else {
            return;
        };
        // Popular songs are found among themselves.
        if browse.find.is_none() || browse.popular {
            return;
        }
        let albums: Vec<String> = discography
            .releases
            .iter()
            .filter(|release| browse.tab.includes(release.group))
            .map(|release| release.uri.clone())
            .collect();
        for album in albums {
            self.read_album(album);
        }
    }

    pub(super) fn read_album(&mut self, album: String) {
        if matches!(
            self.state.browse.albums.get(&album),
            Some(Loadable::Ready(_) | Loadable::Loading)
        ) {
            return;
        }
        self.state
            .browse
            .albums
            .insert(album.clone(), Loadable::Loading);
        self.send(Command::Album(album));
    }
}
