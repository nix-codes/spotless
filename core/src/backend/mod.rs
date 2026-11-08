//! The bridge between the UI thread and asynchronous work.
//!
//! The UI runs on the main thread and must never block. A tokio runtime on
//! its own thread hosts the session, the player and catalogue reads. The UI
//! sends [`Command`]s; the worker answers with [`Event`]s and wakes the UI
//! with the callback it was started with (egui's `request_repaint` on the
//! desktop), so the app stays idle when nothing happens.
//!
//! This file is what the two sides say to each other; `worker/` is what
//! the worker does with it.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Result;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::browse::{Removal, Unlisted};
use crate::browser::Browser;
use crate::images::CoverImage;
use crate::lastfm::{LastFmState, Loved, LovedSong, api::Keys};
use crate::model::catalog::{Discography, SearchResults, Songs, SpotifyPlaylist, SpotifyTarget};
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::PlayerSnapshot;
use crate::playback::link::AudioLink;
use crate::playback::outputs::Output;
use crate::playback::transport::Request;
use crate::spotify::code::{CodeImage, CodeTarget};
use crate::spotify::queue::SpotifyQueue;
use crate::spotify::session::AudioCache;
use crate::spotify::synced;
use crate::spotify::webapi::SearchPage;

mod worker;

use worker::Worker;

#[derive(Debug)]
pub(crate) enum Command {
    SignIn,
    SignOut,
    /// Reads the songs of a track, album or artist link for a playlist.
    AddLink {
        text: String,
        playlist: PlaylistId,
    },
    Player(Request),
    Seek(u32),
    Volume(u16),
    Search(String),
    /// The search's songs after the first `offset`.
    MoreSongs {
        query: String,
        offset: usize,
    },
    /// The search's artists after the first `offset`.
    MoreArtists {
        query: String,
        offset: usize,
    },
    /// An artist's discography, by artist URI.
    Discography(String),
    /// Artists like this one, from Last.fm with this API key.
    SimilarArtists {
        artist: ArtistRef,
        api_key: String,
    },
    /// The Spotify artist with this name, for a similar artist opened.
    FindArtist(String),
    /// These songs loved on Last.fm, on Spotify; answered with
    /// `Event::SongsFound` and this ID.
    FindSongs {
        id: u64,
        songs: Vec<LovedSong>,
    },
    /// The songs of an album, a Spotify playlist or Liked Songs, by URI.
    Album(String),
    /// What the Spotify Code in this picture points to; answered with
    /// `Event::CodeRead`.
    ReadCode(CodeImage),
    /// What the code with this number points to; answered with
    /// `Event::CodeRead`.
    LookUpCode(u64),
    /// What this Spotify link points to; answered with `Event::LinkRead`.
    ReadLink(String),
    /// The account's own Spotify playlists, Liked Songs first.
    SpotifyPlaylists,
    /// Adds songs, by track URI, to a Spotify playlist, or to a new one.
    CopyToSpotify {
        target: SpotifyTarget,
        songs: Vec<String>,
    },
    /// Takes songs out of a Spotify playlist, by their places in it at
    /// this revision, or out of Liked Songs.
    RemoveFromSpotify {
        removal: Removal,
        revision: Vec<u8>,
    },
    /// Puts removed songs back.
    PutBackOnSpotify(Removal),
    /// Takes a playlist out of the account's list; `notice` is what the
    /// status bar says once it's out.
    UnlistSpotifyPlaylist {
        playlist: String,
        name: String,
        notice: String,
    },
    /// Puts a playlist taken out back in the account's list.
    RelistSpotifyPlaylist(Unlisted),
    /// An album cover, by image URL.
    Cover(String),
    /// The personal app search signs in with changed (None: no app).
    SearchApp(Option<String>),
    /// Volume levelling on or off.
    Levelling(bool),
    /// The Spotify tab opened or closed: Spotless joins Spotify Connect, or
    /// leaves it, stopping the queue if it plays here.
    SpotifyTab(
        #[cfg_attr(
            not(feature = "spotify-tab"),
            allow(dead_code, reason = "read by the Spotify tab's device, not built")
        )]
        bool,
    ),
    /// How much the caches hold; answered with `Event::Storage`.
    MeasureStorage,
    /// The system has a network again: a dropped session reconnects now,
    /// not after the wait since the last try.
    NetworkBack,
    /// Empties a cache, then measures again.
    ClearCache(Store),
    /// The outputs to choose from; answered with `Event::Outputs`.
    ListOutputs,
    /// Plays to this output (None: the system's default).
    Output(Option<String>),
    /// Saves this Last.fm API account and approves it in the browser.
    LastFmConnect(Keys),
    LastFmDisconnect,
    /// Loves a song on Last.fm, or doesn't any more.
    LastFmLove {
        artist: String,
        title: String,
        loved: bool,
    },
    /// For the Spotify tab.
    Spotify(
        #[cfg_attr(
            not(feature = "spotify-tab"),
            allow(dead_code, reason = "read by the Spotify tab's device, not built")
        )]
        SpotifyRequest,
    ),
    /// Syncs a synced playlist; answered with `Event::Synced`.
    Sync {
        list: PlaylistId,
        job: synced::Job,
    },
    /// Makes a playlist's Spotify playlist, in the Spotless folder, as it
    /// starts being synced; answered with `Event::SyncCreated`.
    CreateSynced {
        list: PlaylistId,
        name: String,
        /// Deletes the folder's playlists by that name first, rather than
        /// making none (`Event::SyncNameTaken`).
        replace: bool,
    },
    /// The Spotless folder's playlists; answered with `Event::SyncFolder`.
    SyncFolder,
}

