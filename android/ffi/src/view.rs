//! What the screens show, read off the controller's state as plain records.

use spotless_core::backend::{self, SessionState};
use spotless_core::browse::{self as core_browse, Find, Loadable, Page};
use spotless_core::controller::{State, SyncStatus};
use spotless_core::lastfm::{self, LastFmStatus, LovedSong as CoreLovedSong};
use spotless_core::model::catalog::{self, ReleaseGroup as CoreGroup, ReleaseTab as CoreTab};
use spotless_core::model::playlist::from_source;
use spotless_core::model::{
    AlbumRef, ArtistRef, Grouping, Playlist, PlaylistId, Source as CoreSource, TrackRef,
};
use spotless_core::phosphor::Phosphor;
use spotless_core::playback::engine;
use spotless_core::spotify::queue::SpotifyQueue;
use spotless_core::store::AUDIO_CACHE_SIZES;

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Artist {
    pub uri: String,
    pub name: String,
    /// The artist's picture's URL, where the catalogue gave one.
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Album {
    pub uri: String,
    pub name: String,
}

/// Where songs were added from: an album, a Spotify playlist, Liked
/// Songs or an artist, by URI, with its name.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Source {
    pub uri: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Track {
    pub uri: String,
    pub title: String,
    pub artists: Vec<Artist>,
    pub album: Album,
    pub duration_ms: u32,
    /// Where it was added from, kept when it's copied or moved; None for
    /// a song added on its own, as from a search.
    pub source: Option<Source>,
}

impl From<&ArtistRef> for Artist {
    fn from(artist: &ArtistRef) -> Self {
        Self {
            uri: artist.uri.clone(),
            name: artist.name.clone(),
            image: artist.image.clone(),
        }
    }
}

impl From<Artist> for ArtistRef {
    fn from(artist: Artist) -> Self {
        Self {
            uri: artist.uri,
            name: artist.name,
            image: artist.image,
        }
    }
}

impl From<&TrackRef> for Track {
    fn from(track: &TrackRef) -> Self {
        Self {
            uri: track.uri.clone(),
            title: track.title.clone(),
            artists: track.artists.iter().map(Into::into).collect(),
            album: Album {
                uri: track.album.uri.clone(),
                name: track.album.name.clone(),
            },
            duration_ms: track.duration_ms,
            source: track.source.as_ref().map(|source| Source {
                uri: source.uri.clone(),
                name: source.name.clone(),
            }),
        }
    }
}

impl From<Track> for TrackRef {
    fn from(track: Track) -> Self {
        Self {
            uri: track.uri,
            title: track.title,
            artists: track.artists.into_iter().map(Into::into).collect(),
            album: AlbumRef {
                uri: track.album.uri,
                name: track.album.name,
            },
            duration_ms: track.duration_ms,
            source: track.source.map(|source| CoreSource {
                uri: source.uri,
                name: source.name,
            }),
        }
    }
}

fn tracks(tracks: &[TrackRef]) -> Vec<Track> {
    tracks.iter().map(Into::into).collect()
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum Session {
    Connecting,
    SignedOut,
    /// Waiting for the browser approval.
    SigningIn,
    Ready {
        user: String,
    },
    Failed {
        message: String,
    },
}

pub(crate) fn session(state: &State) -> Session {
    match &state.session {
        SessionState::Connecting => Session::Connecting,
        SessionState::SignedOut => Session::SignedOut,
        SessionState::SigningIn => Session::SigningIn,
        SessionState::Ready { user } => Session::Ready { user: user.clone() },
        SessionState::Failed(message) => Session::Failed {
            message: message.clone(),
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Status {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NowPlaying {
    pub uri: String,
    pub title: String,
    pub artists: String,
    /// The first artist, as Last.fm knows the song by.
    pub main_artist: String,
    pub album: String,
    pub duration_ms: u32,
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Player {
    pub status: Status,
    pub track: Option<NowPlaying>,
    pub position_ms: u32,
    pub shuffle: bool,
    pub repeat: bool,
    /// Playback stops once the playing song ends.
    pub stop_after: bool,
    /// 0–100.
    pub volume: u8,
    /// The playlist the song shown is from, loaded or not.
    pub playlist: Option<u64>,
    /// Paused for want of a connection to Spotify; it goes on when there
    /// is one again.
    pub offline: bool,
    /// While Last.fm is connected and a song is shown, not loading:
    /// whether it's loved there, for the display's heart.
    pub loved: Option<bool>,
    /// The sound comes from this phone: a song is loaded, and it's not
    /// the Spotify tab's queue playing on another device. A song shown
    /// paused with nothing loaded (as after a restart) isn't.
    pub here: bool,
}

pub(crate) fn player(state: &State) -> Player {
    let snapshot = &state.now_playing();
    Player {
        status: match snapshot.status {
            engine::Status::Stopped => Status::Stopped,
            engine::Status::Loading => Status::Loading,
            engine::Status::Playing => Status::Playing,
            engine::Status::Paused => Status::Paused,
        },
        track: snapshot.track.as_ref().map(|track| NowPlaying {
            uri: track.uri.clone(),
            title: track.title.clone(),
            artists: track.artists.clone(),
            main_artist: track.main_artist.clone(),
            album: track.album.clone(),
            duration_ms: track.duration_ms,
            cover_url: track.cover_url.clone(),
        }),
        position_ms: snapshot.position_now(),
        shuffle: state.settings.shuffle,
        repeat: state.settings.repeat,
        stop_after: state.transport.stop_after(),
        volume: state.settings.volume,
        playlist: state
            .playing_list()
            .or(state.waiting().map(|(list, _)| list))
            .or(state.transport.playback.playing().filter(|_| state.offline))
            .map(|id| id.0),
        offline: state.offline,
        loved: snapshot
            .track
            .as_ref()
            .filter(|_| lastfm_connected(state) && snapshot.status != engine::Status::Loading)
            .map(|track| state.loved.playing(track)),
        here: state.waiting().is_none()
            && (!state.spotify_playing()
                || state.spotify_starting.is_some()
                || state.spotify.as_ref().is_some_and(|queue| queue.here)),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Message {
    pub text: String,
    /// Something went wrong, rather than a note on what was done.
    pub error: bool,
}

pub(crate) fn message(state: &State) -> Option<Message> {
    let (text, error) = match (&state.error, &state.notice) {
        (Some(error), _) => (error, true),
        (None, Some(notice)) => (notice, false),
        (None, None) => return None,
    };
    Some(Message {
        text: text.clone(),
        error,
    })
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Tab {
    pub id: u64,
    pub name: String,
    /// The Spotify tab's: the songs to come.
    pub songs: u32,
    pub duration_ms: u64,
    /// The Spotify tab, rather than a playlist.
    pub spotify: bool,
    /// A synced playlist's: how it stands. None for any other.
    pub sync: Option<TabSync>,
}

/// How a synced playlist stands, for its tab's mark.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum TabSync {
    /// The same as on the other devices.
    UpToDate,
    /// Changes wait to be sent: offline, or being sent.
    Waiting,
    /// Spotify refused a change, for this reason.
    Failed { reason: String },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Tabs {
    pub tabs: Vec<Tab>,
    /// The playlist shown.
    pub viewed: u64,
}

pub(crate) fn tabs(state: &State) -> Tabs {
    Tabs {
        tabs: state
            .tabs()
            .into_iter()
            .filter_map(|id| match (id, &state.spotify) {
                (PlaylistId::SPOTIFY, Some(queue)) => Some(Tab {
                    id: id.0,
                    name: "Spotify".into(),
                    songs: count(queue.len()),
                    duration_ms: to_come(queue)
                        .map(|track| u64::from(track.duration_ms))
                        .sum(),
                    spotify: true,
                    sync: None,
                }),
                _ => state.library.get(id).map(|list| Tab {
                    id: id.0,
                    name: list.name.clone(),
                    songs: count(list.len()),
                    duration_ms: duration(list),
                    spotify: false,
                    sync: list.sync.as_ref().map(|_| match state.sync.get(&id) {
                        Some(SyncStatus::Waiting) => TabSync::Waiting,
                        Some(SyncStatus::Failed(reason)) => TabSync::Failed {
                            reason: reason.clone(),
                        },
                        Some(SyncStatus::UpToDate) | None => TabSync::UpToDate,
                    }),
                }),
            })
            .collect(),
        viewed: state.viewed.0,
    }
}

/// The songs the Spotify tab has to come: the one playing, then the rest.
fn to_come(queue: &SpotifyQueue) -> impl Iterator<Item = &TrackRef> {
    queue.now.iter().chain(
        queue
            .parts
            .iter()
            .flat_map(|part| part.songs.iter().map(|queued| &queued.track)),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Played {
    /// Played before: shown, dimmed, but not played again.
    Before,
    Now,
    /// To come: it can be played from, and removed.
    Next,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SpotifyRow {
    /// "Now playing", "Next in queue", "Next from: …".
    Heading { title: String },
    Song {
        /// What PlayEntry, Remove and the selection know it by.
        entry: u64,
        track: Track,
        played: Played,
        /// Loved on Last.fm.
        loved: bool,
    },
}

/// The Spotify tab: the account's queue, as Spotify's own apps show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SpotifyView {
    /// Where the queue plays, and how: "Playing on Kitchen.".
    pub where_it_plays: String,
    /// Spotless has the queue: songs can be removed.
    pub held_here: bool,
    /// The device holding the queue, when another does: the tab only shows
    /// it until taken over.
    pub held_by: Option<String>,
    /// Songs can be added: Spotless has the queue, or no device has and
    /// Spotless takes it over for them.
    pub takes_songs: bool,
    /// Before, now, then each part under its heading.
    pub rows: Vec<SpotifyRow>,
    /// The song playing and the ones to come.
    pub to_come: u32,
}

pub(crate) fn spotify(state: &State) -> Option<SpotifyView> {
    let queue = state.spotify.as_ref()?;
    let mut rows = Vec::new();
    let mut index = 0;
    let mut song = |rows: &mut Vec<SpotifyRow>, track: &TrackRef, played| {
        rows.push(SpotifyRow::Song {
            entry: SpotifyQueue::row_id(index).0,
            track: track.into(),
            played,
            loved: state.loved.track(track),
        });
        index += 1;
    };
    for track in &queue.before {
        song(&mut rows, track, Played::Before);
    }
    if let Some(now) = &queue.now {
        rows.push(SpotifyRow::Heading {
            title: "Now playing".into(),
        });
        song(&mut rows, now, Played::Now);
    }
    for part in &queue.parts {
        rows.push(SpotifyRow::Heading {
            title: part.title.clone(),
        });
        for queued in &part.songs {
            song(&mut rows, &queued.track, Played::Next);
        }
    }
    Some(SpotifyView {
        where_it_plays: queue.where_it_plays(),
        held_here: queue.held_here(),
        held_by: queue.device.clone().filter(|_| queue.held_elsewhere()),
        takes_songs: queue.takes_songs(),
        rows,
        to_come: count(queue.len()),
    })
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Song {
    pub entry: u64,
    pub track: Track,
    /// The number of its Q mark: it plays next, in that order.
    pub mark: Option<u32>,
    /// It starts a run of songs from one album or source (as Settings
    /// chooses), under this header.
    pub header: Option<Header>,
    /// It follows a header's run without starting one: a line sets it off.
    pub set_off: bool,
    /// Loved on Last.fm.
    pub loved: bool,
}

/// A header over a run of songs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Header {
    /// How many songs it's over.
    pub songs: u32,
    /// The album's or the source's name.
    pub name: String,
    /// What it is, or whose: an album's artist, "Spotify playlist".
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PlaylistView {
    pub id: u64,
    pub name: String,
    pub songs: Vec<Song>,
    /// The song the pointer is on: the one playing, or the one Play starts.
    pub cursor: Option<u64>,
    pub duration_ms: u64,
    /// What's left to play from the song the pointer is on, once some has
    /// played.
    pub left_ms: Option<u64>,
}

pub(crate) fn playlist(state: &State, id: PlaylistId) -> Option<PlaylistView> {
    let list = state.library.get(id)?;
    let by = state.settings.grouping;
    let runs = list.runs(by);
    let run_at = |index: usize| runs.iter().find(|run| run.start == index);
    Some(PlaylistView {
        id: list.id.0,
        name: list.name.clone(),
        songs: list
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| Song {
                entry: entry.id.0,
                track: (&entry.track).into(),
                mark: list.mark_number(entry.id).map(count),
                header: run_at(index).filter(|run| run.header).map(|&run| {
                    let (name, detail) = list.heading(run, by);
                    Header {
                        songs: count(run.len),
                        name,
                        detail,
                    }
                }),
                set_off: run_at(index).is_some_and(|run| !run.header && index > 0),
                loved: state.loved.track(&entry.track),
            })
            .collect(),
        cursor: list.cursor().map(|id| id.0),
        duration_ms: duration(list),
        left_ms: state.left_ms(id),
    })
}

fn duration(list: &Playlist) -> u64 {
    list.entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .sum()
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReleaseGroup {
    Album,
    Single,
    Compilation,
    AppearsOn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ReleaseTab {
    All,
    /// Albums and compilations.
    Albums,
    SinglesAndEps,
    AppearsOn,
}

impl From<ReleaseTab> for CoreTab {
    fn from(tab: ReleaseTab) -> Self {
        match tab {
            ReleaseTab::All => Self::All,
            ReleaseTab::Albums => Self::Albums,
            ReleaseTab::SinglesAndEps => Self::SinglesAndEps,
            ReleaseTab::AppearsOn => Self::AppearsOn,
        }
    }
}

impl From<CoreTab> for ReleaseTab {
    fn from(tab: CoreTab) -> Self {
        match tab {
            CoreTab::All => Self::All,
            CoreTab::Albums => Self::Albums,
            CoreTab::SinglesAndEps => Self::SinglesAndEps,
            CoreTab::AppearsOn => Self::AppearsOn,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Release {
    pub uri: String,
    pub title: String,
    /// Its artists, as one line.
    pub artists: String,
    pub year: i32,
    pub group: ReleaseGroup,
    /// Spotify's word for it: ALBUM, SINGLE, EP, COMPILATION.
    pub kind: String,
    pub songs: u32,
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AlbumSongs {
    NotRead,
    Loading,
    Ready {
        tracks: Vec<Track>,
        /// Each song's number on the album: with a find, only some show.
        numbers: Vec<u32>,
    },
    Failed {
        message: String,
    },
}

/// A release, with its songs when they are shown.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReleaseRow {
    pub release: Release,
    pub expanded: bool,
    pub songs: AlbumSongs,
}

fn album_songs(state: &State, uri: &str) -> AlbumSongs {
    match state.browse.albums.get(uri) {
        None => AlbumSongs::NotRead,
        Some(Loadable::Loading) => AlbumSongs::Loading,
        Some(Loadable::Ready(list)) => AlbumSongs::Ready {
            tracks: tracks(&added_from(state, uri, list)),
            numbers: (1..=list.len()).map(count).collect(),
        },
        Some(Loadable::Failed(message)) => AlbumSongs::Failed {
            message: message.clone(),
        },
    }
}

/// An album's or a Spotify playlist's songs, as added from it, as the
/// core does for a whole one added.
fn added_from(state: &State, uri: &str, tracks: &[TrackRef]) -> Vec<TrackRef> {
    let name = match state.browse.spotify_playlist(uri) {
        Some(playlist) => playlist.name.clone(),
        None => match tracks.first() {
            Some(track) if track.album.uri == uri => track.album.name.clone(),
            _ => return tracks.to_vec(),
        },
    };
    let source = CoreSource {
        uri: uri.to_string(),
        name,
    };
    from_source(tracks.to_vec(), &source)
}

fn release_row(state: &State, release: &catalog::Release) -> ReleaseRow {
    let browse = &state.browse;
    ReleaseRow {
        release: Release {
            uri: release.uri.clone(),
            title: release.title.clone(),
            artists: release.artists.clone(),
            year: release.year,
            group: match release.group {
                CoreGroup::Album => ReleaseGroup::Album,
                CoreGroup::Single => ReleaseGroup::Single,
                CoreGroup::Compilation => ReleaseGroup::Compilation,
                CoreGroup::AppearsOn => ReleaseGroup::AppearsOn,
            },
            kind: release.kind.clone(),
            songs: count(release.tracks),
            cover_url: release.cover.clone(),
        },
        expanded: browse.expanded.contains(&release.uri),
        songs: album_songs(state, &release.uri),
    }
}

/// A release as the find shows it: only with the find in its title or
/// its songs' names, and then open, with those songs only.
fn found_row(state: &State, release: &catalog::Release, find: &Find) -> Option<ReleaseRow> {
    let mut row = release_row(state, release);
    let found: Vec<usize> = (state.browse.found_in(release, find)?.into_iter())
        .map(|(i, _)| i)
        .collect();
    if found.is_empty() {
        return Some(row);
    }
    if let AlbumSongs::Ready { tracks, numbers } = &mut row.songs {
        *tracks = found.iter().map(|&i| tracks[i].clone()).collect();
        *numbers = found.iter().map(|&i| numbers[i]).collect();
    }
    row.expanded = true;
    Some(row)
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SearchPage {
    Loading,
    Ready {
        artists: Vec<Artist>,
        albums: Vec<ReleaseRow>,
        songs: Vec<Track>,
        /// Spotify has more songs: MoreSongs reads the next ones.
        more_songs: bool,
        /// The next songs are being read.
        reading_songs: bool,
    },
    Failed {
        message: String,
    },
}

/// How many releases each tab holds.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TabCounts {
    pub all: u32,
    pub albums: u32,
    pub singles_and_eps: u32,
    pub appears_on: u32,
}

/// Artists like the page's, from Last.fm.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SimilarArtists {
    /// Not asked for yet (SIMILAR not opened).
    NotRead,
    Loading,
    Ready {
        names: Vec<String>,
    },
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum ArtistPage {
    Loading,
    Ready {
        artist: Artist,
        counts: TabCounts,
        /// The tab's releases, in the order chosen; with a find, those
        /// with it.
        releases: Vec<ReleaseRow>,
        /// With a find, the tab's releases whose songs are still being
        /// read for it.
        unread: u32,
        /// Spotify's pick of its most played songs, and each one's rank
        /// among them: with a find, only some show.
        popular: Vec<Track>,
        popular_ranks: Vec<u32>,
        /// What ADD ALL adds: what the page shows.
        add_all: AddAll,
        /// SIMILAR is offered: a Last.fm API key is saved.
        similar_offered: bool,
        similar: SimilarArtists,
    },
    Failed {
        message: String,
    },
}

/// What ADD ALL (`Action::AddAll`) adds on an artist's page.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AddAll {
    /// Nothing shown to add, or similar artists shown.
    Nothing,
    /// The tab's releases, in the order shown.
    Releases { count: u32 },
    /// The popular songs, or what a find found.
    Songs { count: u32 },
    /// A release found by its title is still being read.
    Reading,
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum BrowsePage {
    /// Searches made and artists opened lately, newest first.
    Home {
        searches: Vec<String>,
        recent: Vec<Artist>,
    },
    Search {
        query: String,
        page: SearchPage,
    },
    Artist {
        uri: String,
        page: ArtistPage,
    },
}

/// Browse's tabs: the catalogue, searched, the account's own Spotify
/// playlists, and the songs loved on Last.fm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum BrowseTab {
    Search,
    Spotify,
    LastFm,
}

impl From<BrowseTab> for core_browse::BrowseTab {
    fn from(tab: BrowseTab) -> Self {
        match tab {
            BrowseTab::Search => Self::Search,
            BrowseTab::Spotify => Self::Spotify,
            BrowseTab::LastFm => Self::LastFm,
        }
    }
}

/// A song loved on Last.fm, known there only by its artist and title.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LovedSong {
    pub artist: String,
    pub title: String,
    /// When it was loved, in Unix seconds (0: not known).
    pub date: u64,
}

impl From<&CoreLovedSong> for LovedSong {
    fn from(song: &CoreLovedSong) -> Self {
        Self {
            artist: song.artist.clone(),
            title: song.title.clone(),
            date: song.date,
        }
    }
}

impl From<LovedSong> for CoreLovedSong {
    fn from(song: LovedSong) -> Self {
        Self {
            artist: song.artist,
            title: song.title,
            date: song.date,
        }
    }
}

/// LAST.FM: the songs loved there, by artist and then title.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LovedSongs {
    /// The Last.fm account.
    pub user: String,
    pub songs: Vec<LovedSong>,
}

fn lastfm_connected(state: &State) -> bool {
    matches!(state.lastfm.status, LastFmStatus::Connected { .. })
}

/// LAST.FM, while it's there: Last.fm is connected, and search set up to
/// find its songs on Spotify.
fn loved_songs(state: &State) -> Option<LovedSongs> {
    let LastFmStatus::Connected { user } = &state.lastfm.status else {
        return None;
    };
    (state.search != backend::SearchState::NoApp).then(|| LovedSongs {
        user: user.clone(),
        songs: state.loved.songs().iter().map(Into::into).collect(),
    })
}

/// One of the account's Spotify playlists, or Liked Songs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SpotifyPlaylist {
    pub uri: String,
    pub name: String,
    pub songs: u32,
    /// Who made it, when someone else did: the account follows it.
    pub owner: Option<String>,
    /// Liked Songs, which can't be deleted.
    pub liked: bool,
    /// Only the account, and those it's shared with, see it.
    pub private: bool,
    /// One of Spotless's synced playlists, the Spotless folder's.
    pub synced: bool,
    /// Synced and open as a tab here.
    pub open: bool,
}

/// Where songs copied to Spotify go.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SpotifyTarget {
    /// A new playlist by this name, private or public.
    New { name: String, private: bool },
    /// One of the account's own playlists.
    Playlist { uri: String, name: String },
}

impl From<SpotifyTarget> for catalog::SpotifyTarget {
    fn from(target: SpotifyTarget) -> Self {
        match target {
            SpotifyTarget::New { name, private } => Self::New { name, private },
            SpotifyTarget::Playlist { uri, name } => Self::Playlist { uri, name },
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SpotifyPlaylists {
    Loading,
    /// Liked Songs and the account's own first, then those it follows.
    Ready {
        playlists: Vec<SpotifyPlaylist>,
    },
    Failed {
        message: String,
    },
}

/// YOUR SPOTIFY: the account's playlists, and the one open.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct YourSpotify {
    pub playlists: SpotifyPlaylists,
    /// The playlist whose songs show, by URI.
    pub open: Option<String>,
    /// Its songs; RemoveFromSpotify knows them by their index here.
    pub songs: AlbumSongs,
    /// What UNDO puts back: what was said once it was taken out.
    pub undo: Option<String>,
}

fn your_spotify(state: &State) -> YourSpotify {
    let browse = &state.browse;
    let playlists = match &browse.spotify_playlists {
        None | Some(Loadable::Loading) => SpotifyPlaylists::Loading,
        Some(Loadable::Ready(playlists)) => {
            let view = |p: &catalog::SpotifyPlaylist| SpotifyPlaylist {
                uri: p.uri.clone(),
                name: p.name.clone(),
                songs: count(p.tracks),
                owner: p.owner.clone(),
                liked: p.uri == spotless_core::spotify::catalog::LIKED_SONGS,
                private: p.private,
                synced: state.library.synced_seen.contains(&p.uri),
                open: state.library.synced(&p.uri).is_some(),
            };
            // As the desktop lists them: the account's own, then followed.
            let (mine, followed): (Vec<_>, Vec<_>) =
                playlists.iter().partition(|p| p.owner.is_none());
            SpotifyPlaylists::Ready {
                playlists: mine.into_iter().chain(followed).map(view).collect(),
            }
        }
        Some(Loadable::Failed(message)) => SpotifyPlaylists::Failed {
            message: message.clone(),
        },
    };
    YourSpotify {
        playlists,
        open: browse.spotify_open.clone(),
        songs: browse
            .spotify_open
            .as_deref()
            .map_or(AlbumSongs::NotRead, |uri| album_songs(state, uri)),
        undo: browse.undo.as_ref().map(|undo| undo.notice().to_string()),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrowseView {
    pub open: bool,
    /// The tab shown.
    pub shown: BrowseTab,
    pub page: BrowsePage,
    pub can_go_back: bool,
    /// The playlist ADD adds to.
    pub target: u64,
    pub tab: ReleaseTab,
    /// The artist page shows similar artists instead of the tab.
    pub similar: bool,
    /// The artist page shows its popular songs instead of the releases.
    pub popular: bool,
    /// The similar artist being looked for on Spotify, by name.
    pub finding: Option<String>,
    pub newest_first: bool,
    pub covers: bool,
    pub search: SearchState,
    pub your_spotify: YourSpotify,
    /// The page's find is open, with this typed in it.
    pub find: Option<String>,
    /// LAST.FM, while it's there.
    pub loved: Option<LovedSongs>,
}

pub(crate) fn browse(state: &State) -> BrowseView {
    let browse = &state.browse;
    let find = browse.finder();
    let loved = loved_songs(state);
    let page = match browse.page() {
        Page::Home => BrowsePage::Home {
            searches: browse.recent_searches.clone(),
            recent: browse.recent.iter().map(Into::into).collect(),
        },
        Page::Search(query) => BrowsePage::Search {
            query: query.clone(),
            page: match browse.searches.get(query) {
                None | Some(Loadable::Loading) => SearchPage::Loading,
                Some(Loadable::Ready(results)) => SearchPage::Ready {
                    artists: (results.artists.iter())
                        .filter(|artist| find.as_ref().is_none_or(|f| f.matches(&[&artist.name])))
                        .map(Into::into)
                        .collect(),
                    albums: (results.albums.iter())
                        .filter(|release| {
                            (find.as_ref())
                                .is_none_or(|f| f.matches(&[&release.title, &release.artists]))
                        })
                        .map(|release| release_row(state, release))
                        .collect(),
                    songs: tracks(
                        &(results.songs.iter())
                            .filter(|track| find.as_ref().is_none_or(|f| f.track(track)))
                            .cloned()
                            .collect::<Vec<_>>(),
                    ),
                    more_songs: results.more_songs,
                    reading_songs: results.reading_songs,
                },
                Some(Loadable::Failed(message)) => SearchPage::Failed {
                    message: message.clone(),
                },
            },
        },
        Page::Artist(uri) => BrowsePage::Artist {
            uri: uri.clone(),
            page: match browse.artists.get(uri) {
                None | Some(Loadable::Loading) => ArtistPage::Loading,
                Some(Loadable::Ready(discography)) => {
                    let releases = &discography.releases;
                    let on = |tab: CoreTab| {
                        count(releases.iter().filter(|r| tab.includes(r.group)).count())
                    };
                    let of_tab =
                        || (releases.iter()).filter(|release| browse.tab.includes(release.group));
                    let in_order = browse.releases_shown().into_iter();
                    let shown: Vec<ReleaseRow> = match &find {
                        Some(find) => in_order
                            .filter_map(|release| found_row(state, release, find))
                            .collect(),
                        None => in_order
                            .map(|release| release_row(state, release))
                            .collect(),
                    };
                    let unread = match &find {
                        Some(_) => of_tab()
                            .filter(|release| {
                                matches!(
                                    browse.albums.get(&release.uri),
                                    None | Some(Loadable::Loading)
                                )
                            })
                            .count(),
                        None => 0,
                    };
                    let popular: Vec<(usize, &TrackRef)> = (discography.popular.iter())
                        .enumerate()
                        .filter(|(_, track)| find.as_ref().is_none_or(|f| f.track(track)))
                        .collect();
                    ArtistPage::Ready {
                        artist: (&discography.artist).into(),
                        popular: tracks(
                            &popular
                                .iter()
                                .map(|(_, t)| (*t).clone())
                                .collect::<Vec<_>>(),
                        ),
                        popular_ranks: popular.iter().map(|(i, _)| count(i + 1)).collect(),
                        add_all: match browse.add_all() {
                            None => AddAll::Nothing,
                            Some(core_browse::AddAll::Reading) => AddAll::Reading,
                            Some(core_browse::AddAll::Releases(releases)) => AddAll::Releases {
                                count: count(releases.len()),
                            },
                            Some(core_browse::AddAll::Songs(songs)) => AddAll::Songs {
                                count: count(songs.len()),
                            },
                        },
                        counts: TabCounts {
                            all: on(CoreTab::All),
                            albums: on(CoreTab::Albums),
                            singles_and_eps: on(CoreTab::SinglesAndEps),
                            appears_on: on(CoreTab::AppearsOn),
                        },
                        releases: shown,
                        unread: count(unread),
                        similar_offered: lastfm::api::is_key(&state.lastfm.api_key)
                            || browse.similar_artists.contains_key(uri),
                        similar: match browse.similar_artists.get(uri) {
                            None => SimilarArtists::NotRead,
                            Some(Loadable::Loading) => SimilarArtists::Loading,
                            Some(Loadable::Ready(names)) => SimilarArtists::Ready {
                                names: names.clone(),
                            },
                            Some(Loadable::Failed(message)) => SimilarArtists::Failed {
                                message: message.clone(),
                            },
                        },
                    }
                }
                Some(Loadable::Failed(message)) => ArtistPage::Failed {
                    message: message.clone(),
                },
            },
        },
    };
    BrowseView {
        open: browse.open,
        shown: match browse.shown {
            core_browse::BrowseTab::Search => BrowseTab::Search,
            core_browse::BrowseTab::Spotify => BrowseTab::Spotify,
            // SEARCH in its place while it isn't there.
            core_browse::BrowseTab::LastFm if loved.is_some() => BrowseTab::LastFm,
            core_browse::BrowseTab::LastFm => BrowseTab::Search,
        },
        page,
        can_go_back: browse.can_go_back(),
        target: browse.target.0,
        tab: browse.tab.into(),
        similar: browse.similar,
        popular: browse.popular_shown(),
        finding: browse.finding.as_ref().map(|(name, _)| name.clone()),
        newest_first: browse.newest_first,
        covers: browse.show_covers,
        search: search_state(&state.search),
        your_spotify: your_spotify(state),
        find: browse.find.clone(),
        loved,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SearchState {
    /// No personal app is set up.
    NoApp,
    SignedOut,
    SigningIn,
    Ready,
}

fn search_state(search: &backend::SearchState) -> SearchState {
    match search {
        backend::SearchState::NoApp => SearchState::NoApp,
        backend::SearchState::SignedOut => SearchState::SignedOut,
        backend::SearchState::SigningIn => SearchState::SigningIn,
        backend::SearchState::Ready => SearchState::Ready,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Store {
    Audio,
    Covers,
}

impl From<Store> for backend::Store {
    fn from(store: Store) -> Self {
        match store {
            Store::Audio => Self::Audio,
            Store::Covers => Self::Covers,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum LastFm {
    Off,
    /// Waiting for the browser approval.
    SigningIn,
    Connected {
        user: String,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SettingsView {
    pub search: SearchState,
    /// Search's personal app (empty: none).
    pub search_app: String,
    pub levelling: bool,
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    /// The sizes to choose from, in MB.
    pub audio_cache_sizes: Vec<u64>,
    /// Bytes on disk, once measured (MeasureStorage).
    pub audio_bytes: Option<u64>,
    pub cover_bytes: Option<u64>,
    pub lastfm: LastFm,
    /// The Last.fm API account, as last saved (empty: none).
    pub lastfm_key: String,
    pub lastfm_secret: String,
    /// The colour of the lists and the display, by its key.
    pub phosphor: String,
    /// What the playlists' songs sit under headers by, by its key.
    pub song_headers: String,
    /// The Spotify tab can be used (Settings).
    pub spotify_tab_enabled: bool,
    /// The Spotify tab is offered in Settings at all.
    pub spotify_tab_offered: bool,
    /// It can, but was closed: + offers it again.
    pub spotify_tab_closed: bool,
    /// The time shows what's left of the song instead of what has played.
    pub time_left: bool,
}

/// A colour the lists and the display can take, with its shades as
/// 0xRRGGBB.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PhosphorView {
    pub key: String,
    pub name: String,
    /// Where the colour comes from (empty: nowhere in particular).
    pub origin: String,
    pub text: u32,
    pub dim: u32,
    pub lit: u32,
    pub current: u32,
    pub selected: u32,
}

/// Every colour, in the order Settings lists them.
#[uniffi::export]
pub fn phosphors() -> Vec<PhosphorView> {
    let rgb = |[r, g, b]: [u8; 3]| u32::from_be_bytes([0, r, g, b]);
    Phosphor::ALL
        .into_iter()
        .map(|phosphor| {
            let shades = phosphor.shades();
            PhosphorView {
                key: phosphor.key().into(),
                name: phosphor.name().into(),
                origin: phosphor.origin().unwrap_or_default().into(),
                text: rgb(shades.text),
                dim: rgb(shades.dim),
                lit: rgb(shades.lit),
                current: rgb(shades.current),
                selected: rgb(shades.selected),
            }
        })
        .collect()
}

/// A way to put the playlists' songs under headers.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct GroupingView {
    pub key: String,
    pub name: String,
}

/// Every way, in the order Settings lists them.
#[uniffi::export]
pub fn groupings() -> Vec<GroupingView> {
    Grouping::ALL
        .into_iter()
        .map(|grouping| GroupingView {
            key: grouping.key().into(),
            name: grouping.name().into(),
        })
        .collect()
}

pub(crate) fn settings(state: &State) -> SettingsView {
    let settings = &state.settings;
    SettingsView {
        search: search_state(&state.search),
        search_app: settings.web_client_id().unwrap_or_default(),
        levelling: settings.levelling,
        audio_cache: settings.audio_cache,
        audio_cache_mb: settings.audio_cache_mb,
        audio_cache_sizes: AUDIO_CACHE_SIZES.to_vec(),
        audio_bytes: state.storage.map(|s| s.audio),
        cover_bytes: state.storage.map(|s| s.covers),
        lastfm: match &state.lastfm.status {
            LastFmStatus::Off => LastFm::Off,
            LastFmStatus::SigningIn => LastFm::SigningIn,
            LastFmStatus::Connected { user } => LastFm::Connected { user: user.clone() },
        },
        lastfm_key: state.lastfm.api_key.clone(),
        lastfm_secret: state.lastfm.secret.clone(),
        phosphor: settings.phosphor.key().into(),
        song_headers: settings.grouping.key().into(),
        spotify_tab_enabled: settings.spotify_tab_enabled,
        spotify_tab_offered: spotless_core::store::SPOTIFY_TAB,
        spotify_tab_closed: settings.spotify_tab_enabled && !settings.spotify_tab,
        time_left: settings.time_left,
    }
}
