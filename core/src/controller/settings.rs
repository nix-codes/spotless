//! The controller's part in signing in, search's app, Last.fm aside, and
//! the settings: audio (levelling, the cache, the output) and the lists'
//! looks.

use super::Controller;
use crate::action::Action;
use crate::backend::{Command, SessionState};

impl Controller {
    /// Signs in or out, and changes the settings that aren't what shows.
    pub(super) fn apply_settings(&mut self, action: Action) {
        let state = &mut self.state;
        match action {
            Action::SignIn => self.send(Command::SignIn),
            Action::SignOut => self.send(Command::SignOut),
            Action::SetPhosphor(phosphor) => {
                state.settings.phosphor = phosphor;
                self.mark_unsaved();
            }
            Action::SetGrouping(grouping) => {
                state.settings.grouping = grouping;
                self.mark_unsaved();
            }
            Action::SetLevelling(on) => {
                state.settings.levelling = on;
                self.mark_unsaved();
                self.send(Command::Levelling(on));
            }
            Action::SetAudioCache(on) => {
                state.settings.audio_cache = on;
                self.mark_unsaved();
            }
            Action::SetAudioCacheSize(mb) => {
                state.settings.audio_cache_mb = mb;
                self.mark_unsaved();
            }
            Action::MeasureStorage => self.send(Command::MeasureStorage),
            Action::NetworkBack => {
                self.send(Command::NetworkBack);
                // Spotify's word of changes made elsewhere meanwhile may
                // not have reached this device: asleep, Android cuts the
                // app's network without the session dropping.
                if matches!(self.state.session, SessionState::Ready { .. }) {
                    self.sync_on_reconnect();
                }
            }
            Action::ListOutputs => self.send(Command::ListOutputs),
            Action::SetOutput(output) => {
                state.settings.audio_output = output.clone();
                self.mark_unsaved();
                self.send(Command::Output(output));
            }
            Action::ClearCache(store) => self.send(Command::ClearCache(store)),
            Action::SetSearchApp(client_id) => {
                let client_id = Some(client_id.trim().to_string()).filter(|id| !id.is_empty());
                state.error = None;
                state.notice = Some(match (&client_id, &state.session) {
                    (None, _) => "Search's app removed.".into(),
                    (Some(_), SessionState::Ready { .. }) => {
                        "Saved. Approve search in the browser page that opens.".into()
                    }
                    (Some(_), _) => "Saved. Search is approved when you sign in.".into(),
                });
                state.settings.set_web_client_id(client_id.clone());
                self.mark_unsaved();
                self.send(Command::SearchApp(client_id));
            }
            _ => {}
        }
    }
}
