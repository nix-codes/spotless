//! The application: owns the state, draws the views, then applies their
//! actions.

use std::time::{Duration, Instant};

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SessionState};
use crate::model::{Library, Playback, PlaylistId};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status};
use crate::playback::transport::{Request, Transport};
use crate::store::{self, Settings};
use crate::ui::{self, Inputs, View, theme::Palette};

/// Changes are saved this long after the last one.
const SAVE_DELAY: Duration = Duration::from_secs(1);

pub struct App {
    paths: Paths,
    backend: Option<Backend>,
    palette: Palette,
    session: SessionState,
    player: PlayerSnapshot,
    error: Option<String>,
    notice: Option<String>,
    inputs: Inputs,
    library: Library,
    transport: Transport,
    /// The playlist shown in the editor.
    viewed: PlaylistId,
    settings: Settings,
    /// When the first unsaved change was made.
    unsaved: Option<Instant>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> Self {
        let palette = Palette::CLASSIC;
        palette.apply(&cc.egui_ctx);
        let settings = Settings::load(&paths);
        let (library, playing) = store::load_library(&paths);
        let mut playback = Playback::new(seed());
        playback.repeat = settings.repeat;
        playback.set_shuffle(settings.shuffle);

        let volume = volume_level(settings.volume);
        let (backend, session) = match Backend::start(paths.clone(), volume, cc.egui_ctx.clone()) {
            Ok(backend) => (Some(backend), SessionState::Connecting),
            Err(error) => (None, SessionState::Failed(format!("{error:#}"))),
        };
        Self {
            backend,
            palette,
            session,
            player: PlayerSnapshot::default(),
            error: None,
            notice: None,
            inputs: Inputs {
                volume: settings.volume,
                ..Inputs::default()
            },
            library,
            transport: Transport::new(playback),
            viewed: playing.unwrap_or(PlaylistId::DEFAULT),
            settings,
            unsaved: None,
            paths,
        }
    }

