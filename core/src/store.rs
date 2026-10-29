//! The playlists, settings and Browse's history on disk, in the config
//! folder.
//!
//! The files are written whole to a temporary file and renamed over the
//! old one, so a crash mid-write never leaves half a file. A file that can't
//! be read is moved aside as `*.bad` rather than overwritten.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{ArtistRef, Grouping, Library, PlaylistId};
use crate::paths::Paths;
use crate::phosphor::Phosphor;
use crate::playback::equaliser::{Equaliser, Preset};

const LIBRARY_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    #[serde(flatten)]
    library: Library,
    /// The playlist Next and Previous act on.
    playing: Option<PlaylistId>,
    /// The tab shown.
    #[serde(default)]
    viewed: Option<PlaylistId>,
    /// Where the Spotify tab stands among the tabs.
    #[serde(default)]
    spotify_at: usize,
}

/// What `playlists.json` holds besides the playlists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tabs {
    pub playing: Option<PlaylistId>,
    pub viewed: Option<PlaylistId>,
    /// Where the Spotify tab stands among the tabs.
    pub spotify_at: usize,
}

/// Where the desktop's window was and how big, to open it there again.
/// In points at a zoom of 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    /// The window's outer top left corner; None where the desktop doesn't
    /// tell (Wayland).
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    /// Its content's size, as tall as it is with the playlists showing.
    pub size: [f32; 2],
    #[serde(default)]
    pub maximized: bool,
}

/// What the desktop's deck plays the music on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckMedia {
    /// A cassette in a Walkman.
    #[default]
    Cassette,
    /// Open reels on a reel-to-reel deck.
    Reel,
    /// A disc in a portable CD player.
    Cd,
    /// A 7" single on a turntable.
    Vinyl,
}

impl DeckMedia {
    pub fn key(self) -> &'static str {
        match self {
            Self::Cassette => "cassette",
            Self::Reel => "reel",
            Self::Cd => "cd",
            Self::Vinyl => "vinyl",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("reel") => Self::Reel,
            Some("cd") => Self::Cd,
            Some("vinyl") => Self::Vinyl,
            _ => Self::Cassette,
        }
    }

    /// The next one, which clicking the deck switches to.
    pub fn next(self) -> Self {
        match self {
            Self::Cassette => Self::Reel,
            Self::Reel => Self::Cd,
            Self::Cd => Self::Vinyl,
            Self::Vinyl => Self::Cassette,
        }
    }
}

/// The saved playlists, or a fresh library with only Default.
pub fn load_library(paths: &Paths) -> (Library, Tabs) {
    let path = paths.playlists();
    match read_library(&path) {
        Ok(Some(loaded)) => loaded,
        Ok(None) => (Library::new(), Tabs::default()),
        Err(error) => {
            log::error!("unable to read {}: {error:#}", path.display());
            move_aside(&path);
            (Library::new(), Tabs::default())
        }
    }
}

fn read_library(path: &Path) -> Result<Option<(Library, Tabs)>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut file: LibraryFile = serde_json::from_slice(&bytes)?;
    if file.version > LIBRARY_VERSION {
        bail!("written by a newer Spotless (version {})", file.version);
    }
    file.library.repair();
    let exists = |id: &PlaylistId| file.library.get(*id).is_some();
    let tabs = Tabs {
        playing: file.playing.filter(exists),
        viewed: file.viewed.filter(exists),
        spotify_at: file.spotify_at,
    };
    Ok(Some((file.library, tabs)))
}

pub fn save_library(paths: &Paths, library: &Library, tabs: Tabs) -> Result<()> {
    let file = LibraryFile {
        version: LIBRARY_VERSION,
        library: library.clone(),
        playing: tabs.playing,
        viewed: tabs.viewed,
        spotify_at: tabs.spotify_at,
    };
    write_atomically(&paths.playlists(), &serde_json::to_vec(&file)?)
}

