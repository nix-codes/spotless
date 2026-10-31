//! What the controller and the audio output share while music plays: the
//! equaliser's settings, which the output applies to each piece of audio,
//! and the analyser it feeds for the desktop's deck. One for each
//! controller, handed to the engine's outputs as they are made.

use std::sync::Mutex;

use super::analyser::Analyser;
use super::equaliser::Equaliser;

#[derive(Default)]
pub struct AudioLink {
    equaliser: Mutex<Equaliser>,
    pub analyser: Analyser,
}

impl AudioLink {
    /// What the audio output applies from now on.
    pub fn set_equaliser(&self, equaliser: Equaliser) {
        *self.equaliser.lock().unwrap_or_else(|p| p.into_inner()) = equaliser;
    }

    /// The equaliser as it is now, for the next piece of audio.
    pub fn equaliser(&self) -> Equaliser {
        *self.equaliser.lock().unwrap_or_else(|p| p.into_inner())
    }
}
