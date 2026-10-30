//! Spotless's core for the Android app, through UniFFI. Kotlin makes one
//! [`Spotless`], hands it [`Action`]s, and reads what the screens show from
//! it whenever its [`Host`] is woken.
//!
//! The types here are the core's, flattened into plain records the
//! bindings can carry; `view.rs` makes them.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use spotless_core::backend::{Command, Event};
use spotless_core::controller::Controller;
use spotless_core::model::{EntryId, PlaylistId};
use spotless_core::paths::Paths;

#[cfg(target_os = "android")]
mod android;
mod view;

pub use view::*;

uniffi::setup_scaffolding!();

/// What the app does for the core.
#[uniffi::export(with_foreign)]
pub trait Host: Send + Sync {
    /// The core has news: call [`Spotless::update`] soon, on the app's own
    /// thread. Called from the core's thread, which must not wait on it.
    fn wake(&self);
    /// Opens an approval page (Spotify's, search's, Last.fm's) in the
    /// browser.
    fn open(&self, url: String);
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StartError {
    // Not `message`, which Kotlin's exceptions have already.
    #[error("{reason}")]
    Folders { reason: String },
}

/// An album cover, read for [`Spotless::load_cover`].
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Cover {
    pub url: String,
    /// Its pixels, or `None` if it couldn't be read.
    pub image: Option<CoverImage>,
}

/// A cover's pixels, 8-bit RGBA row by row; opaque.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct CoverImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The Spotify links of these songs, albums, artists or playlists, one a
/// line, to share; empty if none has one (Liked Songs, local files).
#[uniffi::export]
pub fn spotify_links(uris: Vec<String>) -> String {
    spotless_core::spotify::links(uris.iter().map(String::as_str))
}

/// The made-up artist a demo's Browse has ready (`spotless_core::demo`).
#[uniffi::export]
pub fn demo_artist() -> view::Artist {
    let artist = spotless_core::demo::artist();
    view::Artist {
        uri: artist.uri,
        name: artist.name,
        image: artist.image,
    }
}

#[derive(uniffi::Object)]
pub struct Spotless {
    controller: Mutex<Controller>,
    /// Covers read since the app last took them.
    covers: Mutex<Vec<Cover>>,
    /// In a demo, the host to wake when covers are in: they are drawn
    /// here, not fetched.
    demo: Option<Arc<dyn Host>>,
}

#[uniffi::export]
impl Spotless {
    /// Loads the playlists and settings from these folders (the app's own,
    /// made if missing) and starts connecting.
    #[uniffi::constructor]
    pub fn new(
        config_dir: String,
        cache_dir: String,
        host: Arc<dyn Host>,
    ) -> Result<Arc<Self>, StartError> {
        let paths = Paths::from_dirs(config_dir.into(), cache_dir.into()).map_err(|error| {
            StartError::Folders {
                reason: format!("{error:#}"),
            }
        })?;
        let opener = Arc::clone(&host);
        let controller = Controller::with_browser(
            paths,
            Arc::new(move |url: &str| opener.open(url.to_string())),
            move || host.wake(),
        );
        Ok(Arc::new(Self {
            controller: Mutex::new(controller),
            covers: Mutex::new(Vec::new()),
            demo: None,
        }))
    }

    /// The made-up library the screenshots show
    /// (`spotless_core::demo`): signed in, a song playing, and nothing
    /// connected to Spotify. The settings are read from and saved in
    /// these folders, as `new` does.
    #[uniffi::constructor]
    pub fn demo(
        config_dir: String,
        cache_dir: String,
        host: Arc<dyn Host>,
    ) -> Result<Arc<Self>, StartError> {
        let paths = Paths::from_dirs(config_dir.into(), cache_dir.into()).map_err(|error| {
            StartError::Folders {
                reason: format!("{error:#}"),
            }
        })?;
        Ok(Arc::new(Self {
            controller: Mutex::new(Controller::demo(paths)),
            covers: Mutex::new(Vec::new()),
            demo: Some(host),
        }))
    }

    pub fn apply(&self, action: Action) {
        let mut controller = self.lock();
        // The core's is a switch, Browse's button on the desktop.
        if matches!(action, Action::OpenBrowse) && controller.state().browse.open {
            return;
        }
        controller.apply(action.into());
    }