/// What Browse's home page lists: recent searches and artists, newest
/// first; and where Browse was left.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    #[serde(default)]
    pub searches: Vec<String>,
    #[serde(default)]
    pub artists: Vec<ArtistRef>,
    #[serde(default)]
    pub tab: crate::browse::BrowseTab,
    /// The Spotify playlist YOUR SPOTIFY showed.
    #[serde(default)]
    pub spotify_playlist: Option<String>,
}

/// The saved history, or an empty one.
pub fn load_history(paths: &Paths) -> History {
    let path = paths.history();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(_) => return History::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        log::error!("unable to read {}: {error}", path.display());
        move_aside(&path);
        History::default()
    })
}

pub fn save_history(paths: &Paths, history: &History) -> Result<()> {
    write_atomically(&paths.history(), &serde_json::to_vec(history)?)
}

/// The settings. `web_client_id` goes through its own methods; keys
/// Spotless doesn't know are kept as they are.
#[derive(Clone, Debug)]
pub struct Settings {
    /// 0–100.
    pub volume: u8,
    pub repeat: bool,
    pub shuffle: bool,
    /// Album covers in the catalog.
    pub covers: bool,
    /// The playlists show below the player; off, the window is the
    /// player alone (PL).
    pub playlists: bool,
    /// Volume levelling (Spotify's normalisation).
    pub levelling: bool,
    /// The interface's zoom, 1 at normal size.
    pub zoom: f32,
    /// Songs played are kept on disk, up to `audio_cache_mb`.
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    /// The sound server's name for the chosen output; None follows the
    /// system's default.
    pub audio_output: Option<String>,
    /// The tray icon, which closing the window hides Spotless in.
    pub tray: bool,
    /// The desktop app looks for a newer release of itself once a day.
    pub updates: bool,
    /// A newer release found is announced in a bar under the player.
    pub update_notices: bool,
    /// The Spotify tab can be used: off (as at first) in Settings, it is
    /// never shown and Spotless never joins Spotify Connect.
    pub spotify_tab_enabled: bool,
    /// The Spotify tab is open, unless closed like any tab.
    pub spotify_tab: bool,
    /// The skin the colours come from, by its name in the skins folder;
    /// None is Classic.
    pub skin: Option<String>,
    /// The colour of the lists and the display.
    pub phosphor: Phosphor,
    /// What the playlists' songs are put under headers by.
    pub grouping: Grouping,
    pub equaliser: Equaliser,
    /// The equaliser shows below the player (EQ).
    pub equaliser_shown: bool,
    /// The equaliser's presets the user saved, in the order saved.
    pub equaliser_presets: Vec<Preset>,
    /// The deck shows under the player (VIS): the music on a cassette or
    /// reels, its spectrum and its levels.
    pub deck_shown: bool,
    pub deck_media: DeckMedia,
    /// The time shows what's left of the song, after a minus, instead of
    /// what has played (Winamp's time display mode).
    pub time_left: bool,
    /// The desktop's window, as it was left.
    pub window: Option<WindowPlace>,
    raw: Map<String, Value>,
}

/// The audio cache's sizes to choose from, in MB.
pub const AUDIO_CACHE_SIZES: [u64; 6] = [512, 1024, 2048, 4096, 8192, 16384];
pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.75..=2.5;

