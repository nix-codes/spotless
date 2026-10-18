//! The playlists and settings on disk, in the config folder.
//!
//! Both files are written whole to a temporary file and renamed over the
//! old one, so a crash mid-write never leaves half a file. A file that can't
//! be read is moved aside as `*.bad` rather than overwritten.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::model::{Library, PlaylistId};
use crate::paths::Paths;

const LIBRARY_VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct LibraryFile {
    version: u32,
    #[serde(flatten)]
    library: Library,
    /// The playlist Next and Previous act on.
    playing: Option<PlaylistId>,
}

/// The saved playlists, or a fresh library with only Default.
pub fn load_library(paths: &Paths) -> (Library, Option<PlaylistId>) {
    let path = paths.playlists();
    match read_library(&path) {
        Ok(Some((library, playing))) => (library, playing),
        Ok(None) => (Library::new(), None),
        Err(error) => {
            log::error!("unable to read {}: {error:#}", path.display());
            move_aside(&path);
            (Library::new(), None)
        }
    }
}

fn read_library(path: &Path) -> Result<Option<(Library, Option<PlaylistId>)>> {
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
    let playing = file.playing.filter(|&id| file.library.get(id).is_some());
    Ok(Some((file.library, playing)))
}

pub fn save_library(paths: &Paths, library: &Library, playing: Option<PlaylistId>) -> Result<()> {
    let file = LibraryFile {
        version: LIBRARY_VERSION,
        library: library.clone(),
        playing,
    };
    write_atomically(&paths.playlists(), &serde_json::to_vec(&file)?)
}

/// The settings Spotless changes. Other keys in the file (`web_client_id`)
/// are kept as they are.
#[derive(Clone, Debug)]
pub struct Settings {
    /// 0–100.
    pub volume: u8,
    pub repeat: bool,
    pub shuffle: bool,
    raw: Map<String, Value>,
}

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

    fn from_raw(raw: Map<String, Value>) -> Self {
        let flag = |key: &str| raw.get(key).and_then(Value::as_bool).unwrap_or(false);
        Self {
            volume: raw
                .get("volume")
                .and_then(Value::as_u64)
                .map_or(70, |v| v.min(100) as u8),
            repeat: flag("repeat"),
            shuffle: flag("shuffle"),
            raw: raw.clone(),
        }
    }

    fn to_raw(&self) -> Map<String, Value> {
        let mut raw = self.raw.clone();
        raw.insert("volume".into(), self.volume.into());
        raw.insert("repeat".into(), self.repeat.into());
        raw.insert("shuffle".into(), self.shuffle.into());
        raw
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        write_atomically(
            &paths.settings(),
            &serde_json::to_vec_pretty(&self.to_raw())?,
        )
    }
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
        save_library(&paths, &library, Some(mix)).unwrap();

        let (loaded, playing) = load_library(&paths);
        assert_eq!(playing, Some(mix));
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
}
