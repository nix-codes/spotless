//! The bridge between the UI thread and asynchronous work.
//!
//! egui runs on the main thread and must never block. A tokio runtime on its
//! own thread hosts the session, the player and catalogue reads. The UI sends
//! [`Command`]s; the worker answers with [`Event`]s and wakes the UI with
//! `request_repaint`, so the app stays idle when nothing happens.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Result;
use librespot_core::session::Session;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::paths::Paths;
use crate::playback::engine::{Engine, PlayerSnapshot};
use crate::spotify::{catalog, session};

#[derive(Debug)]
pub enum Command {
    SignIn,
    SignOut,
    /// A track, album or artist link; M0 plays its first track.
    PlayLink(String),
    Play,
    Pause,
    Stop,
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
            Arc::new(move |s| emit(Event::Player(s))),
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
            Command::PlayLink(text) => {
                let (Some(session), Some(engine)) = (&self.session, &self.engine) else {
                    return;
                };
                let resolved = match catalog::parse_uri(&text) {
                    Ok(uri) => catalog::first_track(session, uri).await,
                    Err(error) => Err(error),
                };
                match resolved {
                    Ok(track) => engine.load(track),
                    Err(error) => (self.emit)(Event::Error(format!("{error:#}"))),
                }
            }
            Command::Play => self.with_engine(Engine::play),
            Command::Pause => self.with_engine(Engine::pause),
            Command::Stop => self.with_engine(Engine::stop),
            Command::Seek(position) => self.with_engine(|e| e.seek(position)),
            Command::Volume(volume) => {
                self.volume = volume;
                self.with_engine(|e| e.set_volume(volume));
            }
        }
    }

    fn with_engine(&self, action: impl FnOnce(&Engine)) {
        if let Some(engine) = &self.engine {
            action(engine);
        }
    }
}