impl Settings {
    pub fn load(paths: &Paths) -> Self {
        let path = paths.settings();
        let raw = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Map<String, Value>>(&bytes) {
                Ok(raw) => raw,
                Err(error) => {
                    log::error!("unable to read {}: {error}", path.display());
                    move_aside(&path);
                    Map::new()
                }
            },
            Err(_) => Map::new(),
        };
        Self::from_raw(raw)
    }

    /// The Spotify tab shows, and Spotless is a Spotify Connect device.
    pub fn spotify_tab_shown(&self) -> bool {
        self.spotify_tab_enabled && self.spotify_tab
    }

    /// The personal Spotify app search signs in with.
    pub fn web_client_id(&self) -> Option<String> {
        self.raw
            .get("web_client_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(String::from)
    }

    /// Sets or, with None, removes the personal app.
    pub fn set_web_client_id(&mut self, id: Option<String>) {
        match id {
            Some(id) => self.raw.insert("web_client_id".into(), id.into()),
            None => self.raw.remove("web_client_id"),
        };
    }

    fn from_raw(raw: Map<String, Value>) -> Self {
        let flag = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            volume: raw
                .get("volume")
                .and_then(Value::as_u64)
                .map_or(70, |v| v.min(100) as u8),
            repeat: flag("repeat"),
            shuffle: flag("shuffle"),
            // On by default on phones, where the catalog is browsed by them.
            covers: raw
                .get("covers")
                .and_then(Value::as_bool)
                .unwrap_or(cfg!(target_os = "android")),
            playlists: raw
                .get("playlists")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            levelling: flag("levelling"),
            zoom: raw.get("zoom").and_then(Value::as_f64).map_or(1.0, |z| {
                (z as f32).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end())
            }),
            audio_cache: raw
                .get("audio_cache")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            audio_cache_mb: raw
                .get("audio_cache_mb")
                .and_then(Value::as_u64)
                .filter(|mb| AUDIO_CACHE_SIZES.contains(mb))
                .unwrap_or(2048),
            tray: raw.get("tray").and_then(Value::as_bool).unwrap_or(true),
            updates: raw.get("updates").and_then(Value::as_bool).unwrap_or(true),
            update_notices: raw
                .get("update_notices")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            spotify_tab_enabled: raw
                .get("spotify_tab_enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            spotify_tab: raw
                .get("spotify_tab")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            audio_output: raw
                .get("audio_output")
                .and_then(Value::as_str)
                .map(String::from),
            skin: raw.get("skin").and_then(Value::as_str).map(String::from),
            phosphor: Phosphor::from_key(raw.get("phosphor").and_then(Value::as_str)),
            grouping: Grouping::from_key(raw.get("song_headers").and_then(Value::as_str)),
            equaliser: raw
                .get("equaliser")
                .and_then(|value| Equaliser::deserialize(value).ok())
                .unwrap_or_default()
                .clamped(),
            equaliser_shown: flag("equaliser_shown"),
            equaliser_presets: raw
                .get("equaliser_presets")
                .and_then(|value| Vec::<Preset>::deserialize(value).ok())
                .unwrap_or_default()
                .into_iter()
                .map(|preset| Preset {
                    bands: Equaliser {
                        bands: preset.bands,
                        ..Equaliser::OFF
                    }
                    .clamped()
                    .bands,
                    ..preset
                })
                .collect(),
            deck_shown: flag("deck_shown"),
            deck_media: DeckMedia::from_key(raw.get("deck_media").and_then(Value::as_str)),
            time_left: flag("time_left"),
            window: raw
                .get("window")
                .and_then(|value| WindowPlace::deserialize(value).ok())
                .filter(|place| {
                    place
                        .size
                        .iter()
                        .all(|side| side.is_finite() && *side > 0.0)
                }),
            raw: raw.clone(),
        }
    }

    fn to_raw(&self) -> Map<String, Value> {
        let mut raw = self.raw.clone();
        raw.insert("volume".into(), self.volume.into());
        raw.insert("repeat".into(), self.repeat.into());
        raw.insert("shuffle".into(), self.shuffle.into());
        raw.insert("covers".into(), self.covers.into());
        raw.insert("playlists".into(), self.playlists.into());
        raw.insert("levelling".into(), self.levelling.into());
        raw.insert("zoom".into(), self.zoom.into());
        raw.insert("audio_cache".into(), self.audio_cache.into());
        raw.insert("audio_cache_mb".into(), self.audio_cache_mb.into());
        raw.insert("tray".into(), self.tray.into());
        raw.insert("updates".into(), self.updates.into());
        raw.insert("update_notices".into(), self.update_notices.into());
        raw.insert(
            "spotify_tab_enabled".into(),
            self.spotify_tab_enabled.into(),
        );
        raw.insert("spotify_tab".into(), self.spotify_tab.into());
        match &self.audio_output {
            Some(output) => raw.insert("audio_output".into(), output.clone().into()),
            None => raw.remove("audio_output"),
        };
        match &self.skin {
            Some(skin) => raw.insert("skin".into(), skin.clone().into()),
            None => raw.remove("skin"),
        };
        raw.insert("phosphor".into(), self.phosphor.key().into());
        raw.insert("song_headers".into(), self.grouping.key().into());
        if let Ok(equaliser) = serde_json::to_value(self.equaliser) {
            raw.insert("equaliser".into(), equaliser);
        }
        raw.insert("equaliser_shown".into(), self.equaliser_shown.into());
        if let Ok(presets) = serde_json::to_value(&self.equaliser_presets) {
            raw.insert("equaliser_presets".into(), presets);
        }
        raw.insert("deck_shown".into(), self.deck_shown.into());
        raw.insert("deck_media".into(), self.deck_media.key().into());
        raw.insert("time_left".into(), self.time_left.into());
        match self.window.map(serde_json::to_value) {
            Some(Ok(window)) => raw.insert("window".into(), window),
            _ => raw.remove("window"),
        };
        raw
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_atomically(
            &paths.settings(),
            &serde_json::to_vec_pretty(&self.to_raw())?,
        )
    }
}

