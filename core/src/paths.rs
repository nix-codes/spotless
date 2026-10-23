//! Where Spotless keeps its files.

use std::path::PathBuf;

use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct Paths {
    /// Settings, playlists and the playback credential.
    pub config: PathBuf,
    /// The audio cache and, later, cover art.
    pub cache: PathBuf,
}

impl Paths {
    /// The user's own folders, in a `spotless` folder each.
    pub fn new() -> Result<Self> {
        let config = dirs::config_dir()
            .context("no configuration folder for this user")?
            .join("spotless");
        let cache = dirs::cache_dir()
            .context("no cache folder for this user")?
            .join("spotless");
        Self::from_dirs(config, cache)
    }

    /// These folders, as they are: for a system that gives the app folders
    /// of its own, such as Android. They are made if missing.
    pub fn from_dirs(config: PathBuf, cache: PathBuf) -> Result<Self> {
        for dir in [&config, &cache] {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("unable to create {}", dir.display()))?;
        }
        Ok(Self { config, cache })
    }

    pub fn audio_cache(&self) -> PathBuf {
        self.cache.join("audio")
    }

    pub fn playlists(&self) -> PathBuf {
        self.config.join("playlists.json")
    }

    pub fn settings(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    pub fn history(&self) -> PathBuf {
        self.config.join("history.json")
    }

    pub fn covers(&self) -> PathBuf {
        self.cache.join("covers")
    }

    /// Search's refresh token, for the personal app (private).
    pub fn search_credentials(&self) -> PathBuf {
        self.config.join("search-credentials.json")
    }

    /// The Last.fm API account and sign-in (private).
    pub fn lastfm(&self) -> PathBuf {
        self.config.join("lastfm.json")
    }

    /// Listens Last.fm hasn't taken yet.
    pub fn lastfm_queue(&self) -> PathBuf {
        self.config.join("lastfm-queue.json")
    }

    /// The Spotify tab's device ID, the same every start.
    pub fn connect_device(&self) -> PathBuf {
        self.config.join("connect-device")
    }

    /// librespot writes the reusable playback credential here.
    pub fn credentials(&self) -> PathBuf {
        self.config.join("credentials.json")
    }
}

#[cfg(test)]
mod tests {
    use super::Paths;

    #[test]
    fn given_folders_are_used_as_they_are_and_made() {
        let root = std::env::temp_dir().join(format!("spotless-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::from_dirs(root.join("files"), root.join("cache")).unwrap();
        assert!(root.join("files").is_dir() && root.join("cache").is_dir());
        assert_eq!(paths.settings(), root.join("files/settings.json"));
        assert_eq!(paths.audio_cache(), root.join("cache/audio"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
