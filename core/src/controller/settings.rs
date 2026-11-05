//! The controller's part in signing in, search's app, Last.fm aside, and
//! the settings: audio (levelling, the cache, the output) and the lists'
//! looks.

use super::Controller;
use crate::action::SettingsAction;
use crate::backend::{Command, SessionState};

impl Controller {
    /// Signs in or out, and changes the settings that aren't what shows.
    pub(super) fn apply_settings(&mut self, action: SettingsAction) {
        let state = &mut self.state;
        match action {
            SettingsAction::SignIn => self.send(Command::SignIn),
            SettingsAction::SignOut => self.send(Command::SignOut),
            SettingsAction::SetPhosphor(phosphor) => {
                state.settings.phosphor = phosphor;
                self.mark_unsaved();
            }
            SettingsAction::SetLevelling(on) => {
                state.settings.levelling = on;
                self.mark_unsaved();
                self.send(Command::Levelling(on));
            }
            SettingsAction::SetAudioCache(on) => {
                state.settings.audio_cache = on;
                self.mark_unsaved();
            }
            SettingsAction::SetAudioCacheSize(mb) => {
                state.settings.audio_cache_mb = mb;
                self.mark_unsaved();
            }
            SettingsAction::MeasureStorage => self.send(Command::MeasureStorage),
            SettingsAction::NetworkBack => {
                self.send(Command::NetworkBack);
                // Spotify's word of changes made elsewhere meanwhile may
                // not have reached this device: asleep, Android cuts the
                // app's network without the session dropping.
                if matches!(self.state.session, SessionState::Ready { .. }) {
                    self.sync_on_reconnect();
                }
            }
            SettingsAction::ListOutputs => self.send(Command::ListOutputs),
            SettingsAction::SetOutput(output) => {
                state.settings.audio_output = output.clone();
                self.mark_unsaved();
                self.send(Command::Output(output));
            }
            SettingsAction::ClearCache(store) => self.send(Command::ClearCache(store)),
            SettingsAction::SetSearchApp(client_id) => {
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
        }
    }
}