/// Written readable by the user only, from the moment it exists.
pub fn save_private(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("json.tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(&temporary)
        .and_then(|mut file| file.write_all(bytes))
        .with_context(|| format!("unable to write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("unable to replace {}", path.display()))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("unable to write {}", temporary.display()))?;
    std::fs::rename(&temporary, path)
        .with_context(|| format!("unable to replace {}", path.display()))
}

fn move_aside(path: &Path) {
    let _ = std::fs::rename(path, path.with_extension("json.bad"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::playlist::tests::entry;

    fn temporary_paths(name: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("spotless-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Paths {
            config: dir.clone(),
            cache: dir,
        }
    }

    #[test]
    fn the_library_round_trips() {
        let paths = temporary_paths("library");
        let mut library = Library::new();
        let mix = library.create("Mix");
        let list = library.get_mut(mix).unwrap();
        list.append(vec![entry(1), entry(2), entry(3)]);
        list.set_cursor(crate::model::EntryId(2));
        list.resume_ms = 42_000;
        list.toggle_mark(crate::model::EntryId(3));
        let tabs = Tabs {
            playing: Some(mix),
            viewed: Some(PlaylistId::DEFAULT),
            spotify_at: 1,
        };
        save_library(&paths, &library, tabs).unwrap();

        let (loaded, loaded_tabs) = load_library(&paths);
        assert_eq!(loaded_tabs, tabs);
        let list = loaded.get(mix).unwrap();
        assert_eq!(list.name, "Mix");
        assert_eq!(list.entries(), library.get(mix).unwrap().entries());
        assert_eq!(list.cursor(), Some(crate::model::EntryId(2)));
        assert_eq!(list.resume_ms, 42_000);
        assert_eq!(list.play_next(), [crate::model::EntryId(3)]);
    }

    #[test]
    fn an_unreadable_library_is_moved_aside() {
        let paths = temporary_paths("bad");
        std::fs::write(paths.playlists(), b"{ not json").unwrap();
        let (library, _) = load_library(&paths);
        assert_eq!(library.playlists().len(), 1);
        assert!(paths.playlists().with_extension("json.bad").exists());
    }

    #[test]
    fn settings_keep_keys_they_do_not_know() {
        let paths = temporary_paths("settings");
        std::fs::write(paths.settings(), br#"{"web_client_id":"abc","volume":30}"#).unwrap();
        let mut settings = Settings::load(&paths);
        assert_eq!(settings.volume, 30);
        settings.shuffle = true;
        settings.save(&paths).unwrap();
        let raw: Value = serde_json::from_slice(&std::fs::read(paths.settings()).unwrap()).unwrap();
        assert_eq!(raw["web_client_id"], "abc");
        assert_eq!(raw["shuffle"], true);
    }

    #[test]
    fn the_window_place_is_kept_and_a_broken_one_dropped() {
        let paths = temporary_paths("window");
        assert_eq!(Settings::load(&paths).window, None);
        let place = WindowPlace {
            position: Some([120.0, 80.0]),
            size: [900.0, 640.0],
            maximized: false,
        };
        let mut settings = Settings::load(&paths);
        settings.window = Some(place);
        settings.save(&paths).unwrap();
        assert_eq!(Settings::load(&paths).window, Some(place));
        std::fs::write(paths.settings(), br#"{"window":{"size":[0,600]}}"#).unwrap();
        assert_eq!(Settings::load(&paths).window, None);
    }

    #[test]
    fn playback_and_storage_settings_default_and_stay_in_range() {
        let paths = temporary_paths("ranges");
        let settings = Settings::load(&paths);
        assert!(!settings.levelling);
        assert_eq!(settings.zoom, 1.0);
        assert!(settings.audio_cache);
        assert_eq!(settings.audio_cache_mb, 2048);

        std::fs::write(paths.settings(), br#"{"zoom":9,"audio_cache_mb":3}"#).unwrap();
        let settings = Settings::load(&paths);
        assert_eq!(settings.zoom, *ZOOM_RANGE.end());
        assert_eq!(settings.audio_cache_mb, 2048);
    }

    #[test]
    fn the_equaliser_round_trips() {
        let paths = temporary_paths("equaliser");
        let mut settings = Settings::load(&paths);
        assert_eq!(settings.equaliser, Equaliser::OFF);
        assert!(!settings.equaliser_shown);
        let mut bands = [0.0; 10];
        bands[2] = -3.5;
        settings.equaliser = Equaliser {
            on: true,
            preamp: -2.0,
            bands,
        };
        settings.equaliser_shown = true;
        settings.equaliser_presets = vec![Preset {
            name: "Mine".into(),
            bands,
        }];
        settings.save(&paths).unwrap();
        let read = Settings::load(&paths);
        assert_eq!(read.equaliser, settings.equaliser);
        assert!(read.equaliser_shown);
        assert_eq!(read.equaliser_presets, settings.equaliser_presets);
    }

    #[test]
    fn the_deck_round_trips() {
        let paths = temporary_paths("deck");
        let mut settings = Settings::load(&paths);
        assert!(!settings.deck_shown);
        assert_eq!(settings.deck_media, DeckMedia::Cassette);
        assert!(!settings.time_left);
        settings.deck_shown = true;
        settings.deck_media = DeckMedia::Reel;
        settings.time_left = true;
        settings.save(&paths).unwrap();
        let read = Settings::load(&paths);
        assert!(read.deck_shown);
        assert!(read.time_left);
        assert_eq!(read.deck_media, DeckMedia::Reel);
        assert_eq!(read.grouping, Grouping::Source, "by source to start with");
        for media in [
            DeckMedia::Cassette,
            DeckMedia::Reel,
            DeckMedia::Cd,
            DeckMedia::Vinyl,
        ] {
            assert_eq!(DeckMedia::from_key(Some(media.key())), media);
        }
        for grouping in Grouping::ALL {
            assert_eq!(Grouping::from_key(Some(grouping.key())), grouping);
        }
    }

    #[test]
    fn the_search_app_is_set_and_removed() {
        let paths = temporary_paths("search-app");
        let mut settings = Settings::load(&paths);
        assert_eq!(settings.web_client_id(), None);
        settings.set_web_client_id(Some("abc".into()));
        settings.save(&paths).unwrap();
        let mut settings = Settings::load(&paths);
        assert_eq!(settings.web_client_id().as_deref(), Some("abc"));
        settings.set_web_client_id(None);
        settings.save(&paths).unwrap();
        assert_eq!(Settings::load(&paths).web_client_id(), None);
    }
}
