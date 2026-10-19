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
    pub fn new() -> Result<Self> {
        let config = dirs::config_dir()
            .context("no configuration folder for this user")?
            .join("spotless");
        let cache = dirs::cache_dir()
            .context("no cache folder for this user")?
            .join("spotless");
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

    /// librespot writes the reusable playback credential here.
    pub fn credentials(&self) -> PathBuf {
        self.config.join("credentials.json")
    }
}
