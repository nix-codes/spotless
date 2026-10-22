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

#[derive(uniffi::Object)]
pub struct Spotless {
    controller: Mutex<Controller>,
    /// Covers read since the app last took them.
    covers: Mutex<Vec<Cover>>,
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
        self.lock().send(Command::Cover(url));
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
    /// Saves this Last.fm API account and opens its approval page.
    ConnectLastFm {
        api_key: String,
        secret: String,
    },
    DisconnectLastFm,
    /// From the next start.
    SetAudioCache {
        on: bool,
    },
    /// The audio cache's limit in MB, from the next start.
    SetAudioCacheSize {
        mb: u64,
    },
    MeasureStorage,
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
    SetTab {
        tab: ReleaseTab,
    },
    /// Shows artists like this one (the artist page's), from Last.fm.
    ShowSimilar {
        artist: Artist,
    },
    /// Opens a similar artist, by name, finding it on Spotify first.
    OpenSimilar {
        name: String,
    },
    ToggleSort,
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
}

impl From<Action> for spotless_core::action::Action {
    fn from(action: Action) -> Self {
        use spotless_core::action::Action as Core;
        match action {
            Action::SignIn => Core::SignIn,
            Action::SignOut => Core::SignOut,
            Action::SetSearchApp { client_id } => Core::SetSearchApp(client_id),
            Action::SetLevelling { on } => Core::SetLevelling(on),
            Action::ConnectLastFm { api_key, secret } => Core::ConnectLastFm(api_key, secret),
            Action::DisconnectLastFm => Core::DisconnectLastFm,
            Action::SetAudioCache { on } => Core::SetAudioCache(on),
            Action::SetAudioCacheSize { mb } => Core::SetAudioCacheSize(mb),
            Action::MeasureStorage => Core::MeasureStorage,
            Action::ClearCache { store } => Core::ClearCache(store.into()),
            Action::AddLink { link } => Core::AddLink(link),
            Action::Play => Core::Play,
            Action::TogglePause => Core::TogglePause,
            Action::Stop => Core::Stop,
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
            Action::ForgetSearch { query } => Core::ForgetSearch(query),
            Action::OpenArtist { artist } => Core::OpenArtist(artist.into()),
            Action::ToggleAlbum { album } => Core::ToggleAlbum(album),
            Action::SetTab { tab } => Core::SetTab(tab.into()),
            Action::ShowSimilar { artist } => Core::ShowSimilar(artist.into()),
            Action::OpenSimilar { name } => Core::OpenSimilar(name),
            Action::ToggleSort => Core::ToggleSort,
            Action::SetTarget { playlist } => Core::SetTarget(PlaylistId(playlist)),
            Action::ToggleCovers => Core::ToggleCovers,
            Action::AddTracks { tracks } => {
                Core::AddTracks(tracks.into_iter().map(Into::into).collect())
            }
            Action::AddAlbum { album } => Core::AddAlbum(album),
            Action::PlayNow { track } => Core::PlayNow(track.into()),
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
    fn changes_are_saved_once_they_rest() {
        let (spotless, root) = spotless("save");
        assert_eq!(spotless.update(), None);
        spotless.apply(Action::NewPlaylist);
        assert!(spotless.update().is_some_and(|ms| ms <= 1000));
        spotless.save();
        assert_eq!(spotless.update(), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
