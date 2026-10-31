//! The worker's part in the caches on disk: measuring and emptying them.

use super::Worker;
use crate::backend::{Command, Event};
use crate::backend::{Storage, Store};
use crate::paths::Paths;
use std::sync::Arc;

impl Worker {
    /// Measures the caches, or empties one.
    pub(super) fn handle_storage(&mut self, command: Command) {
        match command {
            Command::MeasureStorage => self.measure_storage(),
            Command::ClearCache(store) => {
                let dir = match store {
                    Store::Audio => self.paths.audio_cache(),
                    Store::Covers => self.paths.covers(),
                };
                let emit = Arc::clone(&self.emit);
                let paths = self.paths.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(error) = clear_dir(&dir) {
                        log::error!("unable to clear {}: {error}", dir.display());
                        emit(Event::Error("Couldn't clear all of the cache.".into()));
                    }
                    emit(Event::Storage(measure(&paths)));
                });
            }
            _ => {}
        }
    }

    /// Measures the caches in the background.
    fn measure_storage(&self) {
        let emit = Arc::clone(&self.emit);
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || emit(Event::Storage(measure(&paths))));
    }
}

fn measure(paths: &Paths) -> Storage {
    Storage {
        audio: dir_size(&paths.audio_cache()),
        covers: dir_size(&paths.covers()),
    }
}

/// The bytes in a folder and its subfolders; a missing folder holds none.
fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Removes what a folder holds, keeping the folder.
fn clear_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{clear_dir, dir_size};

    #[test]
    fn a_cleared_folder_is_empty_and_kept() {
        let dir = std::env::temp_dir().join(format!("spotless-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ab")).unwrap();
        std::fs::write(dir.join("ab/cd"), [0; 300]).unwrap();
        std::fs::write(dir.join("ef"), [0; 200]).unwrap();
        assert_eq!(dir_size(&dir), 500);
        clear_dir(&dir).unwrap();
        assert_eq!(dir_size(&dir), 0);
        assert!(dir.is_dir());
        assert!(clear_dir(&dir.join("missing")).is_ok());
    }
}
