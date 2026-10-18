//! The application: owns the state, draws the views, then applies their
//! actions.

use std::time::Duration;

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SessionState};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status};
use crate::ui::{self, Inputs, View, theme::Palette};

const DEFAULT_VOLUME: u8 = 70;

pub struct App {
    backend: Option<Backend>,
    palette: Palette,
    session: SessionState,
    player: PlayerSnapshot,
    error: Option<String>,
    inputs: Inputs,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Paths) -> Self {
        let palette = Palette::CLASSIC;
        palette.apply(&cc.egui_ctx);
        let (backend, session) =
            match Backend::start(paths, volume_level(DEFAULT_VOLUME), cc.egui_ctx.clone()) {
                Ok(backend) => (Some(backend), SessionState::Connecting),
                Err(error) => (None, SessionState::Failed(format!("{error:#}"))),
            };
        Self {
            backend,
            palette,
            session,
            player: PlayerSnapshot::default(),
            error: None,
            inputs: Inputs {
                volume: DEFAULT_VOLUME,
                ..Inputs::default()
            },
        }
    }

    fn receive(&mut self) {
        let Some(backend) = &self.backend else { return };
        while let Ok(event) = backend.events.try_recv() {
            match event {
                Event::Session(state) => {
                    self.error = None;
                    self.session = state;
                }
                Event::Player(snapshot) => {
                    if snapshot.error.is_some() {
                        self.error = snapshot.error.clone();
                    }
                    self.player = snapshot;
                }
                Event::Error(message) => self.error = Some(message),
            }
        }
    }

    fn apply(&mut self, action: Action) {
        let command = match action {
            Action::SignIn => Command::SignIn,
            Action::SignOut => Command::SignOut,
            Action::PlayLink(link) => {
                self.error = None;
                Command::PlayLink(link)
            }
            Action::Play => Command::Play,
            Action::Pause => Command::Pause,
            Action::Stop => Command::Stop,
            Action::Seek(ms) => Command::Seek(ms),
            Action::Volume(volume) => Command::Volume(volume_level(volume)),
        };
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }
}

/// The slider's 0–100 as librespot's 0–65535.
fn volume_level(percent: u8) -> u16 {
    (u32::from(percent.min(100)) * u32::from(u16::MAX) / 100) as u16
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let mut actions = Vec::new();
        let view = View {
            palette: self.palette,
            session: &self.session,
            player: &self.player,
            error: self.error.as_deref(),
        };
        ui::draw(ui, &view, &mut self.inputs, &mut actions);
        for action in actions {
            self.apply(action);
        }
        // The clock moves on its own while a song plays.
        if self.player.status == Status::Playing {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
        }
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
