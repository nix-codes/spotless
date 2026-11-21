//! Songs kept on disk once downloaded whole, to play again without
//! downloading them, and with no connection. Each source keeps its files
//! under a key of its own (`spotify/<file id>`), as the source gives them:
//! a Spotify file stays encrypted.
//!
//! Beside each file, a record (`<key>.json`) of what it is: the names it's
//! found by (the songs asked for that play it) and what its source needs to
//! open it without asking anyone. The record is written first and the file
//! after, so a file there means its record is whole.
//!
//! A file is written under a temporary name and renamed once whole, so a
//! crash never leaves one cut off; those left over are removed at start,
//! with records whose file never came. Past its size, the files played
//! longest ago go first, with their records.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::paths::Paths;
use crate::spotify::session::AudioCache;

impl AudioCache {
    /// The songs kept, as the settings have it: off, or up to their size.
    /// The cache librespot kept before is removed, in the background: its
    /// files are no longer read.
    pub fn open(&self, paths: &Paths) -> SongCache {
        let old = paths.old_audio_cache();
        if old.exists() {
            std::thread::spawn(move || match fs::remove_dir_all(&old) {
                Ok(()) => log::info!("removed the old audio cache"),
                Err(e) => log::warn!("unable to remove the old audio cache: {e}"),
            });
        }
        if self.on {
            SongCache::new(paths.audio_cache(), self.mb * 1024 * 1024)
        } else {
            SongCache::off()
        }
    }
}

/// Files being written end in this until whole.
const PART: &str = "part";
/// Records end in this.
const RECORD: &str = "json";

/// A record as kept on disk.
#[derive(Serialize, Deserialize)]
struct Record {
    names: Vec<String>,
    /// The source's own, as it wrote it.
    details: serde_json::Value,
}

/// A song found by one of its names.
pub struct Found {
    pub key: String,
    /// What its source recorded of it.
    pub details: serde_json::Value,
    pub file: File,
}

pub struct SongCache {
    /// None: songs aren't kept.
    dir: Option<PathBuf>,
    limit: u64,
    index: Mutex<Index>,
}

#[derive(Default)]
struct Index {
    /// Each file kept, by its path: its size with its record's, and when it
    /// was last played.
    files: HashMap<PathBuf, (u64, SystemTime)>,
    /// The file each name plays.
    names: HashMap<String, PathBuf>,
}

impl SongCache {
    /// Keeps songs in `dir`, up to `limit` bytes.
    pub fn new(dir: PathBuf, limit: u64) -> Self {
        let cache = Self {
            dir: Some(dir.clone()),
            limit,
            index: Mutex::new(scan(&dir)),
        };
        cache.prune();
        cache
    }

    /// Keeps nothing.
    pub fn off() -> Self {
        Self {
            dir: None,
            limit: 0,
            index: Mutex::default(),
        }
    }

    /// Where a download is written until it's whole: beside the songs, or
    /// the system's temporary folder when none are kept.
    pub fn temporary_dir(&self) -> PathBuf {
        self.dir
            .as_ref()
            .map_or_else(std::env::temp_dir, |dir| dir.join("downloading"))
    }

    /// The song kept under `key`, if it is.
    pub fn get(&self, key: &str) -> Option<File> {
        let path = self.path(key)?;
        self.open(&path, key)
    }

    /// The song `name` plays, with its record, if it's kept.
    pub fn find(&self, name: &str) -> Option<Found> {
        let path = self.lock().names.get(name)?.clone();
        let key = self.key(&path)?;
        let record = fs::read(record_path(&path))
            .map_err(|e| e.to_string())
            .and_then(|r| serde_json::from_slice::<Record>(&r).map_err(|e| e.to_string()));
        let record = match record {
            Ok(record) => record,
            Err(e) => {
                log::warn!("unable to read the record of {key}: {e}");
                return None;
            }
        };
        let file = self.open(&path, &key)?;
        Some(Found {
            key,
            details: record.details,
            file,
        })
    }

    /// Keeps `contents` under `key`, whole or not at all, with its record:
    /// found from now on by `names`. Then makes room.
    pub fn put(
        &self,
        key: &str,
        names: &[String],
        details: &serde_json::Value,
        contents: &mut impl Read,
    ) -> io::Result<()> {
        let Some(path) = self.path(key) else {
            return Ok(());
        };
        let record_size = self.write_record(&path, names, details)?;
        let written = write_whole(&path, contents);
        let size = match written {
            Ok(size) => size,
            Err(e) => {
                let _ = fs::remove_file(record_path(&path));
                return Err(e);
            }
        };
        let mut index = self.lock();
        index.files.insert(path.clone(), (size + record_size, SystemTime::now()));
        for name in names {
            index.names.insert(name.clone(), path.clone());
        }
        drop(index);
        self.prune();
        Ok(())
    }