/// What the Spotify tab asks of the account's queue.
#[derive(Debug)]
#[cfg_attr(
    not(feature = "spotify-tab"),
    allow(dead_code, reason = "read by the Spotify tab's device, not built")
)]
pub(crate) enum SpotifyRequest {
    Play,
    Pause,
    /// Pauses at the song's start: Spotify has no stop, and a song paused
    /// there shows as stopped.
    Stop,
    TogglePause,
    Next,
    Previous,
    Seek(u32),
    /// Adds songs, or an album, by URI to the end of the queue.
    Add(Vec<String>),
    /// Removes songs to come, by their place in Spotify's list and URI.
    Remove(Vec<(usize, String)>),
    /// Takes playback over from the device playing the queue, and plays it
    /// here, from this song to come if one is given (by URI): the songs
    /// before it leave the queue, as Spotify's apps skip to a song.
    PlayHere(Option<String>),
    /// Stops the queue and empties it: the songs queued go, and Empty
    /// Track, paused, takes the place of the song and of the album or
    /// playlist it plays from. Takes it over first, if no device has it,
    /// without playing it.
    Clear,
    /// Takes the queue from the device holding it, paused: that device is
    /// paused first, as Spotify hands a playing queue over playing.
    TakeOver,
}

/// What Spotless keeps on disk that can be thrown away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Audio,
    Covers,
}

/// Bytes on disk per cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Storage {
    pub audio: u64,
    pub covers: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchState {
    /// No personal app is set up.
    NoApp,
    SignedOut,
    SigningIn,
    Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Connecting,
    SignedOut,
    /// Waiting for the browser approval.
    SigningIn,
    Ready {
        user: String,
    },
    Failed(String),
}

