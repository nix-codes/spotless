//! The controller's part in what shows: the playlists, the equaliser and
//! the deck below the player, the deck's media and meters, and the time's
//! mode; and the equaliser's settings and presets.

use super::Controller;
use crate::action::Action;
use crate::playback::equaliser::{self, Preset};

impl Controller {
    /// Shows or hides what's below the player, and sets how it shows.
    pub(super) fn apply_view(&mut self, action: Action) {
        match action {
            Action::TogglePlaylists => {
                self.state.settings.playlists = !self.state.settings.playlists;
                self.mark_unsaved();
            }
            Action::ToggleEqualiser => {
                self.state.settings.equaliser_shown = !self.state.settings.equaliser_shown;
                self.mark_unsaved();
            }
            Action::ToggleDeck => {
                self.state.settings.deck_shown = !self.state.settings.deck_shown;
                self.mark_unsaved();
            }
            Action::SetDeckMedia(media) => {
                self.state.settings.deck_media = media;
                self.mark_unsaved();
            }
            Action::SetDeckMeter(meter) => {
                self.state.settings.deck_meter = meter;
                self.mark_unsaved();
            }
            Action::SetVuFace(face) => {
                self.state.settings.vu_face = face;
                self.mark_unsaved();
            }
            Action::SetVuCalibration(calibration) => {
                self.state.settings.vu_calibration = calibration;
                self.mark_unsaved();
            }
            Action::SetCassetteMix(mix) => {
                self.state.settings.cassette_mix = mix;
                self.mark_unsaved();
            }
            Action::SetVinylLp(lp) => {
                self.state.settings.vinyl_lp = lp;
                self.mark_unsaved();
            }
            Action::SetReelSlow(slow) => {
                self.state.settings.reel_slow = slow;
                self.mark_unsaved();
            }
            Action::SetCdMini(mini) => {
                self.state.settings.cd_mini = mini;
                self.mark_unsaved();
            }
            Action::ToggleTimeLeft => {
                self.state.settings.time_left = !self.state.settings.time_left;
                self.mark_unsaved();
            }
            Action::SetEqualiser(equaliser) => {
                let equaliser = equaliser.clamped();
                self.state.settings.equaliser = equaliser;
                equaliser::set(equaliser);
                self.mark_unsaved();
            }
            Action::SaveEqualiserPreset(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    let presets = &mut self.state.settings.equaliser_presets;
                    let bands = self.state.settings.equaliser.bands;
                    match presets.iter_mut().find(|preset| preset.name == name) {
                        Some(preset) => preset.bands = bands,
                        None => presets.push(Preset { name, bands }),
                    }
                    self.mark_unsaved();
                }
            }
            Action::DeleteEqualiserPreset(name) => {
                self.state
                    .settings
                    .equaliser_presets
                    .retain(|preset| preset.name != name);
                self.mark_unsaved();
            }
            _ => {}
        }
    }
}
