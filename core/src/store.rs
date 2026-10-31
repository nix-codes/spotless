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

/// What the desktop's deck measures the music on, beside the device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeckMeter {
    /// The spectrum's LED bars.
    #[default]
    Spectrum,
    /// A needle VU meter for each channel.
    Vu,
}

impl DeckMeter {
    pub fn key(self) -> &'static str {
        match self {
            Self::Spectrum => "spectrum",
            Self::Vu => "vu",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("vu") => Self::Vu,
            _ => Self::Spectrum,
        }
    }

    /// The next one, which clicking the meter switches to.
    pub fn next(self) -> Self {
        match self {
            Self::Spectrum => Self::Vu,
            Self::Vu => Self::Spectrum,
        }
    }
}

/// The VU meters' face.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VuFace {
    /// Black glass in the display's colour.
    #[default]
    Display,
    /// A warm backlit cream face, '70s.
    Cream,
    /// A blue backlit face with an amber needle, '90s.
    Blue,
}

impl VuFace {
    pub const ALL: [Self; 3] = [Self::Display, Self::Cream, Self::Blue];

    pub fn key(self) -> &'static str {
        match self {
            Self::Display => "display",
            Self::Cream => "cream",
            Self::Blue => "blue",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|face| Some(face.key()) == key)
            .unwrap_or_default()
    }
}

/// What loudness the VU meters read as 0 VU: following the music, or
/// fixed, in dBFS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VuCalibration {
    /// The music's last few seconds read about −4 VU, whatever their
    /// loudness.
    #[default]
    Auto,
    Minus18,
    Minus14,
    Minus10,
}

impl VuCalibration {
    pub const ALL: [Self; 4] = [Self::Auto, Self::Minus18, Self::Minus14, Self::Minus10];

    pub fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Minus18 => "-18",
            Self::Minus14 => "-14",
            Self::Minus10 => "-10",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|calibration| Some(calibration.key()) == key)
            .unwrap_or_default()
    }

    /// The fixed loudness (dBFS) that reads 0 VU; none for AUTO.
    pub fn reference(self) -> Option<f32> {
        match self {
            Self::Auto => None,
            Self::Minus18 => Some(-18.0),
            Self::Minus14 => Some(-14.0),
            Self::Minus10 => Some(-10.0),
        }
    }

    /// The next one, which CAL switches to.
    pub fn next(self) -> Self {
        let at = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        Self::ALL[(at + 1) % Self::ALL.len()]
    }
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

/// The Spotify tab is offered: built with the `spotify-tab` feature. While
/// false it's never on, and Settings doesn't show it, as if it weren't
/// there.
/// Saves each of these by its `key`, and reads it back with `from_key`,
/// which takes a key it doesn't know for the default.
macro_rules! saved_by_key {
    ($($kind:ty),*) => {$(
        impl Serialize for $kind {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.key())
            }
        }

        impl<'de> Deserialize<'de> for $kind {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                String::deserialize(deserializer).map(|key| Self::from_key(Some(&key)))
            }
        }
    )*};
}

saved_by_key!(
    Phosphor,
    Grouping,
    DeckMedia,
    DeckMeter,
    VuFace,
    VuCalibration
);

pub const SPOTIFY_TAB: bool = cfg!(feature = "spotify-tab");

/// The settings both apps share, as `settings.json` holds them. Keys the
/// core doesn't know are kept as they are, among them each app's own
/// settings ([`Settings::extra`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 0–100.
    #[serde(deserialize_with = "percent")]
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
    /// Songs played are kept on disk, up to `audio_cache_mb`.
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    /// The sound server's name for the chosen output; None follows the
    /// system's default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_output: Option<String>,
    /// The Spotify tab can be used: off (as at first) in Settings, it is
    /// never shown and Spotless never joins Spotify Connect. Always off
    /// while [`SPOTIFY_TAB`] is.
    pub spotify_tab_enabled: bool,
    /// The Spotify tab is open, unless closed like any tab.
    pub spotify_tab: bool,
    /// The colour of the lists and the display.
    pub phosphor: Phosphor,
    /// What the playlists' songs are put under headers by.
    #[serde(rename = "song_headers")]
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
    pub deck_meter: DeckMeter,
    pub vu_face: VuFace,
    pub vu_calibration: VuCalibration,
    /// The deck's cassette holds the whole playlist, on side A then side
    /// B (MIX), instead of the song playing (SINGLE).
    pub cassette_mix: bool,
    /// The deck's turntable plays the whole playlist on an LP, side A then
    /// side B (33), instead of a single for each song (45).
    pub vinyl_lp: bool,
    /// The deck's reels play at 3¾ IPS, the whole playlist in one pass,
    /// instead of at 7½, a song a reel.
    pub reel_slow: bool,
    /// The deck's CD player plays a mini CD for each song (8cm), instead
    /// of the whole playlist on one disc (12cm).
    pub cd_mini: bool,
    /// The time shows what's left of the song, after a minus, instead of
    /// what has played (Winamp's time display mode).
    pub time_left: bool,
    /// The personal Spotify app search signs in with, through its own
    /// methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    web_client_id: Option<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 70,
            repeat: false,
            shuffle: false,
            // On by default on phones, where the catalog is browsed by them.
            covers: cfg!(target_os = "android"),
            playlists: true,
            levelling: false,
            audio_cache: true,
            audio_cache_mb: 2048,
            audio_output: None,
            spotify_tab_enabled: false,
            spotify_tab: true,
            phosphor: Phosphor::default(),
            grouping: Grouping::default(),
            equaliser: Equaliser::default(),
            equaliser_shown: false,
            equaliser_presets: Vec::new(),
            deck_shown: false,
            deck_media: DeckMedia::default(),
            deck_meter: DeckMeter::default(),
            vu_face: VuFace::default(),
            vu_calibration: VuCalibration::default(),
            cassette_mix: false,
            vinyl_lp: false,
            reel_slow: false,
            cd_mini: false,
            time_left: false,
            web_client_id: None,
            extra: Map::new(),
        }
    }
}

