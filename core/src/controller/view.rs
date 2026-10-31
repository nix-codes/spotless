//! The controller's part in what shows: the playlists, the equaliser and
//! the deck below the player, the deck's media and meters, and the time's
//! mode; and the equaliser's settings and presets.

use super::Controller;
use crate::action::ViewAction;
use crate::playback::equaliser::Preset;

impl Controller {
    /// Shows or hides what's below the player, and sets how it shows.
    pub(super) fn apply_view(&mut self, action: ViewAction) {
        match action {
            ViewAction::TogglePlaylists => {
                self.state.settings.playlists = !self.state.settings.playlists;
                self.mark_unsaved();
            }
            ViewAction::ToggleEqualiser => {
                self.state.settings.equaliser_shown = !self.state.settings.equaliser_shown;
                self.mark_unsaved();
            }
            ViewAction::ToggleDeck => {
                self.state.settings.deck_shown = !self.state.settings.deck_shown;
                self.mark_unsaved();
            }
            ViewAction::SetDeckMedia(media) => {
                self.state.settings.deck_media = media;
                self.mark_unsaved();
            }
            ViewAction::SetDeckMeter(meter) => {
                self.state.settings.deck_meter = meter;
                self.mark_unsaved();
            }
            ViewAction::SetVuFace(face) => {
                self.state.settings.vu_face = face;
                self.mark_unsaved();
            }
            ViewAction::SetVuCalibration(calibration) => {
                self.state.settings.vu_calibration = calibration;
                self.mark_unsaved();
            }
            ViewAction::SetCassetteMix(mix) => {
                self.state.settings.cassette_mix = mix;
                self.mark_unsaved();
            }
            ViewAction::SetVinylLp(lp) => {
                self.state.settings.vinyl_lp = lp;
                self.mark_unsaved();
            }
            ViewAction::SetReelSlow(slow) => {
                self.state.settings.reel_slow = slow;
                self.mark_unsaved();
            }
            ViewAction::SetCdMini(mini) => {
                self.state.settings.cd_mini = mini;
                self.mark_unsaved();
            }
            ViewAction::ToggleTimeLeft => {
                self.state.settings.time_left = !self.state.settings.time_left;
                self.mark_unsaved();
            }
            ViewAction::SetEqualiser(equaliser) => {
                let equaliser = equaliser.clamped();
                self.state.settings.equaliser = equaliser;
                self.audio.set_equaliser(equaliser);
                self.mark_unsaved();
            }
            ViewAction::SaveEqualiserPreset(name) => {
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
            ViewAction::DeleteEqualiserPreset(name) => {
                self.state
                    .settings
                    .equaliser_presets
                    .retain(|preset| preset.name != name);
                self.mark_unsaved();
            }
        }
    }
}
