//! The Spotify tab's Connect device, left out of this build: without the
//! `spotify-tab` feature Spotless never joins Spotify Connect, the tab
//! can't be shown (`store::SPOTIFY_TAB`), and what the worker asks of the
//! device here does nothing. `connect.rs` is the device itself.

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::Worker;
use crate::backend::Command;

/// What a device would tell the worker: never anything.
pub(super) enum RemoteReport {}

pub(super) struct Connect {
    /// Kept, so the reports wait forever rather than end.
    _remotes: UnboundedSender<RemoteReport>,
    remote_reports: Option<UnboundedReceiver<RemoteReport>>,
}

impl Connect {
    pub(super) fn new(_spotify_tab: bool) -> Self {
        let (remotes, remote_reports) = unbounded_channel();
        Self {
            _remotes: remotes,
            remote_reports: Some(remote_reports),
        }
    }

    pub(super) fn take_reports(&mut self) -> UnboundedReceiver<RemoteReport> {
        self.remote_reports.take().expect("taken once")
    }
}

impl Worker {
    pub(super) fn handle_spotify_tab(&mut self, _command: Command) {}

    pub(super) fn start_remote(&mut self) {}

    pub(super) fn restart_remote_if_needed(&mut self) {}

    pub(super) fn stop_remote(&mut self) {}

    pub(super) fn remote_report(&mut self, report: RemoteReport) {
        match report {}
    }

    pub(super) fn connect_due(&self) -> Option<tokio::time::Instant> {
        None
    }

    pub(super) fn connect_step(&mut self) {}

    pub(super) fn is_remote(&self, _device: &str) -> bool {
        false
    }

    pub(super) fn set_remote_volume(&self, _volume: u16) {}

    pub(super) fn pause_remote_instead(&self) -> bool {
        false
    }
}