#[derive(Debug)]
pub(crate) enum Event {
    Session(SessionState),
    Player(PlayerSnapshot),
    /// The engine started the followed song by itself.
    Followed(EntryId),
    /// The engine finished a song with nothing to follow, and stopped.
    Ended,
    /// Another of the account's devices started playing, by name, so
    /// Spotless paused: Spotify allows one at a time.
    PlayingElsewhere(String),
    /// Spotless started playing and paused the other device, by name.
    PausedElsewhere(String),
    /// The account's queue, for the Spotify tab; None while Spotless isn't
    /// in Spotify Connect.
    #[cfg_attr(
        not(feature = "spotify-tab"),
        allow(dead_code, reason = "sent by the Spotify tab's device, not built")
    )]
    Spotify(Option<SpotifyQueue>),
    /// The loaded song can't be played; `offline`: for want of a
    /// connection.
    Unavailable {
        offline: bool,
    },
    /// The playing song stopped short here for want of a connection.
    Cut {
        position_ms: u32,
    },
    /// The session dropped and a new one took its place.
    Reconnected,
    Search(SearchState),
    SearchResults {
        query: String,
        results: Result<SearchResults, String>,
    },
    /// A search's next page of songs, and whether Spotify has more.
    MoreSongs {
        query: String,
        result: Result<SearchPage<TrackRef>, String>,
    },
    /// A search's next page of artists, and whether Spotify has more.
    MoreArtists {
        query: String,
        result: Result<SearchPage<ArtistRef>, String>,
    },
    Discography {
        artist: String,
        result: Result<Discography, String>,
    },
    Album {
        album: String,
        result: Result<Songs, String>,
    },
    SpotifyPlaylists(Result<Vec<SpotifyPlaylist>, String>),
    /// Songs removed from a Spotify playlist, or put back with `undo`.
    SpotifyEdited {
        removal: Removal,
        undo: bool,
        result: Result<(), String>,
    },
    /// A playlist taken out of the account's list, or put back with
    /// `undo`.
    SpotifyUnlisted {
        result: Result<Unlisted, String>,
        undo: bool,
    },
    /// Songs copied to a Spotify playlist: its URI, once they are.
    CopiedToSpotify {
        target: SpotifyTarget,
        songs: usize,
        result: Result<String, String>,
    },
    /// Similar artists' names, for an artist's URI.
    SimilarArtists {
        artist: String,
        result: Result<Vec<String>, String>,
    },
    /// The artist a `FindArtist` name is on Spotify (None: none by it).
    ArtistFound {
        name: String,
        result: Result<Option<ArtistRef>, String>,
    },
    /// A `FindSongs`' songs on Spotify, in order (None: not found).
    SongsFound {
        id: u64,
        result: Result<Vec<Option<TrackRef>>, String>,
    },
    Cover {
        url: String,
        result: Result<CoverImage, String>,
    },
    Storage(Storage),
    LastFm(LastFmState),
    /// The songs loved on Last.fm.
    Loved(Loved),
    /// The outputs to choose from; None when they can't be listed.
    Outputs(Option<Vec<Output>>),
    /// The songs of an `AddLink`.
    Tracks {
        playlist: PlaylistId,
        tracks: Vec<TrackRef>,
    },
    /// A synced playlist synced, or not.
    Synced {
        list: PlaylistId,
        result: Result<synced::Done, String>,
    },
    /// A playlist's Spotify playlist made, by URI, as it starts being
    /// synced.
    SyncCreated {
        list: PlaylistId,
        result: Result<String, String>,
    },
    /// A playlist wasn't made for `list`: the Spotless folder has one by
    /// its name already.
    SyncNameTaken {
        list: PlaylistId,
    },
    /// The Spotless folder's playlists, by URI and name.
    SyncFolder(Result<Vec<(String, String)>, String>),
    /// Spotify told of a change to one of the account's playlists.
    PlaylistChanged(synced::Changed),
    /// Spotify told of a change to the account's list of playlists.
    PlaylistsChanged,
    /// A Spotify Code's bars and what it points to, or why it couldn't
    /// be read.
    CodeRead(Result<([u8; 23], CodeTarget), String>),
    /// What a Spotify link points to, or why it couldn't be opened.
    LinkRead(Result<CodeTarget, String>),
    Error(String),
}

/// What the worker starts with, from the controller's settings.
#[derive(Clone, Debug, Default)]
pub(crate) struct Start {
    /// librespot's 0–65535.
    pub volume: u16,
    pub levelling: bool,
    /// Spotless joins Spotify Connect, for the Spotify tab.
    pub spotify_tab: bool,
    /// The personal app search signs in with.
    pub search_app: Option<String>,
    /// Every session's audio cache, until the next start.
    pub audio_cache: AudioCache,
}

pub(crate) struct Backend {
    commands: UnboundedSender<Command>,
    pub(crate) events: mpsc::Receiver<Event>,
}

impl Backend {
    /// `wake` is called after every event, from the worker's thread.
    /// `browser` opens the approval pages; the audio outputs use `link`'s
    /// equaliser, output and analyser.
    pub fn start(
        paths: Paths,
        start: Start,
        browser: Browser,
        link: Arc<AudioLink>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("spotless-backend")
            .build()?;
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        let emit: Emit = Arc::new(move |event| {
            let _ = sender.send(event);
            wake();
        });
        std::thread::Builder::new()
            .name("spotless-runtime".into())
            .spawn(move || {
                runtime.block_on(
                    Worker::new(paths, start, browser, link, emit).run(receiver),
                );
            })?;
        Ok(Self { commands, events })
    }

    /// A backend with no worker, for tests: the commands it is sent come out
    /// of the receiver, and the events put in the sender come in.
    #[cfg(test)]
    pub fn detached() -> (
        Self,
        tokio::sync::mpsc::UnboundedReceiver<Command>,
        mpsc::Sender<Event>,
    ) {
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        (Self { commands, events }, receiver, sender)
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::error!("the backend has stopped");
        }
    }
}

type Emit = Arc<dyn Fn(Event) + Send + Sync>;