    fn receive(&mut self) {
        let Some(backend) = &self.backend else { return };
        let events: Vec<Event> = backend.events.try_iter().collect();
        for event in events {
            match event {
                Event::Session(state) => {
                    self.error = None;
                    if !matches!(state, SessionState::Ready { .. }) {
                        // The engine is gone with the session.
                        self.transport.stop(&mut self.library);
                        self.player = PlayerSnapshot::default();
                    }
                    self.session = state;
                }
                Event::Player(snapshot) => {
                    if snapshot.error.is_some() {
                        self.error = snapshot.error.clone();
                    }
                    self.player = snapshot;
                }
                Event::Followed(entry) => {
                    let requests = self.transport.followed(&mut self.library, entry);
                    self.changed(requests);
                }
                Event::Ended => {
                    let requests = self.transport.ended(&mut self.library);
                    self.changed(requests);
                }
                Event::Unavailable => {
                    let requests = self.transport.unavailable(&mut self.library);
                    self.changed(requests);
                }
                Event::Tracks { playlist, tracks } => {
                    let count = tracks.len();
                    let entries = tracks
                        .into_iter()
                        .map(|track| self.library.entry(track))
                        .collect();
                    if let Some(list) = self.library.get_mut(playlist) {
                        list.append(entries);
                        self.notice = Some(match count {
                            1 => "Added 1 song".into(),
                            n => format!("Added {n} songs"),
                        });
                    }
                    let requests = self.transport.edited(&mut self.library, playlist);
                    self.changed(requests);
                }
                Event::Error(message) => self.error = Some(message),
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let paused = self.player.status == Status::Paused;
        let position = self.player.position_now();
        let library = &mut self.library;
        let viewed = self.viewed;
        let requests = match action {
            Action::SignIn => return self.send(Command::SignIn),
            Action::SignOut => return self.send(Command::SignOut),
            Action::AddLink(text) => {
                self.error = None;
                self.notice = Some("Reading the link…".into());
                return self.send(Command::AddLink {
                    text,
                    playlist: viewed,
                });
            }
            Action::Seek(ms) => return self.send(Command::Seek(ms)),
            Action::Volume(volume) => {
                self.settings.volume = volume;
                self.mark_unsaved();
                return self.send(Command::Volume(volume_level(volume)));
            }
            Action::Play => self.transport.play(library, viewed, paused, position),
            Action::TogglePause => self.transport.toggle_pause(paused),
            Action::Stop => self.transport.stop(library),
            Action::Next => self.transport.next(library, viewed),
            Action::Previous => self.transport.prev(library, viewed, position),
            Action::SetShuffle(on) => {
                self.settings.shuffle = on;
                self.transport.set_shuffle(library, on)
            }
            Action::SetRepeat(on) => {
                self.settings.repeat = on;
                self.transport.set_repeat(library, on)
            }
            Action::PlayEntry(entry) => {
                self.error = None;
                self.transport.play_entry(library, viewed, entry, position)
            }
            Action::Remove(entries) => {
                if let Some(list) = library.get_mut(viewed) {
                    list.remove(&entries);
                }
                self.transport.edited(library, viewed)
            }
            Action::Move(entries, index) => {
                if let Some(list) = library.get_mut(viewed) {
                    list.move_to(&entries, index);
                }
                self.transport.edited(library, viewed)
            }
            Action::ToggleMarks(entries) => {
                if let Some(list) = library.get_mut(viewed) {
                    for entry in entries {
                        list.toggle_mark(entry);
                    }
                }
                self.transport.edited(library, viewed)
            }
        };
        self.changed(requests);
    }

    /// The model changed: send what the engine must do, and save soon.
    fn changed(&mut self, requests: Vec<Request>) {
        for request in requests {
            self.send(Command::Player(request));
        }
        self.mark_unsaved();
    }

    fn mark_unsaved(&mut self) {
        self.unsaved.get_or_insert_with(Instant::now);
    }

    fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    fn save(&mut self) {
        self.unsaved = None;
        self.transport
            .remember_position(&mut self.library, self.player.position_now());
        let playing = self.transport.playback.playing();
        if let Err(error) = store::save_library(&self.paths, &self.library, playing) {
            log::error!("{error:#}");
            self.error = Some("Couldn't save the playlists.".into());
        }
        if let Err(error) = self.settings.save(&self.paths) {
            log::error!("{error:#}");
        }
    }
}

/// The slider's 0–100 as librespot's 0–65535.
fn volume_level(percent: u8) -> u16 {
    (u32::from(percent.min(100)) * u32::from(u16::MAX) / 100) as u16
}

/// A different shuffle every run.
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let mut actions = Vec::new();
        let playlist = self
            .library
            .get(self.viewed)
            .or_else(|| self.library.get(PlaylistId::DEFAULT))
            .expect("Default always exists");
        let view = View {
            palette: self.palette,
            session: &self.session,
            player: &self.player,
            error: self.error.as_deref(),
            notice: self.notice.as_deref(),
            playlist,
            shuffle: self.transport.playback.shuffle(),
            repeat: self.transport.playback.repeat,
        };
        ui::draw(ui, &view, &mut self.inputs, &mut actions);
        for action in actions {
            self.apply(action);
        }
        // The clock moves on its own while a song plays.
        if self.player.status == Status::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
        if let Some(since) = self.unsaved {
            match SAVE_DELAY.checked_sub(since.elapsed()) {
                Some(wait) if !wait.is_zero() => ui.ctx().request_repaint_after(wait),
                _ => self.save(),
            }
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.desk.to_normalized_gamma_f32()
    }
}

#[cfg(test)]
mod tests {
    use super::volume_level;

    #[test]
    fn volume_spans_librespots_range() {
        assert_eq!(volume_level(0), 0);
        assert_eq!(volume_level(100), u16::MAX);
        assert_eq!(volume_level(250), u16::MAX);
    }
}
