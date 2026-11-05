//! Spotless's core for the Android app, through UniFFI. Kotlin makes one
//! [`Spotless`], hands it [`Action`]s, and reads what the screens show from
//! it whenever its [`Host`] is woken.
//!
//! The types here are the core's, flattened into plain records the
//! bindings can carry; `view.rs` makes them.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use spotless_core::Controller;
use spotless_core::Paths;
use spotless_core::model::{EntryId, PlaylistId};

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
        controller.apply(spotless_core::action::Action::from(action));
    }

    /// Takes in the core's news, and saves once changes have rested. Returns
    /// the milliseconds after which to call it again, while a save is due.
    pub fn update(&self) -> Option<u64> {
        let mut controller = self.lock();
        let arrived = controller.receive().into_iter().map(|read| Cover {
            url: read.url,
            image: read.image.ok().map(|image| CoverImage {
                width: image.width,
                height: image.height,
                rgba: image.rgba,
            }),
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
            return self.lock().load_cover(url);
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
    /// Shows or hides the equaliser above the display (EQ).
    ToggleEqualiser,
    /// The equaliser's switch, preamp and bands, as they are now.
    SetEqualiser {
        equaliser: Equaliser,
    },
    /// The lists and the display in this colour, by its key.
    SetPhosphor {
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
    /// Sorts or shuffles a playlist, once. UndoReorder puts it back.
    Reorder {
        playlist: u64,
        order: SortOrder,
    },
    /// Puts back the order from before the last Reorder, while nothing has
    /// changed its songs since (Tabs::reorder_undone).
    UndoReorder,
    /// Switches a playlist's headers on or off.
    SetHeaders {
        playlist: u64,
        on: bool,
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
        use spotless_core::action::{
            BrowseAction, LastFmAction, PlaybackAction, PlaylistsAction, SettingsAction,
            SpotifyListsAction, SpotifyTabAction, SyncAction, ViewAction,
        };
        match action {
            Action::SignIn => SettingsAction::SignIn.into(),
            Action::SignOut => SettingsAction::SignOut.into(),
            Action::SetSearchApp { client_id } => SettingsAction::SetSearchApp(client_id).into(),
            Action::SetLevelling { on } => SettingsAction::SetLevelling(on).into(),
            Action::ToggleEqualiser => ViewAction::ToggleEqualiser.into(),
            Action::SetEqualiser { equaliser } => ViewAction::SetEqualiser(equaliser.into()).into(),
            Action::SetPhosphor { key } => {
                SettingsAction::SetPhosphor(spotless_core::settings::Phosphor::from_key(Some(&key)))
                    .into()
            }
            Action::ConnectLastFm { api_key, secret } => {
                LastFmAction::ConnectLastFm(api_key, secret).into()
            }
            Action::DisconnectLastFm => LastFmAction::DisconnectLastFm.into(),
            Action::SetLoved {
                artist,
                title,
                loved,
            } => LastFmAction::SetLoved {
                artist,
                title,
                loved,
            }
            .into(),
            Action::SetAudioCache { on } => SettingsAction::SetAudioCache(on).into(),
            Action::SetAudioCacheSize { mb } => SettingsAction::SetAudioCacheSize(mb).into(),
            Action::MeasureStorage => SettingsAction::MeasureStorage.into(),
            Action::NetworkBack => SettingsAction::NetworkBack.into(),
            Action::ClearCache { store } => SettingsAction::ClearCache(store.into()).into(),
            Action::AddLink { link } => PlaylistsAction::AddLink(link).into(),
            Action::Play => PlaybackAction::Play.into(),
            Action::TogglePause => PlaybackAction::TogglePause.into(),
            Action::Stop => PlaybackAction::Stop.into(),
            Action::ToggleStopAfter => PlaybackAction::ToggleStopAfter.into(),
            Action::ToggleTimeLeft => ViewAction::ToggleTimeLeft.into(),
            Action::Release => PlaybackAction::Release.into(),
            Action::Next => PlaybackAction::Next.into(),
            Action::Previous => PlaybackAction::Previous.into(),
            Action::Seek { position_ms } => PlaybackAction::Seek(position_ms).into(),
            Action::Volume { percent } => PlaybackAction::Volume(percent).into(),
            Action::SetShuffle { on } => PlaybackAction::SetShuffle(on).into(),
            Action::SetRepeat { on } => PlaybackAction::SetRepeat(on).into(),
            Action::PlayEntry { playlist, entry } => {
                PlaybackAction::PlayEntry(PlaylistId(playlist), EntryId(entry)).into()
            }
            Action::Remove {
                playlist,
                entries: ids,
            } => PlaylistsAction::Remove(PlaylistId(playlist), entries(ids)).into(),
            Action::Move {
                entries: ids,
                before,
            } => PlaylistsAction::Move(entries(ids), before as usize).into(),
            Action::Reorder { playlist, order } => {
                PlaylistsAction::Reorder(PlaylistId(playlist), order.into()).into()
            }
            Action::UndoReorder => PlaylistsAction::UndoReorder.into(),
            Action::SetHeaders { playlist, on } => {
                PlaylistsAction::SetHeaders(PlaylistId(playlist), on).into()
            }
            Action::ToggleMarks { entries: ids } => {
                PlaylistsAction::ToggleMarks(entries(ids)).into()
            }
            Action::SelectPlaylist { playlist } => {
                PlaylistsAction::SelectPlaylist(PlaylistId(playlist)).into()
            }
            Action::NewPlaylist => PlaylistsAction::NewPlaylist.into(),
            Action::NewPlaylistWith { tracks } => {
                PlaylistsAction::NewPlaylistWith(tracks.into_iter().map(Into::into).collect())
                    .into()
            }
            Action::ClosePlaylist { playlist } => {
                PlaylistsAction::ClosePlaylist(PlaylistId(playlist)).into()
            }
            Action::RenamePlaylist { playlist, name } => {
                PlaylistsAction::RenamePlaylist(PlaylistId(playlist), name).into()
            }
            Action::MovePlaylist { playlist, index } => {
                PlaylistsAction::MovePlaylist(PlaylistId(playlist), index as usize).into()
            }
            Action::OpenBrowse => BrowseAction::ToggleBrowse.into(),
            Action::CloseBrowse => BrowseAction::CloseBrowse.into(),
            Action::Back => BrowseAction::Back.into(),
            Action::Search { query } => BrowseAction::Search(query).into(),
            Action::MoreSongs { query } => BrowseAction::MoreSongs(query).into(),
            Action::Find { text } => BrowseAction::Find(text).into(),
            Action::ForgetSearch { query } => BrowseAction::ForgetSearch(query).into(),
            Action::OpenArtist { artist } => BrowseAction::OpenArtist(artist.into()).into(),
            Action::ToggleAlbum { album } => BrowseAction::ToggleAlbum(album).into(),
            Action::ExpandAlbums { albums } => BrowseAction::ExpandAlbums(albums).into(),
            Action::SetTab { tab } => BrowseAction::SetTab(tab.into()).into(),
            Action::ShowSimilar { artist } => BrowseAction::ShowSimilar(artist.into()).into(),
            Action::ShowPopular => BrowseAction::ShowPopular.into(),
            Action::AddAll => BrowseAction::AddAll.into(),
            Action::OpenSimilar { name } => BrowseAction::OpenSimilar(name).into(),
            Action::ToggleSort => BrowseAction::ToggleSort.into(),
            Action::ShowBrowseTab { tab } => BrowseAction::ShowBrowseTab(tab.into()).into(),
            Action::ReadSpotifyPlaylists => SpotifyListsAction::ReadSpotifyPlaylists.into(),
            Action::OpenSpotifyPlaylist { uri } => {
                SpotifyListsAction::OpenSpotifyPlaylist(uri).into()
            }
            Action::RemoveFromSpotify { uri, rows } => SpotifyListsAction::RemoveFromSpotify(
                uri,
                rows.into_iter().map(|row| row as usize).collect(),
            )
            .into(),
            Action::UnlistSpotifyPlaylist { uri } => {
                SpotifyListsAction::UnlistSpotifyPlaylist(uri).into()
            }
            Action::UndoSpotifyRemoval => SpotifyListsAction::UndoSpotifyRemoval.into(),
            Action::SetSync { playlist, on } => {
                SyncAction::SetSync(PlaylistId(playlist), on).into()
            }
            Action::ReplaceSynced { playlist } => {
                SyncAction::ReplaceSynced(PlaylistId(playlist)).into()
            }
            Action::DismissSyncNameTaken => SyncAction::DismissSyncNameTaken.into(),
            Action::OpenSynced { uri, name } => SyncAction::OpenSynced(uri, name).into(),
            Action::CopyToSpotify { target, tracks } => SpotifyListsAction::CopyToSpotify(
                target.into(),
                tracks.into_iter().map(Into::into).collect(),
            )
            .into(),
            Action::SetTarget { playlist } => BrowseAction::SetTarget(PlaylistId(playlist)).into(),
            Action::ToggleCovers => BrowseAction::ToggleCovers.into(),
            Action::AddTracks { tracks } => {
                BrowseAction::AddTracks(tracks.into_iter().map(Into::into).collect()).into()
            }
            Action::AddAlbum { album } => BrowseAction::AddAlbum(album).into(),
            Action::PlayNow { track } => PlaybackAction::PlayNow(track.into()).into(),
            Action::AddLoved { songs } => {
                LastFmAction::AddLoved(songs.into_iter().map(Into::into).collect()).into()
            }
            Action::PlayLoved { song } => LastFmAction::PlayLoved(song.into()).into(),
            Action::CopyTo { playlist, tracks } => PlaylistsAction::CopyTo(
                PlaylistId(playlist),
                tracks.into_iter().map(Into::into).collect(),
            )
            .into(),
            Action::ClearSpotify => SpotifyTabAction::ClearSpotify.into(),
            Action::MoveTo {
                from,
                to,
                entries: ids,
            } => PlaylistsAction::MoveTo(PlaylistId(from), PlaylistId(to), entries(ids)).into(),
            Action::TakeOverSpotify => SpotifyTabAction::TakeOverSpotify.into(),
            Action::OpenSpotifyTab => SpotifyTabAction::OpenSpotifyTab.into(),
            Action::SetSpotifyTabEnabled { on } => {
                SpotifyTabAction::SetSpotifyTabEnabled(on).into()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Whether the app has a way to ask for this action of the core's. Every
    /// one is named here, so an action new to the core doesn't compile (in
    /// the tests, as CI builds them) until it's given an [`Action`] or put
    /// among those the app doesn't have yet: new features come to the
    /// desktop first.
    fn on_android(action: &spotless_core::action::Action) -> bool {
        use spotless_core::action::{
            Action as Core, BrowseAction, LastFmAction, PlaybackAction, PlaylistsAction,
            SettingsAction, SpotifyListsAction, SpotifyTabAction, SyncAction, ViewAction,
        };
        match action {
            Core::Settings(action) => match action {
                SettingsAction::SignIn
                | SettingsAction::SignOut
                | SettingsAction::SetSearchApp(..)
                | SettingsAction::SetLevelling(..)
                | SettingsAction::SetPhosphor(..)
                | SettingsAction::SetAudioCache(..)
                | SettingsAction::SetAudioCacheSize(..)
                | SettingsAction::MeasureStorage
                | SettingsAction::NetworkBack
                | SettingsAction::ClearCache(..) => true,
                // Not in the app yet.
                SettingsAction::ListOutputs | SettingsAction::SetOutput(..) => false,
            },
            Core::LastFm(action) => match action {
                LastFmAction::ConnectLastFm(..)
                | LastFmAction::DisconnectLastFm
                | LastFmAction::SetLoved { .. }
                | LastFmAction::AddLoved(..)
                | LastFmAction::PlayLoved(..) => true,
                // Not in the app yet.
                LastFmAction::DropLoved(..) => false,
            },
            Core::Playback(action) => match action {
                PlaybackAction::Play
                | PlaybackAction::TogglePause
                | PlaybackAction::Stop
                | PlaybackAction::ToggleStopAfter
                | PlaybackAction::Release
                | PlaybackAction::Next
                | PlaybackAction::Previous
                | PlaybackAction::Seek(..)
                | PlaybackAction::Volume(..)
                | PlaybackAction::SetShuffle(..)
                | PlaybackAction::SetRepeat(..)
                | PlaybackAction::PlayEntry(..)
                | PlaybackAction::PlayNow(..) => true,
            },
            Core::Playlists(action) => match action {
                PlaylistsAction::AddLink(..)
                | PlaylistsAction::Remove(..)
                | PlaylistsAction::Move(..)
                | PlaylistsAction::ToggleMarks(..)
                | PlaylistsAction::SelectPlaylist(..)
                | PlaylistsAction::NewPlaylist
                | PlaylistsAction::NewPlaylistWith(..)
                | PlaylistsAction::ClosePlaylist(..)
                | PlaylistsAction::RenamePlaylist(..)
                | PlaylistsAction::MovePlaylist(..)
                | PlaylistsAction::CopyTo(..)
                | PlaylistsAction::MoveTo(..)
                | PlaylistsAction::Reorder(..)
                | PlaylistsAction::UndoReorder
                | PlaylistsAction::SetHeaders(..) => true,
            },
            Core::Sync(action) => match action {
                SyncAction::SetSync(..)
                | SyncAction::ReplaceSynced(..)
                | SyncAction::DismissSyncNameTaken
                | SyncAction::OpenSynced(..) => true,
            },
            Core::View(action) => match action {
                ViewAction::ToggleEqualiser
                | ViewAction::SetEqualiser(..)
                | ViewAction::ToggleTimeLeft => true,
                // Not in the app yet.
                ViewAction::TogglePlaylists
                | ViewAction::SaveEqualiserPreset(..)
                | ViewAction::DeleteEqualiserPreset(..)
                | ViewAction::ToggleDeck
                | ViewAction::SetDeckMedia(..)
                | ViewAction::SetDeckMeter(..)
                | ViewAction::SetVuFace(..)
                | ViewAction::SetVuCalibration(..)
                | ViewAction::SetCassetteMix(..)
                | ViewAction::SetVinylLp(..)
                | ViewAction::SetReelSlow(..)
                | ViewAction::SetCdMini(..) => false,
            },
            Core::Browse(action) => match action {
                BrowseAction::ToggleBrowse
                | BrowseAction::CloseBrowse
                | BrowseAction::Back
                | BrowseAction::Search(..)
                | BrowseAction::MoreSongs(..)
                | BrowseAction::Find(..)
                | BrowseAction::ForgetSearch(..)
                | BrowseAction::OpenArtist(..)
                | BrowseAction::ToggleAlbum(..)
                | BrowseAction::ExpandAlbums(..)
                | BrowseAction::SetTab(..)
                | BrowseAction::ShowSimilar(..)
                | BrowseAction::ShowPopular
                | BrowseAction::OpenSimilar(..)
                | BrowseAction::ToggleSort
                | BrowseAction::SetTarget(..)
                | BrowseAction::ToggleCovers
                | BrowseAction::AddTracks(..)
                | BrowseAction::AddAlbum(..)
                | BrowseAction::AddAll
                | BrowseAction::ShowBrowseTab(..) => true,
                // Not in the app yet.
                BrowseAction::MoreArtists(..)
                | BrowseAction::AddAlbums(..)
                | BrowseAction::DropTracks(..)
                | BrowseAction::DropAlbum(..) => false,
            },
            Core::SpotifyLists(action) => match action {
                SpotifyListsAction::OpenSpotifyPlaylist(..)
                | SpotifyListsAction::RemoveFromSpotify(..)
                | SpotifyListsAction::UnlistSpotifyPlaylist(..)
                | SpotifyListsAction::UndoSpotifyRemoval
                | SpotifyListsAction::ReadSpotifyPlaylists
                | SpotifyListsAction::CopyToSpotify(..) => true,
            },
            Core::SpotifyTab(action) => match action {
                SpotifyTabAction::ClearSpotify
                | SpotifyTabAction::TakeOverSpotify
                | SpotifyTabAction::OpenSpotifyTab
                | SpotifyTabAction::SetSpotifyTabEnabled(..) => true,
            },
        }
    }

    fn artist() -> Artist {
        Artist {
            uri: "spotify:artist:a".into(),
            name: "An Artist".into(),
            image: None,
        }
    }

    fn track() -> Track {
        Track {
            uri: "spotify:track:t".into(),
            title: "A Song".into(),
            artists: vec![artist()],
            album: Album {
                uri: "spotify:album:b".into(),
                name: "An Album".into(),
            },
            duration_ms: 1000,
            source: None,
        }
    }

    fn loved() -> LovedSong {
        LovedSong {
            artist: "An Artist".into(),
            title: "A Song".into(),
            date: 0,
        }
    }

    #[test]
    fn every_action_of_the_apps_is_one_the_core_has_for_android() {
        let examples = [
            Action::SignIn,
            Action::SignOut,
            Action::SetSearchApp {
                client_id: "x".into(),
            },
            Action::SetLevelling { on: true },
            Action::ToggleEqualiser,
            Action::SetEqualiser {
                equaliser: Equaliser {
                    on: true,
                    preamp: 0.0,
                    bands: vec![0.0; 10],
                },
            },
            Action::SetPhosphor { key: "x".into() },
            Action::ConnectLastFm {
                api_key: "x".into(),
                secret: "x".into(),
            },
            Action::DisconnectLastFm,
            Action::SetLoved {
                artist: "x".into(),
                title: "x".into(),
                loved: true,
            },
            Action::SetAudioCache { on: true },
            Action::SetAudioCacheSize { mb: 1 },
            Action::MeasureStorage,
            Action::NetworkBack,
            Action::ClearCache {
                store: Store::Audio,
            },
            Action::AddLink { link: "x".into() },
            Action::Play,
            Action::TogglePause,
            Action::Stop,
            Action::ToggleStopAfter,
            Action::ToggleTimeLeft,
            Action::Release,
            Action::Next,
            Action::Previous,
            Action::Seek { position_ms: 1 },
            Action::Volume { percent: 50 },
            Action::SetShuffle { on: true },
            Action::SetRepeat { on: true },
            Action::PlayEntry {
                playlist: 1,
                entry: 1,
            },
            Action::Remove {
                playlist: 1,
                entries: vec![1],
            },
            Action::Move {
                entries: vec![1],
                before: 1,
            },
            Action::ToggleMarks { entries: vec![1] },
            Action::SelectPlaylist { playlist: 1 },
            Action::NewPlaylist,
            Action::NewPlaylistWith {
                tracks: vec![track()],
            },
            Action::ClosePlaylist { playlist: 1 },
            Action::RenamePlaylist {
                playlist: 1,
                name: "x".into(),
            },
            Action::MovePlaylist {
                playlist: 1,
                index: 1,
            },
            Action::OpenBrowse,
            Action::CloseBrowse,
            Action::Back,
            Action::Search { query: "x".into() },
            Action::MoreSongs { query: "x".into() },
            Action::Find { text: None },
            Action::ForgetSearch { query: None },
            Action::OpenArtist { artist: artist() },
            Action::ToggleAlbum { album: "x".into() },
            Action::ExpandAlbums {
                albums: vec!["x".into()],
            },
            Action::SetTab {
                tab: ReleaseTab::All,
            },
            Action::ShowSimilar { artist: artist() },
            Action::ShowPopular,
            Action::AddAll,
            Action::OpenSimilar { name: "x".into() },
            Action::ToggleSort,
            Action::ShowBrowseTab {
                tab: BrowseTab::Search,
            },
            Action::ReadSpotifyPlaylists,
            Action::OpenSpotifyPlaylist { uri: "x".into() },
            Action::RemoveFromSpotify {
                uri: "x".into(),
                rows: vec![0],
            },
            Action::UnlistSpotifyPlaylist { uri: "x".into() },
            Action::UndoSpotifyRemoval,
            Action::Reorder {
                playlist: 1,
                order: SortOrder::Reversed,
            },
            Action::UndoReorder,
            Action::SetHeaders {
                playlist: 1,
                on: false,
            },
            Action::SetSync {
                playlist: 1,
                on: true,
            },
            Action::ReplaceSynced { playlist: 1 },
            Action::DismissSyncNameTaken,
            Action::OpenSynced {
                uri: "x".into(),
                name: "x".into(),
            },
            Action::CopyToSpotify {
                target: SpotifyTarget::New {
                    name: "x".into(),
                    private: false,
                },
                tracks: vec![track()],
            },
            Action::SetTarget { playlist: 1 },
            Action::ToggleCovers,
            Action::AddTracks {
                tracks: vec![track()],
            },
            Action::AddAlbum { album: "x".into() },
            Action::PlayNow { track: track() },
            Action::AddLoved {
                songs: vec![loved()],
            },
            Action::PlayLoved { song: loved() },
            Action::CopyTo {
                playlist: 1,
                tracks: vec![track()],
            },
            Action::ClearSpotify,
            Action::MoveTo {
                from: 1,
                to: 1,
                entries: vec![1],
            },
            Action::TakeOverSpotify,
            Action::OpenSpotifyTab,
            Action::SetSpotifyTabEnabled { on: true },
        ];
        for action in examples {
            let core = spotless_core::action::Action::from(action.clone());
            assert!(on_android(&core), "{action:?} is {core:?}, not on Android");
        }
    }

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
    fn sort_gathers_artists_under_headers_and_undo_puts_the_order_back() {
        let (spotless, root) = spotless("sort");
        let track = |n: u32, artist: &str| Track {
            uri: format!("spotify:track:{n}"),
            title: format!("Song {n}"),
            artists: vec![Artist {
                uri: format!("spotify:artist:{artist}"),
                name: artist.into(),
                image: None,
            }],
            album: Album {
                uri: format!("spotify:album:{artist}"),
                name: format!("{artist}'s Album"),
            },
            duration_ms: 1_000,
            source: None,
        };
        spotless.apply(Action::OpenBrowse);
        spotless.apply(Action::AddTracks {
            tracks: vec![track(1, "Zed"), track(2, "Abba"), track(3, "Abba")],
        });
        let titles = |spotless: &Spotless| -> Vec<String> {
            let list = spotless.playlist(0).unwrap();
            list.songs
                .iter()
                .map(|song| song.track.title.clone())
                .collect()
        };
        assert_eq!(spotless.tabs().reorder_undone, None);

        spotless.apply(Action::Reorder {
            playlist: 0,
            order: SortOrder::ArtistAlbums,
        });
        assert_eq!(titles(&spotless), ["Song 2", "Song 3", "Song 1"]);
        let songs = spotless.playlist(0).unwrap().songs;
        let header = songs[0].header.clone().expect("Abba's header");
        assert_eq!((header.name.as_str(), header.songs), ("Abba", 2));
        assert!(songs[0].under && songs[1].under && !songs[2].under);
        assert!(songs[2].ends_run, "Zed's lone song ends Abba's run");
        assert_eq!(songs[0].album.as_deref(), Some("Abba's Album"));
        assert_eq!(spotless.tabs().reorder_undone, Some(0));
        assert!(
            spotless
                .message()
                .unwrap()
                .text
                .starts_with("Sorted by artist")
        );

        spotless.apply(Action::UndoReorder);
        assert_eq!(titles(&spotless), ["Song 1", "Song 2", "Song 3"]);
        assert_eq!(spotless.tabs().reorder_undone, None);
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
    fn songs_added_from_a_spotify_playlist_sit_under_its_name_until_headers_are_off() {
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

        assert!(songs[before..before + 3].iter().all(|song| song.under));

        spotless.apply(Action::SetHeaders {
            playlist: id,
            on: false,
        });
        let view = spotless.playlist(id).unwrap();
        assert!(!view.headers_on);
        assert!(
            view.songs
                .iter()
                .all(|song| song.header.is_none() && !song.under)
        );
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
