//! Saving: once changes have rested a moment, and at once as the app
//! closes or goes to the background.

use std::time::{Duration, Instant};

use super::Controller;
use crate::store::{self, Tabs};

/// Changes are saved this long after the last one.
pub(super) const SAVE_DELAY: Duration = Duration::from_secs(1);

impl Controller {
    pub(super) fn save_tick(&mut self) -> Option<Duration> {
        let since = self.unsaved?;
        match SAVE_DELAY.checked_sub(since.elapsed()) {
            Some(wait) if !wait.is_zero() => Some(wait),
            _ => {
                self.store();
                self.sync_all(false);
                None
            }
        }
    }

    pub(super) fn mark_unsaved(&mut self) {
        self.unsaved.get_or_insert_with(Instant::now);
    }

    /// Saves now, and sends the synced playlists' positions, as the app
    /// closes or goes to the background.
    pub fn save(&mut self) {
        self.store();
        self.sync_all(true);
    }

    pub(super) fn store(&mut self) {
        self.unsaved = None;
        let state = &mut self.state;
        state
            .transport
            .remember_position(&mut state.library, state.player.position_now());
        let tabs = Tabs {
            playing: state.transport.playback.playing(),
            viewed: Some(state.viewed),
            spotify_at: state.spotify_at,
        };
        if let Err(error) = store::save_library(&self.paths, &state.library, tabs) {
            log::error!("{error:#}");
            state.error = Some("Couldn't save the playlists.".into());
        }
        if let Err(error) = state.settings.save(&self.paths) {
            log::error!("{error:#}");
        }
        let history = store::History {
            searches: state.browse.recent_searches.clone(),
            artists: state.browse.recent.clone(),
            tab: state.browse.shown,
            spotify_playlist: state.browse.spotify_open.clone(),
        };
        if let Err(error) = store::save_history(&self.paths, &history) {
            log::error!("{error:#}");
        }
    }
}