    /// Finds the song kept under `key` by `name` too from now on, with
    /// `details` as its record: a song asked for by another name, or kept
    /// before it had a record. Nothing if the song isn't kept.
    pub fn name(&self, key: &str, name: &str, details: &serde_json::Value) {
        let Some(path) = self.path(key) else {
            return;
        };
        if !self.lock().files.contains_key(&path) {
            return;
        }
        let mut names: Vec<String> = fs::read(record_path(&path))
            .ok()
            .and_then(|r| serde_json::from_slice::<Record>(&r).ok())
            .map(|r| r.names)
            .unwrap_or_default();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
        match self.write_record(&path, &names, details) {
            Ok(_) => {
                self.lock().names.insert(name.to_string(), path);
            }
            Err(e) => log::warn!("unable to write the record of {key}: {e}"),
        }
    }

    /// Forgets the song under `key`, as it didn't decode.
    pub fn remove(&self, key: &str) {
        let Some(path) = self.path(key) else {
            return;
        };
        self.lock().forget(&path);
        if let Err(e) = remove(&path) {
            log::warn!("unable to remove {key} from the cache: {e}");
        }
    }

    fn open(&self, path: &Path, key: &str) -> Option<File> {
        match File::open(path) {
            Ok(file) => {
                if let Some(entry) = self.lock().files.get_mut(path) {
                    entry.1 = SystemTime::now();
                }
                Some(file)
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("unable to read {key} from the cache: {e}");
                }
                None
            }
        }
    }

    /// Writes the record of the file at `path`, whole or not at all.
    /// Returns its size.
    fn write_record(&self, path: &Path, names: &[String], details: &serde_json::Value) -> io::Result<u64> {
        let record = serde_json::to_vec(&Record {
            names: names.to_vec(),
            details: details.clone(),
        })
        .map_err(io::Error::other)?;
        write_whole(&record_path(path), &mut record.as_slice())
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(key))
    }

    /// The key of the file at `path`.
    fn key(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(self.dir.as_ref()?).ok()?;
        let parts: Vec<_> = relative.iter().map(|p| p.to_string_lossy()).collect();
        Some(parts.join("/"))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Index> {
        self.index.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Removes the files played longest ago until the rest fit.
    fn prune(&self) {
        let mut index = self.lock();
        let mut total: u64 = index.files.values().map(|(size, _)| size).sum();
        if total <= self.limit {
            return;
        }
        let mut oldest: Vec<_> = index.files.iter().map(|(p, &(s, t))| (t, s, p.clone())).collect();
        oldest.sort();
        for (_, size, path) in oldest {
            if total <= self.limit {
                break;
            }
            match remove(&path) {
                Ok(()) => log::debug!("removed {} from the cache", path.display()),
                Err(e) => {
                    log::warn!("unable to remove {}: {e}", path.display());
                    continue;
                }
            }
            index.forget(&path);
            total -= size;
        }
    }
}

impl Index {
    fn forget(&mut self, path: &Path) {
        self.files.remove(path);
        self.names.retain(|_, p| p != path);
    }
}

/// The record beside the file at `path`.
fn record_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".");
    name.push(RECORD);
    PathBuf::from(name)
}

/// Removes the file at `path` and its record; one already gone is no
/// failure.
fn remove(path: &Path) -> io::Result<()> {
    let gone = |result: io::Result<()>| match result {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    gone(fs::remove_file(path))?;
    gone(fs::remove_file(record_path(path)))
}

/// Lists the files kept under `dir` and the names in their records,
/// removing what was left half written, records whose file never came and
/// any download left over.
fn scan(dir: &Path) -> Index {
    let mut index = Index::default();
    let mut records = Vec::new();
    walk(dir, &mut index.files, &mut records);
    for record in records {
        let file = record.with_extension("");
        let Some((size, _)) = index.files.get_mut(&file) else {
            let _ = fs::remove_file(&record);
            continue;
        };
        let Ok(bytes) = fs::read(&record) else {
            continue;
        };
        *size += bytes.len() as u64;
        match serde_json::from_slice::<Record>(&bytes) {
            Ok(Record { names, .. }) => {
                for name in names {
                    index.names.insert(name, file.clone());
                }
            }
            Err(e) => log::warn!("unable to read {}: {e}", record.display()),
        }
    }
    index
}

fn walk(dir: &Path, files: &mut HashMap<PathBuf, (u64, SystemTime)>, records: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            if entry.file_name() == "downloading" {
                let _ = fs::remove_dir_all(&path);
            } else {
                walk(&path, files, records);
            }
        } else if path.extension().is_some_and(|e| e == PART) {
            let _ = fs::remove_file(&path);
        } else if path.extension().is_some_and(|e| e == RECORD) {
            records.push(path);
        } else {
            let played = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.insert(path, (metadata.len(), played));
        }
    }
}