/// A percentage saved as any whole number, kept to 0–100.
fn percent<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    u64::deserialize(deserializer).map(|value| value.min(100) as u8)
}

/// The audio cache's sizes to choose from, in MB.
pub const AUDIO_CACHE_SIZES: [u64; 6] = [512, 1024, 2048, 4096, 8192, 16384];

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
        self.web_client_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(String::from)
    }

    /// The file's keys the core doesn't know, for the settings an app
    /// keeps of its own (the desktop's window, skin and tray).
    pub fn extra(&self) -> &Map<String, Value> {
        &self.extra
    }

    /// The same, to change an app's own settings: they are saved with the
    /// rest.
    pub fn extra_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.extra
    }

    /// Sets or, with None, removes the personal app.
    pub fn set_web_client_id(&mut self, id: Option<String>) {
        self.web_client_id = id;
    }

    /// The settings in `raw`. A value that can't be read (from a newer or
    /// an older Spotless, or edited by hand) leaves only its own setting
    /// as at first.
    fn from_raw(raw: Map<String, Value>) -> Self {
        let read = |raw: &Map<String, Value>| Self::deserialize(Value::Object(raw.clone()));
        let settings = read(&raw).unwrap_or_else(|_| {
            let mut readable = Map::new();
            for (key, value) in raw {
                readable.insert(key.clone(), value);
                if read(&readable).is_err() {
                    log::warn!("the setting {key} can't be read; it is as at first");
                    readable.remove(&key);
                }
            }
            read(&readable).unwrap_or_default()
        });
        settings.in_range()
    }

    /// The settings with each kept to what it can be.
    fn in_range(self) -> Self {
        let audio_cache_mb = if AUDIO_CACHE_SIZES.contains(&self.audio_cache_mb) {
            self.audio_cache_mb
        } else {
            Self::default().audio_cache_mb
        };
        let equaliser_presets = self
            .equaliser_presets
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
            .collect();
        Self {
            audio_cache_mb,
            spotify_tab_enabled: SPOTIFY_TAB && self.spotify_tab_enabled,
            equaliser: self.equaliser.clamped(),
            equaliser_presets,
            ..self
        }
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_atomically(&paths.settings(), &serde_json::to_vec_pretty(self)?)
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

    /// `settings.json` as Spotless wrote it before its settings were read
    /// with serde, every setting away from where it starts, with two of
    /// the desktop's own.
    const WRITTEN_BEFORE: &str = r#"{
        "audio_cache": false, "audio_cache_mb": 4096,
        "audio_output": "alsa_output.usb", "cassette_mix": true,
        "cd_mini": true, "covers": true, "deck_media": "vinyl",
        "deck_meter": "vu", "deck_shown": true,
        "equaliser": {"bands": [0.0, 3.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "on": true, "preamp": -2.0},
        "equaliser_presets": [{"bands": [0.0, 3.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            "name": "Mine"}],
        "equaliser_shown": true, "levelling": true, "phosphor": "amber",
        "playlists": false, "reel_slow": true, "repeat": true, "shuffle": true,
        "song_headers": "album", "spotify_tab": false,
        "spotify_tab_enabled": false, "time_left": true, "vinyl_lp": true,
        "volume": 42, "vu_calibration": "-14", "vu_face": "cream",
        "web_client_id": "abc", "window": {"x": 1}, "zoom": 1.5
    }"#;

    #[test]
    fn a_file_written_before_reads_and_saves_back_the_same() {
        let paths = temporary_paths("before");
        std::fs::write(paths.settings(), WRITTEN_BEFORE).unwrap();
        let settings = Settings::load(&paths);
        assert_eq!(settings.volume, 42);
        assert_eq!(settings.audio_output.as_deref(), Some("alsa_output.usb"));
        assert_eq!(settings.phosphor, Phosphor::Amber);
        assert_eq!(settings.grouping, Grouping::Album);
        assert_eq!(settings.vu_calibration, VuCalibration::Minus14);
        assert_eq!(settings.equaliser_presets[0].bands[1], 3.5);
        assert_eq!(settings.web_client_id().as_deref(), Some("abc"));
        assert_eq!(settings.extra().len(), 2, "only the desktop's are extra");
        assert_eq!(settings.extra()["zoom"], 1.5);

        settings.save(&paths).unwrap();
        let saved: Value =
            serde_json::from_slice(&std::fs::read(paths.settings()).unwrap()).unwrap();
        let before: Value = serde_json::from_str(WRITTEN_BEFORE).unwrap();
        assert_eq!(saved, before);
    }

    #[test]
    fn a_value_that_cant_be_read_leaves_only_its_own_setting_as_at_first() {
        let paths = temporary_paths("unreadable");
        std::fs::write(
            paths.settings(),
            br#"{"volume": "loud", "phosphor": 5, "vu_face": "chrome",
                "shuffle": true, "web_client_id": "abc", "zoom": 2}"#,
        )
        .unwrap();
        let settings = Settings::load(&paths);
        assert_eq!(settings.volume, 70);
        assert_eq!(settings.phosphor, Phosphor::Green);
        assert_eq!(
            settings.vu_face,
            VuFace::Display,
            "a face this one doesn't know"
        );
        assert!(settings.shuffle);
        assert_eq!(settings.web_client_id().as_deref(), Some("abc"));
        assert_eq!(settings.extra()["zoom"], 2);
        assert!(!paths.settings().with_extension("json.bad").exists());
    }

    #[test]
    fn the_volume_and_the_spotify_tab_stay_in_range() {
        let paths = temporary_paths("volume");
        std::fs::write(
            paths.settings(),
            br#"{"volume": 300, "spotify_tab_enabled": true}"#,
        )
        .unwrap();
        let settings = Settings::load(&paths);
        assert_eq!(settings.volume, 100);
        assert_eq!(settings.spotify_tab_enabled, SPOTIFY_TAB);
    }

    #[test]
    fn playback_and_storage_settings_default_and_stay_in_range() {
        let paths = temporary_paths("ranges");
        let settings = Settings::load(&paths);
        assert!(!settings.levelling);
        assert!(settings.audio_cache);
        assert_eq!(settings.audio_cache_mb, 2048);

        std::fs::write(paths.settings(), br#"{"audio_cache_mb":3}"#).unwrap();
        let settings = Settings::load(&paths);
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
        // Not exact in binary: saved as 4.8, it reads back the same f32.
        bands[5] = 4.8;
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
        assert_eq!(settings.deck_meter, DeckMeter::Spectrum);
        assert_eq!(settings.vu_face, VuFace::Display);
        assert_eq!(settings.vu_calibration, VuCalibration::Auto);
        assert!(!settings.cassette_mix, "a song a tape to start with");
        assert!(!settings.vinyl_lp, "a single a song to start with");
        assert!(!settings.reel_slow, "a song a reel to start with");
        assert!(!settings.cd_mini, "the playlist on a disc to start with");
        assert!(!settings.time_left);
        settings.deck_shown = true;
        settings.deck_media = DeckMedia::Reel;
        settings.deck_meter = DeckMeter::Vu;
        settings.vu_face = VuFace::Cream;
        settings.vu_calibration = VuCalibration::Minus10;
        settings.cassette_mix = true;
        settings.vinyl_lp = true;
        settings.reel_slow = true;
        settings.cd_mini = true;
        settings.time_left = true;
        settings.save(&paths).unwrap();
        let read = Settings::load(&paths);
        assert!(read.deck_shown);
        assert!(read.time_left);
        assert_eq!(read.deck_media, DeckMedia::Reel);
        assert_eq!(read.deck_meter, DeckMeter::Vu);
        assert_eq!(read.vu_face, VuFace::Cream);
        assert_eq!(read.vu_calibration, VuCalibration::Minus10);
        assert!(read.cassette_mix);
        assert!(read.vinyl_lp);
        assert!(read.reel_slow);
        assert!(read.cd_mini);
        assert_eq!(read.grouping, Grouping::Source, "by source to start with");
        for media in [
            DeckMedia::Cassette,
            DeckMedia::Reel,
            DeckMedia::Cd,
            DeckMedia::Vinyl,
        ] {
            assert_eq!(DeckMedia::from_key(Some(media.key())), media);
        }
        for meter in [DeckMeter::Spectrum, DeckMeter::Vu] {
            assert_eq!(DeckMeter::from_key(Some(meter.key())), meter);
        }
        for face in VuFace::ALL {
            assert_eq!(VuFace::from_key(Some(face.key())), face);
        }
        for calibration in VuCalibration::ALL {
            assert_eq!(
                VuCalibration::from_key(Some(calibration.key())),
                calibration
            );
        }
        assert_eq!(VuCalibration::Minus10.next(), VuCalibration::Auto);
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
