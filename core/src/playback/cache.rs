//! Songs kept on disk once downloaded whole, to play again without
//! downloading them, and with no connection. Each source keeps its files
//! under a key of its own (`spotify/<file id>`), as the source gives them:
//! a Spotify file stays encrypted.
//!
//! A file is written under a temporary name and renamed once whole, so a
//! crash never leaves one cut off; those left over are removed at start.
//! Past its size, the files played longest ago go first.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

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

pub struct SongCache {
    /// None: songs aren't kept.
    dir: Option<PathBuf>,
    limit: u64,
    /// Each file kept, by its path: its size, and when it was last played.
    files: Mutex<HashMap<PathBuf, (u64, SystemTime)>>,
}

impl SongCache {
    /// Keeps songs in `dir`, up to `limit` bytes.
    pub fn new(dir: PathBuf, limit: u64) -> Self {
        let mut files = HashMap::new();
        scan(&dir, &mut files);
        let cache = Self {
            dir: Some(dir),
            limit,
            files: Mutex::new(files),
        };
        cache.prune();
        cache
    }

    /// Keeps nothing.
    pub fn off() -> Self {
        Self {
            dir: None,
            limit: 0,
            files: Mutex::default(),
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
        match File::open(&path) {
            Ok(file) => {
                if let Some(entry) = self.lock().get_mut(&path) {
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

    /// Keeps `contents` under `key`, whole or not at all, then makes room.
    pub fn put(&self, key: &str, contents: &mut impl Read) -> io::Result<()> {
        let Some(path) = self.path(key) else {
            return Ok(());
        };
        let dir = path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let part = path.with_extension(PART);
        let written = write_whole(&part, contents).and_then(|size| {
            fs::rename(&part, &path)?;
            Ok(size)
        });
        let size = match written {
            Ok(size) => size,
            Err(e) => {
                let _ = fs::remove_file(&part);
                return Err(e);
            }
        };
        self.lock().insert(path, (size, SystemTime::now()));
        self.prune();
        Ok(())
    }

    /// Forgets the song under `key`, as it didn't decode.
    pub fn remove(&self, key: &str) {
        let Some(path) = self.path(key) else {
            return;
        };
        self.lock().remove(&path);
        if let Err(e) = fs::remove_file(&path)
            && e.kind() != io::ErrorKind::NotFound
        {
            log::warn!("unable to remove {key} from the cache: {e}");
        }
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(key))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<PathBuf, (u64, SystemTime)>> {
        self.files.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Removes the files played longest ago until the rest fit.
    fn prune(&self) {
        let mut files = self.lock();
        let mut total: u64 = files.values().map(|(size, _)| size).sum();
        if total <= self.limit {
            return;
        }
        let mut oldest: Vec<_> = files.iter().map(|(p, &(s, t))| (t, s, p.clone())).collect();
        oldest.sort();
        for (_, size, path) in oldest {
            if total <= self.limit {
                break;
            }
            match fs::remove_file(&path) {
                Ok(()) => log::debug!("removed {} from the cache", path.display()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => {
                    log::warn!("unable to remove {}: {e}", path.display());
                    continue;
                }
            }
            files.remove(&path);
            total -= size;
        }
    }
}

/// Lists the files kept under `dir`, removing those left half written and
/// any download left over.
fn scan(dir: &Path, files: &mut HashMap<PathBuf, (u64, SystemTime)>) {
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
                scan(&path, files);
            }
        } else if path.extension().is_some_and(|e| e == PART) {
            let _ = fs::remove_file(&path);
        } else {
            let played = metadata
                .accessed()
                .or_else(|_| metadata.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            files.insert(path, (metadata.len(), played));
        }
    }
}

fn write_whole(path: &Path, contents: &mut impl Read) -> io::Result<u64> {
    let mut file = File::create(path)?;
    let size = io::copy(contents, &mut file)?;
    file.flush()?;
    file.sync_all()?;
    Ok(size)
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
        cache.put("spotify/ab", &mut Cursor::new(vec![1, 2, 3])).unwrap();
        assert_eq!(read(cache.get("spotify/ab")), [1, 2, 3]);
        let left: Vec<_> = fs::read_dir(dir.join("spotify")).unwrap().flatten().collect();
        assert_eq!(left.len(), 1);
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
        assert!(cache.put("spotify/ab", &mut Failing).is_err());
        assert!(cache.get("spotify/ab").is_none());
        let left: Vec<_> = fs::read_dir(dir.join("spotify")).unwrap().flatten().collect();
        assert!(left.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn past_its_size_the_songs_played_longest_ago_go_first() {
        let dir = dir("pruned");
        let cache = SongCache::new(dir.clone(), 10);
        cache.put("spotify/a", &mut Cursor::new(vec![0; 4])).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        cache.put("spotify/b", &mut Cursor::new(vec![0; 4])).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        // Played again: now the latest.
        cache.get("spotify/a").unwrap();
        cache.put("spotify/c", &mut Cursor::new(vec![0; 4])).unwrap();
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
        fs::write(dir.join("downloading/x"), [1]).unwrap();
        let cache = SongCache::new(dir.clone(), 1000);
        assert!(cache.get("spotify/a").is_some());
        assert!(!dir.join("spotify/b.part").exists());
        assert!(!dir.join("downloading").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn off_keeps_nothing() {
        let cache = SongCache::off();
        cache.put("spotify/a", &mut Cursor::new(vec![1])).unwrap();
        assert!(cache.get("spotify/a").is_none());
    }
}