/// Writes `contents` to `path` under a temporary name, renamed once whole;
/// nothing is left if it fails. Returns the size written.
fn write_whole(path: &Path, contents: &mut impl Read) -> io::Result<u64> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut part = OsString::from(path.as_os_str());
    part.push(".");
    part.push(PART);
    let part = PathBuf::from(part);
    let written = (|| {
        let mut file = File::create(&part)?;
        let size = io::copy(contents, &mut file)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&part, path)?;
        Ok(size)
    })();
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("spotless-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn put(cache: &SongCache, key: &str, contents: &mut impl Read) -> io::Result<()> {
        cache.put(key, &[format!("name of {key}")], &serde_json::json!({"key": key}), contents)
    }

    fn read(file: Option<File>) -> Vec<u8> {
        let mut data = Vec::new();
        file.unwrap().read_to_end(&mut data).unwrap();
        data
    }

    #[test]
    fn a_kept_song_is_there_whole_under_its_key() {
        let dir = dir("kept");
        let cache = SongCache::new(dir.clone(), 1000);
        assert!(cache.get("spotify/ab").is_none());
        put(&cache, "spotify/ab", &mut Cursor::new(vec![1, 2, 3])).unwrap();
        assert_eq!(read(cache.get("spotify/ab")), [1, 2, 3]);
        let left: Vec<_> = fs::read_dir(dir.join("spotify")).unwrap().flatten().collect();
        // The file and its record.
        assert_eq!(left.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_save_failing_midway_leaves_nothing() {
        struct Failing;
        impl Read for Failing {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::other("gone"))
            }
        }
        let dir = dir("failing");
        let cache = SongCache::new(dir.clone(), 1000);
        assert!(put(&cache, "spotify/ab", &mut Failing).is_err());
        assert!(cache.get("spotify/ab").is_none());
        let left: Vec<_> = fs::read_dir(dir.join("spotify")).unwrap().flatten().collect();
        assert!(left.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn past_its_size_the_songs_played_longest_ago_go_first() {
        let dir = dir("pruned");
        // Each takes 65 bytes with its record: two fit.
        let cache = SongCache::new(dir.clone(), 150);
        put(&cache, "spotify/a", &mut Cursor::new(vec![0; 4])).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        put(&cache, "spotify/b", &mut Cursor::new(vec![0; 4])).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // Played again: now the latest.
        cache.get("spotify/a").unwrap();
        put(&cache, "spotify/c", &mut Cursor::new(vec![0; 4])).unwrap();
        assert!(cache.get("spotify/a").is_some());
        assert!(cache.get("spotify/b").is_none());
        assert!(cache.get("spotify/c").is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn what_was_left_half_written_is_removed_at_start() {
        let dir = dir("leftover");
        fs::create_dir_all(dir.join("spotify")).unwrap();
        fs::create_dir_all(dir.join("downloading")).unwrap();
        fs::write(dir.join("spotify/a"), [1]).unwrap();
        fs::write(dir.join("spotify/b.part"), [1]).unwrap();
        fs::write(dir.join("spotify/c.json"), br#"{"names":["c"],"details":null}"#).unwrap();
        fs::write(dir.join("downloading/x"), [1]).unwrap();
        let cache = SongCache::new(dir.clone(), 1000);
        assert!(cache.get("spotify/a").is_some());
        assert!(!dir.join("spotify/b.part").exists());
        assert!(!dir.join("downloading").exists());
        assert!(!dir.join("spotify/c.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn off_keeps_nothing() {
        let cache = SongCache::off();
        put(&cache, "spotify/a", &mut Cursor::new(vec![1])).unwrap();
        assert!(cache.get("spotify/a").is_none());
    }

    #[test]
    fn a_kept_song_is_found_by_its_names_with_its_record() {
        let dir = dir("names");
        let names = ["spotify:track:asked".to_string(), "spotify:track:played".to_string()];
        let details = serde_json::json!({"title": "A song"});
        let cache = SongCache::new(dir.clone(), 1000);
        cache.put("spotify/ab", &names, &details, &mut Cursor::new(vec![1, 2])).unwrap();
        for name in &names {
            let found = cache.find(name).unwrap();
            assert_eq!(found.key, "spotify/ab");
            assert_eq!(found.details, details);
            assert_eq!(read(Some(found.file)), [1, 2]);
        }
        assert!(cache.find("spotify:track:other").is_none());
        // And again after a restart.
        let cache = SongCache::new(dir.clone(), 1000);
        assert_eq!(cache.find(&names[0]).unwrap().details, details);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_song_kept_is_given_another_name() {
        let dir = dir("renamed");
        let details = serde_json::json!({"title": "A song"});
        let cache = SongCache::new(dir.clone(), 1000);
        put(&cache, "spotify/ab", &mut Cursor::new(vec![1])).unwrap();
        cache.name("spotify/ab", "another", &details);
        assert_eq!(cache.find("another").unwrap().details, details);
        assert!(cache.find("name of spotify/ab").is_some());
        // A song not kept gets no record.
        cache.name("spotify/cd", "a third", &details);
        assert!(cache.find("a third").is_none());
        assert!(!dir.join("spotify/cd.json").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_song_removed_takes_its_record_and_names() {
        let dir = dir("removed");
        let cache = SongCache::new(dir.clone(), 1000);
        put(&cache, "spotify/ab", &mut Cursor::new(vec![1])).unwrap();
        cache.remove("spotify/ab");
        assert!(cache.find("name of spotify/ab").is_none());
        assert!(!dir.join("spotify/ab.json").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
