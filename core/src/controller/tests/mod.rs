//! The controller's tests, a file for each of its parts, on a backend the
//! test plays ([`Rig`]).

use super::persist::SAVE_DELAY;
use super::*;
use crate::backend::SpotifyRequest;
use crate::browse::{Page, Unlisted};
use crate::model::ArtistRef;
use crate::model::catalog::Songs;
use crate::model::catalog::{SpotifyPlaylist, SpotifyTarget};
use crate::model::playlist::tests::entry;
use crate::model::{AlbumRef, Grouping};
use crate::playback::equaliser::Equaliser;
use crate::playback::transport::Request;
use crate::spotify::queue::SpotifyQueue;
use std::sync::mpsc;
use tokio::sync::mpsc::UnboundedReceiver;

mod browse;
mod lastfm;
mod persist;
mod playback;
mod playlists;
mod spotify_lists;
mod spotify_tab;
mod sync;
mod view;

/// A controller on empty folders, with a backend the test plays.
struct Rig {
    controller: Controller,
    commands: UnboundedReceiver<Command>,
    events: mpsc::Sender<Event>,
    root: std::path::PathBuf,
}

impl Rig {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("spotless-controller-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let (backend, commands, events) = Backend::detached();
        let settings = Settings::load(&paths);
        let controller = Controller::with_backend(paths, settings, Arc::default(), Ok(backend));
        Self {
            controller,
            commands,
            events,
            root,
        }
    }

    /// The backend tells this, and the controller takes it in.
    fn tell(&mut self, event: Event) {
        self.events.send(event).unwrap();
        self.controller.receive();
    }

    fn sent(&mut self) -> Vec<Command> {
        std::iter::from_fn(|| self.commands.try_recv().ok()).collect()
    }

    /// The titles of the viewed playlist's songs.
    fn songs(&self) -> Vec<String> {
        let state = self.controller.state();
        state
            .library
            .get(state.viewed)
            .unwrap()
            .entries()
            .iter()
            .map(|e| e.track.title.clone())
            .collect()
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn tracks(ids: impl IntoIterator<Item = u64>) -> Vec<TrackRef> {
    ids.into_iter().map(|id| entry(id).track).collect()
}

/// An album's songs, as read.
fn songs(tracks: Vec<TrackRef>) -> Songs {
    Songs {
        tracks,
        ..Songs::default()
    }
}

fn titles(rig: &Rig, list: PlaylistId) -> Vec<String> {
    let library = &rig.controller.state().library;
    let list = library.get(list).unwrap();
    list.entries()
        .iter()
        .map(|entry| entry.track.title.clone())
        .collect()
}
