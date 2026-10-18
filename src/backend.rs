//! The bridge between the UI thread and asynchronous work.
//!
//! egui runs on the main thread and must never block. A tokio runtime on its
//! own thread hosts the session, the player and catalogue reads. The UI sends
//! [`Command`]s; the worker answers with [`Event`]s and wakes the UI with
//! `request_repaint`, so the app stays idle when nothing happens.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Result;
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::model::{EntryId, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::playback::transport::Request;
use crate::spotify::{catalog, session};

#[derive(Debug)]
pub enum Command {
    SignIn,
    SignOut,
    /// Reads the songs of a track, album or artist link for a playlist.
    AddLink {
        text: String,
        playlist: PlaylistId,
    },
    Player(Request),
    Seek(u32),
    Volume(u16),
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Connecting,
    SignedOut,
    /// Waiting for the browser approval.
    SigningIn,
    Ready {
        user: String,
    },
    Failed(String),
}

#[derive(Debug)]
pub enum Event {
    Session(SessionState),
    Player(PlayerSnapshot),
    /// The engine started the followed song by itself.
    Followed(EntryId),
    /// The engine finished a song with nothing to follow, and stopped.
    Ended,
    Unavailable,
    /// The songs of an `AddLink`.
    Tracks {
        playlist: PlaylistId,
        tracks: Vec<TrackRef>,
    },
    Error(String),
}

pub struct Backend {
    commands: UnboundedSender<Command>,
    pub events: mpsc::Receiver<Event>,
}

impl Backend {
    pub fn start(paths: Paths, volume: u16, ctx: egui::Context) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("spotless-backend")
            .build()?;
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        let emit: Emit = Arc::new(move |event| {
            let _ = sender.send(event);
            ctx.request_repaint();
        });
        std::thread::Builder::new()
            .name("spotless-runtime".into())
            .spawn(move || {
                runtime.block_on(Worker::new(paths, volume, emit).run(receiver));
            })?;
        Ok(Self { commands, events })
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::error!("the backend has stopped");
        }
    }
}

type Emit = Arc<dyn Fn(Event) + Send + Sync>;

struct Worker {
    paths: Paths,
    volume: u16,
    emit: Emit,
    session: Option<Session>,
    engine: Option<Engine>,
}

impl Worker {
    fn new(paths: Paths, volume: u16, emit: Emit) -> Self {
        Self {
            paths,
            volume,
            emit,
            session: None,
            engine: None,
        }
    }

    async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        (self.emit)(Event::Session(SessionState::Connecting));
        match session::restore(&self.paths).await {
            Ok(Some(session)) => self.ready(session),
            Ok(None) => (self.emit)(Event::Session(SessionState::SignedOut)),
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
        while let Some(command) = commands.recv().await {
            self.handle(command).await;
        }
    }

    fn ready(&mut self, session: Session) {
        let emit = Arc::clone(&self.emit);
        match Engine::new(
            &session,
            self.volume,
            Arc::new(move |event| {
                emit(match event {
                    EngineEvent::Snapshot(snapshot) => Event::Player(snapshot),
                    EngineEvent::Followed(entry) => Event::Followed(entry),
                    EngineEvent::Ended => Event::Ended,
                    EngineEvent::Unavailable => Event::Unavailable,
                })
            }),
        ) {
            Ok(engine) => {
                (self.emit)(Event::Session(SessionState::Ready {
                    user: session.username(),
                }));
                self.engine = Some(engine);
                self.session = Some(session);
            }
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn => {
                (self.emit)(Event::Session(SessionState::SigningIn));
                match session::sign_in(&self.paths).await {
                    Ok(session) => self.ready(session),
                    Err(error) => {
                        (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}"))))
                    }
                }
            }
            Command::SignOut => {
                if let Some(engine) = self.engine.take() {
                    engine.stop();
                }
                if let Some(session) = self.session.take() {
                    session.shutdown();
                }
                session::sign_out(&self.paths);
                (self.emit)(Event::Player(PlayerSnapshot::default()));
                (self.emit)(Event::Session(SessionState::SignedOut));
            }
            Command::AddLink { text, playlist } => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let emit = Arc::clone(&self.emit);
                // Album reads take a moment; commands go on meanwhile.
                tokio::spawn(async move {
                    match catalog::link_tracks(&session, &text).await {
                        Ok(tracks) => emit(Event::Tracks { playlist, tracks }),
                        Err(error) => emit(Event::Error(format!("{error:#}"))),
                    }
                });
            }
            Command::Player(request) => self.player(request),
            Command::Seek(position) => self.with_engine(|e| e.seek(position)),
            Command::Volume(volume) => {
                self.volume = volume;
                self.with_engine(|e| e.set_volume(volume));
            }
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
                Some(track) => engine.load(track, position_ms),
                None => (self.emit)(Event::Unavailable),
            },
            Request::Follow(next) => {
                engine.follow(next.and_then(|(entry, text)| Some((entry, uri(&text)?))));
            }
            Request::Resume => engine.play(),
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
}
