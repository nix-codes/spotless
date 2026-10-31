//! The worker's part in playing here: the engine, the volume and
//! levelling, the outputs, and pausing for the account's other devices.

use super::Worker;
use crate::backend::{Command, Event};
use crate::playback::engine::Engine;
use crate::playback::outputs;
use crate::playback::transport::Request;
use crate::spotify::devices::{self, Playing};
use librespot_core::SpotifyUri;
use std::sync::Arc;

impl Worker {
    /// Plays, seeks, sets the volume and levelling, and lists or picks outputs.
    pub(super) fn handle_player(&mut self, command: Command) {
        match command {
            Command::Player(request) => self.player(request),
            Command::Seek(position) => self.with_engine(|e| e.seek(position)),
            Command::Volume(volume) => {
                self.volume = volume;
                self.with_engine(|e| e.set_volume(volume));
                self.set_remote_volume(volume);
            }
            Command::Levelling(on) => {
                self.levelling = on;
                if let Some(engine) = &mut self.engine {
                    engine.set_levelling(on);
                }
            }
            Command::ListOutputs => {
                let emit = Arc::clone(&self.emit);
                tokio::task::spawn_blocking(move || emit(Event::Outputs(outputs::list())));
            }
            Command::Output(output) => outputs::choose(&self.link, output),
            _ => {}
        }
    }

    fn player(&self, request: Request) {
        let Some(engine) = &self.engine else { return };
        let uri = |text: &str| {
            SpotifyUri::from_uri(text)
                .map_err(|error| log::error!("not a Spotify URI {text:?}: {error}"))
                .ok()
        };
        match request {
            Request::Load {
                uri: text,
                position_ms,
                ..
            } => match uri(&text) {
                Some(track) => {
                    self.pause_elsewhere();
                    engine.load(track, position_ms);
                }
                None => (self.emit)(Event::Unavailable { offline: false }),
            },
            Request::Follow(next) => {
                engine.follow(next.and_then(|(entry, text)| Some((entry, uri(&text)?))));
            }
            Request::Resume => {
                self.pause_elsewhere();
                engine.play();
            }
            Request::Pause => engine.pause(),
            Request::Restart => engine.seek(0),
            Request::Stop => engine.stop(),
        }
    }

    fn with_engine(&self, action: impl FnOnce(&Engine)) {
        if let Some(engine) = &self.engine {
            action(engine);
        }
    }

    /// Pauses when another device starts playing.
    pub(super) fn elsewhere_changed(&mut self, playing: Option<Playing>) {
        let started = playing.as_ref().filter(|now| {
            self.elsewhere
                .as_ref()
                .is_none_or(|before| before.device != now.device)
        });
        if let Some(other) = started {
            log::info!("{} started playing", other.name);
        }
        if let (Some(other), Some(engine)) = (started, &self.engine)
            && engine.playing()
        {
            engine.pause();
            // The Spotify tab playing needs no word.
            if !self.is_remote(&other.device) {
                (self.emit)(Event::PlayingElsewhere(other.name.clone()));
            }
        }
        self.elsewhere = playing;
    }

    /// Pauses the other device playing, if one is, as Spotless starts (the
    /// Spotify tab's, if that's the one: see `pause_remote_instead`).
    pub(super) fn pause_elsewhere(&self) {
        if self.pause_remote_instead() {
            return;
        }
        let (Some(other), Some(session)) = (self.elsewhere.clone(), self.session.clone()) else {
            return;
        };
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            match devices::pause(&session, &other.device).await {
                Ok(()) => emit(Event::PausedElsewhere(other.name)),
                Err(error) => {
                    log::warn!("unable to pause {}: {error:#}", other.name);
                    emit(Event::Error(format!(
                        "Couldn't pause Spotify on {}.",
                        other.name
                    )));
                }
            }
        });
    }
}