    /// Takes in the core's news, and saves once changes have rested. Returns
    /// the milliseconds after which to call it again, while a save is due.
    pub fn update(&self) -> Option<u64> {
        let mut controller = self.lock();
        let arrived = controller
            .receive()
            .into_iter()
            .filter_map(|event| match event {
                Event::Cover { url, result } => Some(Cover {
                    url,
                    image: result.ok().map(|image| CoverImage {
                        width: image.width,
                        height: image.height,
                        rgba: image.rgba,
                    }),
                }),
                _ => None,
            });
        self.covers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .extend(arrived);
        controller.tick().map(millis)
    }

    /// Reads a cover, from the disk cache or downloaded into it. It comes
    /// back in [`Spotless::take_covers`] after a wake. Ask once per URL.
    pub fn load_cover(&self, url: String) {
        let Some(host) = &self.demo else {
            return self.lock().send(Command::Cover(url));
        };
        let image = spotless_core::demo::cover(&url).map(|image| CoverImage {
            width: image.width,
            height: image.height,
            rgba: image.rgba,
        });
        self.covers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(Cover { url, image });
        host.wake();
    }

    /// The covers read since the last call.
    pub fn take_covers(&self) -> Vec<Cover> {
        std::mem::take(&mut *self.covers.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Saves now, as the app goes to the background.
    pub fn save(&self) {
        self.lock().save();
    }

    pub fn session(&self) -> Session {
        view::session(self.lock().state())
    }

    /// The position in it is the one now: read it again every so often
    /// while a song plays.
    pub fn player(&self) -> Player {
        view::player(self.lock().state())
    }

    /// The last thing that went wrong, or else the last thing done.
    pub fn message(&self) -> Option<Message> {
        view::message(self.lock().state())
    }

    pub fn tabs(&self) -> Tabs {
        view::tabs(self.lock().state())
    }

    pub fn playlist(&self, id: u64) -> Option<PlaylistView> {
        view::playlist(self.lock().state(), PlaylistId(id))
    }

    /// The Spotify tab's queue, while the tab shows.
    pub fn spotify(&self) -> Option<SpotifyView> {
        view::spotify(self.lock().state())
    }

    pub fn browse(&self) -> BrowseView {
        view::browse(self.lock().state())
    }

    pub fn settings(&self) -> SettingsView {
        view::settings(self.lock().state())
    }
}

impl Spotless {
    fn lock(&self) -> MutexGuard<'_, Controller> {
        self.controller.lock().unwrap_or_else(|p| p.into_inner())
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn entries(ids: Vec<u64>) -> Vec<EntryId> {
    ids.into_iter().map(EntryId).collect()
}

/// What the screens ask for: the core's actions, less the desktop's own
/// (audio outputs, the PL switch, drags onto the slim column).
///
/// The Spotify tab is a tab like the playlists, by its ID in [`Tab`]: its
/// songs are played, removed and copied by the entries [`SpotifyView`]
/// gives them.
#[derive(Clone, Debug, uniffi::Enum)]
pub enum Action {
    SignIn,
    SignOut,
    /// Search signs in with this personal app's client ID (empty: none).
    SetSearchApp {
        client_id: String,
    },
    SetLevelling {
        on: bool,
    },
    /// The lists and the display in this colour, by its key.
    SetPhosphor {
        key: String,
    },
    /// The playlists' songs under headers by this way, by its key.
    SetGrouping {
        key: String,
    },
    /// Saves this Last.fm API account and opens its approval page.
    ConnectLastFm {
        api_key: String,
        secret: String,
    },
    DisconnectLastFm,
    /// Loves a song on Last.fm, by its first artist and title, or doesn't
    /// any more.
    SetLoved {
        artist: String,
        title: String,
        loved: bool,
    },
    /// From the next start.
    SetAudioCache {
        on: bool,
    },
    /// The audio cache's limit in MB, from the next start.
    SetAudioCacheSize {
        mb: u64,
    },
    MeasureStorage,
    /// The phone has a network again.
    NetworkBack,
    ClearCache {
        store: Store,
    },
    /// Adds a track, album or artist link's songs.
    AddLink {
        link: String,
    },
    Play,
    TogglePause,
    Stop,
    /// Stops once the playing song ends, or doesn't any more (Winamp's
    /// Stop after current). Only while a song is loaded.
    ToggleStopAfter,
    /// The time shows what's left of the song, or what has played.
    ToggleTimeLeft,
    /// Stops, keeping where the song was: the app was put away.
    Release,
    Next,
    Previous,
    Seek {
        position_ms: u32,
    },
    /// 0–100.
    Volume {
        percent: u8,
    },
    SetShuffle {
        on: bool,
    },
    SetRepeat {
        on: bool,
    },
    PlayEntry {
        playlist: u64,
        entry: u64,
    },
    Remove {
        playlist: u64,
        entries: Vec<u64>,
    },
    /// Moves these songs of the viewed playlist, keeping their order, to
    /// just before the song now at this index.
    Move {
        entries: Vec<u64>,
        before: u32,
    },
    /// Q marks on the viewed playlist's songs.
    ToggleMarks {
        entries: Vec<u64>,
    },
    SelectPlaylist {
        playlist: u64,
    },
    NewPlaylist,
    /// Makes a playlist of these songs and shows it.
    NewPlaylistWith {
        tracks: Vec<Track>,
    },
    /// Deletes a playlist and its songs (never Default).
    ClosePlaylist {
        playlist: u64,
    },
    RenamePlaylist {
        playlist: u64,
        name: String,
    },
    /// Moves a tab to this index, counted after it is taken out.
    MovePlaylist {
        playlist: u64,
        index: u32,
    },
    /// Opens Browse, adding to the viewed playlist.
    OpenBrowse,
    CloseBrowse,
    Back,
    Search {
        query: String,
    },
    /// Reads the search's next songs.
    MoreSongs {
        query: String,
    },
    /// Opens the page's find with this typed in it, or closes it (None).
    Find {
        text: Option<String>,
    },
    /// Drops a search from the recent ones, or all of them with None.
    ForgetSearch {
        query: Option<String>,
    },
    OpenArtist {
        artist: Artist,
    },
    /// Shows or hides an album's songs, reading them if needed.
    ToggleAlbum {
        album: String,
    },
    /// Shows these albums' songs, or hides them all with an empty list.
    ExpandAlbums {
        albums: Vec<String>,
    },
    SetTab {
        tab: ReleaseTab,
    },
    /// Shows artists like this one (the artist page's), from Last.fm.
    ShowSimilar {
        artist: Artist,
    },
    /// The artist page shows its popular songs instead of the releases.
    ShowPopular,
    /// Adds what the artist's page shows (ADD ALL).
    AddAll,
    /// Opens a similar artist, by name, finding it on Spotify first.
    OpenSimilar {
        name: String,
    },
    ToggleSort,
    /// Shows a tab of Browse, opening Browse if it's closed.
    ShowBrowseTab {
        tab: BrowseTab,
    },
    /// Reads the account's Spotify playlists again.
    ReadSpotifyPlaylists,
    /// Shows a Spotify playlist's songs in YOUR SPOTIFY, or Liked Songs',
    /// by URI.
    OpenSpotifyPlaylist {
        uri: String,
    },
    /// Removes the songs at these indices of the open playlist's from it
    /// on Spotify (from Liked Songs: un-likes them). Only the account's
    /// own and Liked Songs.
    RemoveFromSpotify {
        uri: String,
        rows: Vec<u32>,
    },
    /// Deletes one of the account's own Spotify playlists, or unfollows
    /// someone else's. Never Liked Songs.
    UnlistSpotifyPlaylist {
        uri: String,
    },
    /// Puts back the songs or playlist taken out last.
    UndoSpotifyRemoval,
    /// Syncs a playlist, keeping it the same on the account's other
    /// devices, or stops: its Spotify playlist is deleted, and it stays
    /// here only (the other devices keep theirs, not synced).
    SetSync {
        playlist: u64,
        on: bool,
    },
    /// Syncs a playlist whose name the Spotless folder has already,
    /// deleting that one first (its devices keep it, not synced).
    ReplaceSynced {
        playlist: u64,
    },
    /// The question of a name taken in the Spotless folder goes unanswered.
    DismissSyncNameTaken,
    /// Opens a synced playlist of the Spotless folder as a tab, by URI
    /// and name, or shows it if it's open.
    OpenSynced {
        uri: String,
        name: String,
    },
    /// Copies songs to a Spotify playlist of the account's own, or to a
    /// new one, once: it isn't kept in step with the songs' tab after.
    CopyToSpotify {
        target: SpotifyTarget,
        tracks: Vec<Track>,
    },
    /// The playlist ADD adds to.
    SetTarget {
        playlist: u64,
    },
    ToggleCovers,
    AddTracks {
        tracks: Vec<Track>,
    },
    AddAlbum {
        album: String,
    },
    /// Puts a song after the target playlist's current one and plays it.
    PlayNow {
        track: Track,
    },
    /// Finds songs loved on Last.fm on Spotify and adds them.
    AddLoved {
        songs: Vec<LovedSong>,
    },
    /// Finds a song loved on Last.fm on Spotify and plays it as PlayNow.
    PlayLoved {
        song: LovedSong,
    },
    /// Copies songs to the end of a playlist (the Spotify tab's: queues
    /// them).
    CopyTo {
        playlist: u64,
        tracks: Vec<Track>,
    },
    /// Empties the Spotify tab: stops, and drops the song, the queue and
    /// the album or playlist it plays from. Not while another device holds
    /// the queue.
    ClearSpotify,
    /// Moves these songs of a playlist to the end of another. To the
    /// Spotify tab: queues them, leaving them where they were if Spotify
    /// won't take them. From it: only songs to come, by their entries in
    /// [`SpotifyView`].
    MoveTo {
        from: u64,
        to: u64,
        entries: Vec<u64>,
    },
    /// Takes the queue from the device holding it, paused.
    TakeOverSpotify,
    /// Opens the Spotify tab again, closed with ClosePlaylist like any tab.
    OpenSpotifyTab,
    /// The Spotify tab can be used, or not (Settings).
    SetSpotifyTabEnabled {
        on: bool,
    },
}

impl From<Action> for spotless_core::action::Action {
    fn from(action: Action) -> Self {
        use spotless_core::action::Action as Core;
        match action {
            Action::SignIn => Core::SignIn,
            Action::SignOut => Core::SignOut,
            Action::SetSearchApp { client_id } => Core::SetSearchApp(client_id),
            Action::SetLevelling { on } => Core::SetLevelling(on),
            Action::SetPhosphor { key } => {
                Core::SetPhosphor(spotless_core::phosphor::Phosphor::from_key(Some(&key)))
            }
            Action::SetGrouping { key } => {
                Core::SetGrouping(spotless_core::model::Grouping::from_key(Some(&key)))
            }
            Action::ConnectLastFm { api_key, secret } => Core::ConnectLastFm(api_key, secret),
            Action::DisconnectLastFm => Core::DisconnectLastFm,
            Action::SetLoved {
                artist,
                title,
                loved,
            } => Core::SetLoved {
                artist,
                title,
                loved,
            },
            Action::SetAudioCache { on } => Core::SetAudioCache(on),
            Action::SetAudioCacheSize { mb } => Core::SetAudioCacheSize(mb),
            Action::MeasureStorage => Core::MeasureStorage,
            Action::NetworkBack => Core::NetworkBack,
            Action::ClearCache { store } => Core::ClearCache(store.into()),
            Action::AddLink { link } => Core::AddLink(link),
            Action::Play => Core::Play,
            Action::TogglePause => Core::TogglePause,
            Action::Stop => Core::Stop,
            Action::ToggleStopAfter => Core::ToggleStopAfter,
            Action::ToggleTimeLeft => Core::ToggleTimeLeft,
            Action::Release => Core::Release,
            Action::Next => Core::Next,
            Action::Previous => Core::Previous,
            Action::Seek { position_ms } => Core::Seek(position_ms),
            Action::Volume { percent } => Core::Volume(percent),
            Action::SetShuffle { on } => Core::SetShuffle(on),
            Action::SetRepeat { on } => Core::SetRepeat(on),
            Action::PlayEntry { playlist, entry } => {
                Core::PlayEntry(PlaylistId(playlist), EntryId(entry))
            }
            Action::Remove {
                playlist,
                entries: ids,
            } => Core::Remove(PlaylistId(playlist), entries(ids)),
            Action::Move {
                entries: ids,
                before,
            } => Core::Move(entries(ids), before as usize),
            Action::ToggleMarks { entries: ids } => Core::ToggleMarks(entries(ids)),
            Action::SelectPlaylist { playlist } => Core::SelectPlaylist(PlaylistId(playlist)),
            Action::NewPlaylist => Core::NewPlaylist,
            Action::NewPlaylistWith { tracks } => {
                Core::NewPlaylistWith(tracks.into_iter().map(Into::into).collect())
            }
            Action::ClosePlaylist { playlist } => Core::ClosePlaylist(PlaylistId(playlist)),
            Action::RenamePlaylist { playlist, name } => {
                Core::RenamePlaylist(PlaylistId(playlist), name)
            }
            Action::MovePlaylist { playlist, index } => {
                Core::MovePlaylist(PlaylistId(playlist), index as usize)
            }
            Action::OpenBrowse => Core::ToggleBrowse,
            Action::CloseBrowse => Core::CloseBrowse,
            Action::Back => Core::Back,
            Action::Search { query } => Core::Search(query),
            Action::MoreSongs { query } => Core::MoreSongs(query),
            Action::Find { text } => Core::Find(text),
            Action::ForgetSearch { query } => Core::ForgetSearch(query),
            Action::OpenArtist { artist } => Core::OpenArtist(artist.into()),
            Action::ToggleAlbum { album } => Core::ToggleAlbum(album),
            Action::ExpandAlbums { albums } => Core::ExpandAlbums(albums),
            Action::SetTab { tab } => Core::SetTab(tab.into()),
            Action::ShowSimilar { artist } => Core::ShowSimilar(artist.into()),
            Action::ShowPopular => Core::ShowPopular,
            Action::AddAll => Core::AddAll,
            Action::OpenSimilar { name } => Core::OpenSimilar(name),
            Action::ToggleSort => Core::ToggleSort,
            Action::ShowBrowseTab { tab } => Core::ShowBrowseTab(tab.into()),
            Action::ReadSpotifyPlaylists => Core::ReadSpotifyPlaylists,
            Action::OpenSpotifyPlaylist { uri } => Core::OpenSpotifyPlaylist(uri),
            Action::RemoveFromSpotify { uri, rows } => {
                Core::RemoveFromSpotify(uri, rows.into_iter().map(|row| row as usize).collect())
            }
            Action::UnlistSpotifyPlaylist { uri } => Core::UnlistSpotifyPlaylist(uri),
            Action::UndoSpotifyRemoval => Core::UndoSpotifyRemoval,
            Action::SetSync { playlist, on } => Core::SetSync(PlaylistId(playlist), on),
            Action::ReplaceSynced { playlist } => Core::ReplaceSynced(PlaylistId(playlist)),
            Action::DismissSyncNameTaken => Core::DismissSyncNameTaken,
            Action::OpenSynced { uri, name } => Core::OpenSynced(uri, name),
            Action::CopyToSpotify { target, tracks } => {
                Core::CopyToSpotify(target.into(), tracks.into_iter().map(Into::into).collect())
            }
            Action::SetTarget { playlist } => Core::SetTarget(PlaylistId(playlist)),
            Action::ToggleCovers => Core::ToggleCovers,
            Action::AddTracks { tracks } => {
                Core::AddTracks(tracks.into_iter().map(Into::into).collect())
            }
            Action::AddAlbum { album } => Core::AddAlbum(album),
            Action::PlayNow { track } => Core::PlayNow(track.into()),
            Action::AddLoved { songs } => {
                Core::AddLoved(songs.into_iter().map(Into::into).collect())
            }
            Action::PlayLoved { song } => Core::PlayLoved(song.into()),
            Action::CopyTo { playlist, tracks } => Core::CopyTo(
                PlaylistId(playlist),
                tracks.into_iter().map(Into::into).collect(),
            ),
            Action::ClearSpotify => Core::ClearSpotify,
            Action::MoveTo {
                from,
                to,
                entries: ids,
            } => Core::MoveTo(PlaylistId(from), PlaylistId(to), entries(ids)),
            Action::TakeOverSpotify => Core::TakeOverSpotify,
            Action::OpenSpotifyTab => Core::OpenSpotifyTab,
            Action::SetSpotifyTabEnabled { on } => Core::SetSpotifyTabEnabled(on),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[derive(Default)]
    struct Recorder {
        wakes: AtomicUsize,
        opened: Mutex<Vec<String>>,
    }

    impl Host for Recorder {
        fn wake(&self) {
            self.wakes.fetch_add(1, Ordering::Relaxed);
        }
        fn open(&self, url: String) {
            self.opened.lock().unwrap().push(url);
        }
    }

    /// A Spotless on empty folders of its own, signed out.
    fn spotless(name: &str) -> (Arc<Spotless>, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!("spotless-ffi-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spotless = Spotless::new(
            root.join("config").display().to_string(),
            root.join("cache").display().to_string(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        (spotless, root)
    }

    #[test]
    fn playlists_made_and_renamed_show_in_the_tabs() {
        let (spotless, root) = spotless("tabs");
        let tabs = spotless.tabs();
        assert_eq!(tabs.tabs.len(), 1);
        assert_eq!(tabs.viewed, 0);

        spotless.apply(Action::NewPlaylist);
        let made = spotless.tabs().viewed;
        assert_ne!(made, 0);
        spotless.apply(Action::RenamePlaylist {
            playlist: made,
            name: "Road trip".into(),
        });
        let names: Vec<_> = spotless.tabs().tabs.into_iter().map(|t| t.name).collect();
        assert_eq!(names.last().map(String::as_str), Some("Road trip"));
        let list = spotless.playlist(made).unwrap();
        assert!(list.songs.is_empty());
        assert_eq!(list.cursor, None);
        assert!(spotless.playlist(made + 1000).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn last_fm_shows_search_in_its_place_while_not_connected() {
        let (spotless, root) = spotless("lastfm-tab");
        spotless.apply(Action::ShowBrowseTab {
            tab: BrowseTab::LastFm,
        });
        let browse = spotless.browse();
        assert!(browse.open);
        assert_eq!(browse.shown, BrowseTab::Search);
        assert_eq!(browse.loved, None);
        assert_eq!(spotless.player().loved, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn open_browse_opens_and_stays_open() {
        let (spotless, root) = spotless("browse");
        assert!(!spotless.browse().open);
        spotless.apply(Action::OpenBrowse);
        spotless.apply(Action::OpenBrowse);
        let browse = spotless.browse();
        assert!(browse.open);
        assert_eq!(
            browse.page,
            BrowsePage::Home {
                searches: Vec::new(),
                recent: Vec::new()
            }
        );
        spotless.apply(Action::CloseBrowse);
        assert!(!spotless.browse().open);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn songs_added_from_browse_land_in_the_target() {
        let (spotless, root) = spotless("add");
        let track = Track {
            uri: "spotify:track:1".into(),
            title: "Roygbiv".into(),
            artists: vec![Artist {
                uri: "spotify:artist:1".into(),
                name: "Boards of Canada".into(),
                image: None,
            }],
            album: Album {
                uri: "spotify:album:1".into(),
                name: "Music Has the Right to Children".into(),
            },
            duration_ms: 151_000,
            source: None,
        };
        spotless.apply(Action::OpenBrowse);
        spotless.apply(Action::AddTracks {
            tracks: vec![track.clone(), track.clone()],
        });
        let list = spotless.playlist(0).unwrap();
        assert_eq!(list.songs.len(), 2);
        assert_eq!(list.songs[0].track, track);
        assert_ne!(list.songs[0].entry, list.songs[1].entry);
        assert_eq!(list.duration_ms, 302_000);
        assert_eq!(list.cursor, Some(list.songs[0].entry));
        let message = spotless.message().unwrap();
        assert!(!message.error);
        assert_eq!(message.text, "Added 2 songs to Default");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn your_spotify_lists_the_accounts_own_first_and_opens_one() {
        let root = std::env::temp_dir().join(format!("spotless-ffi-yours-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spotless = Spotless::demo(
            root.join("config").display().to_string(),
            root.join("cache").display().to_string(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        spotless.apply(Action::ShowBrowseTab {
            tab: BrowseTab::Spotify,
        });
        let browse = spotless.browse();
        assert!(browse.open);
        assert_eq!(browse.shown, BrowseTab::Spotify);
        let SpotifyPlaylists::Ready { playlists } = browse.your_spotify.playlists else {
            panic!("the demo's playlists are read");
        };
        let owned: Vec<bool> = playlists.iter().map(|p| p.owner.is_none()).collect();
        assert_eq!(owned, [true, true, true, true, true, false, false]);
        assert!(playlists[0].liked && !playlists[1].liked);

        let names: Vec<&str> = playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names[..3], ["Liked Songs", "Basement Tapes", "Late Trains"]);

        let late_trains = playlists[2].uri.clone();
        spotless.apply(Action::OpenSpotifyPlaylist {
            uri: late_trains.clone(),
        });
        let yours = spotless.browse().your_spotify;
        assert_eq!(yours.open, Some(late_trains));
        assert!(matches!(yours.songs, AlbumSongs::Ready { tracks, .. } if tracks.len() == 12));
        assert_eq!(yours.undo, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn songs_added_from_a_spotify_playlist_sit_under_its_name() {
        let root =
            std::env::temp_dir().join(format!("spotless-ffi-headers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spotless = Spotless::demo(
            root.join("config").display().to_string(),
            root.join("cache").display().to_string(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        spotless.apply(Action::ShowBrowseTab {
            tab: BrowseTab::Spotify,
        });
        let SpotifyPlaylists::Ready { playlists } = spotless.browse().your_spotify.playlists else {
            panic!("the demo's playlists are read");
        };
        let late_trains = playlists
            .into_iter()
            .find(|p| p.name == "Late Trains")
            .unwrap();
        spotless.apply(Action::OpenSpotifyPlaylist {
            uri: late_trains.uri.clone(),
        });
        let AlbumSongs::Ready { tracks, .. } = spotless.browse().your_spotify.songs else {
            panic!("the demo's songs are read");
        };
        let from = Source {
            uri: late_trains.uri.clone(),
            name: late_trains.name.clone(),
        };
        assert!(tracks.iter().all(|t| t.source.as_ref() == Some(&from)));
        assert_eq!(
            spotless.settings().song_headers,
            "source",
            "by source to start with"
        );

        let id = spotless.tabs().viewed;
        let before = spotless.playlist(id).unwrap().songs.len();
        spotless.apply(Action::AddTracks {
            tracks: tracks[..3].to_vec(),
        });
        let songs = spotless.playlist(id).unwrap().songs;
        let header = songs[before].header.clone().expect("a header over them");
        assert_eq!(header.songs, 3);
        assert_eq!(header.name, late_trains.name);
        assert_eq!(header.detail.as_deref(), Some("Spotify playlist"));
        assert_eq!(songs[before].track.source, Some(from));

        spotless.apply(Action::SetGrouping { key: "none".into() });
        assert_eq!(spotless.settings().song_headers, "none");
        let songs = spotless.playlist(id).unwrap().songs;
        assert!(songs.iter().all(|song| song.header.is_none()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn songs_move_to_another_playlist_or_a_new_one() {
        let (spotless, root) = spotless("move");
        let track = |n: u32| Track {
            uri: format!("spotify:track:{n}"),
            title: format!("Song {n}"),
            artists: Vec::new(),
            album: Album {
                uri: "spotify:album:1".into(),
                name: "Album".into(),
            },
            duration_ms: 1000,
            source: None,
        };
        spotless.apply(Action::OpenBrowse);
        spotless.apply(Action::AddTracks {
            tracks: (1..=4).map(track).collect(),
        });
        spotless.apply(Action::NewPlaylist);
        let other = spotless.tabs().viewed;
        let entries: Vec<u64> = spotless.playlist(0).unwrap().songs[..2]
            .iter()
            .map(|song| song.entry)
            .collect();
        spotless.apply(Action::MoveTo {
            from: 0,
            to: other,
            entries,
        });
        let titles = |id| -> Vec<String> {
            spotless
                .playlist(id)
                .unwrap()
                .songs
                .into_iter()
                .map(|s| s.track.title)
                .collect()
        };
        assert_eq!(titles(0), ["Song 3", "Song 4"]);
        assert_eq!(titles(other), ["Song 1", "Song 2"]);

        spotless.apply(Action::NewPlaylistWith {
            tracks: vec![track(3)],
        });
        let made = spotless.tabs().viewed;
        assert!(made != 0 && made != other);
        assert_eq!(titles(made), ["Song 3"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stop_after_shows_while_a_song_plays_and_turns_off_again() {
        let root = std::env::temp_dir().join(format!("spotless-ffi-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spotless = Spotless::demo(
            root.join("config").display().to_string(),
            root.join("cache").display().to_string(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        assert!(!spotless.player().stop_after);
        spotless.apply(Action::ToggleStopAfter);
        assert!(spotless.player().stop_after);
        spotless.apply(Action::ToggleStopAfter);
        assert!(!spotless.player().stop_after);
        assert!(!spotless.settings().time_left);
        spotless.apply(Action::ToggleTimeLeft);
        assert!(spotless.settings().time_left);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn changes_are_saved_once_they_rest() {
        let (spotless, root) = spotless("save");
        assert_eq!(spotless.update(), None);
        spotless.apply(Action::NewPlaylist);
        assert!(spotless.update().is_some_and(|ms| ms <= 1000));
        spotless.save();
        assert_eq!(spotless.update(), None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_find_narrows_an_artist_page_to_its_releases_and_songs_with_it() {
        let root = std::env::temp_dir().join(format!("spotless-ffi-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let spotless = Spotless::demo(
            root.join("config").display().to_string(),
            root.join("cache").display().to_string(),
            Arc::new(Recorder::default()),
        )
        .unwrap();
        spotless.apply(Action::OpenBrowse);
        spotless.apply(Action::OpenArtist {
            artist: (&spotless_core::demo::artist()).into(),
        });
        spotless.apply(Action::Find {
            text: Some("STATIC".into()),
        });
        let browse = spotless.browse();
        assert_eq!(browse.find.as_deref(), Some("STATIC"));
        let BrowsePage::Artist {
            page:
                ArtistPage::Ready {
                    releases,
                    unread,
                    popular,
                    popular_ranks,
                    add_all,
                    ..
                },
            ..
        } = browse.page
        else {
            panic!("the demo artist's page");
        };
        assert_eq!(unread, 0);
        // The popular songs too, each with its rank among all of them.
        let popular: Vec<&str> = popular.iter().map(|t| t.title.as_str()).collect();
        assert_eq!((popular, popular_ranks), (vec!["Static Rain"], vec![1]));
        // ADD ALL: the song found, Everything Is Static having none here.
        assert_eq!(add_all, AddAll::Songs { count: 1 });
        let found: Vec<(&str, bool)> = (releases.iter())
            .map(|row| (row.release.title.as_str(), row.expanded))
            .collect();
        // One by a song, open; one by its title.
        assert_eq!(
            found,
            [
                ("Weather for Machines", true),
                ("Everything Is Static", false)
            ]
        );
        let AlbumSongs::Ready { tracks, numbers } = &releases[0].songs else {
            panic!("its songs are read");
        };
        let titles: Vec<&str> = tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!((titles, numbers.clone()), (vec!["Static Rain"], vec![4]));
        spotless.apply(Action::Find { text: None });
        assert_eq!(spotless.browse().find, None);

        // POPULAR shows in place of the releases; ADD ALL adds its songs.
        spotless.apply(Action::ShowPopular);
        let browse = spotless.browse();
        assert!(browse.popular);
        let BrowsePage::Artist {
            page: ArtistPage::Ready { add_all, .. },
            ..
        } = browse.page
        else {
            panic!("the demo artist's page");
        };
        assert_eq!(add_all, AddAll::Songs { count: 5 });
        let id = spotless.tabs().viewed;
        let before = spotless.playlist(id).unwrap().songs.len();
        spotless.apply(Action::AddAll);
        assert_eq!(spotless.playlist(id).unwrap().songs.len(), before + 5);
        let _ = std::fs::remove_dir_all(root);
    }
}
