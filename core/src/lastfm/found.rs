//! Songs loved on Last.fm, found on Spotify for Browse's LAST.FM tab to
//! add or play. Last.fm knows them only by artist and title, so each is
//! searched for once, and what was found is kept in `lastfm-found.json`;
//! a song not found is looked for again the next time it is asked for.

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::Mutex;

use super::loved::LovedSong;
use crate::model::TrackRef;
use crate::paths::Paths;
use crate::spotify::webapi::WebApi;
use crate::store::save_private;

/// The songs found so far, by their loved song's key.
pub struct Found {
    paths: Paths,
    songs: HashMap<String, TrackRef>,
}

impl Found {
    pub fn load(paths: &Paths) -> Self {
        let songs = std::fs::read(paths.lastfm_found())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            paths: paths.clone(),
            songs,
        }
    }

    fn save(&self) {
        let result = serde_json::to_vec(&self.songs)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| save_private(&self.paths.lastfm_found(), &bytes));
        if let Err(error) = result {
            log::error!("unable to save the loved songs found on Spotify: {error:#}");
        }
    }
}

/// Each of `songs` on Spotify, in order: None for those it doesn't have.
/// What is found is kept even if a later search fails.
pub async fn find(
    web: &Mutex<WebApi>,
    found: &Mutex<Found>,
    songs: &[LovedSong],
) -> Result<Vec<Option<TrackRef>>> {
    let mut found = found.lock().await;
    let mut tracks = Vec::with_capacity(songs.len());
    let mut result = Ok(());
    let mut added = false;
    for song in songs {
        let key = song.key();
        if let Some(track) = found.songs.get(&key) {
            tracks.push(Some(track.clone()));
            continue;
        }
        match web.lock().await.find_song(&song.artist, &song.title).await {
            Ok(track) => {
                if let Some(track) = &track {
                    found.songs.insert(key, track.clone());
                    added = true;
                }
                tracks.push(track);
            }
            Err(error) => {
                result = Err(error);
                break;
            }
        }
    }
    if added {
        found.save();
    }
    result.map(|()| tracks)
}
