//! What the controller and the audio output share while music plays: the
//! equaliser's settings, which the output applies to each piece of audio,
//! the output chosen in Settings, and the analyser it feeds for the
//! desktop's deck. One for each controller, handed to the engine's outputs
//! as they are made.

use std::sync::Mutex;

use super::analyser::Analyser;
use super::equaliser::Equaliser;

#[derive(Default)]
pub struct AudioLink {
    equaliser: Mutex<Equaliser>,
    /// The output chosen, by the sound server's name; None follows the
    /// system's choice.
    output: Mutex<Option<String>>,
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

    /// The output streams go to from now on (`outputs::choose` moves the
    /// one open).
    pub(crate) fn set_output(&self, output: Option<String>) {
        *self.output.lock().unwrap_or_else(|p| p.into_inner()) = output;
    }

    /// The output chosen, as a stream opens.
    #[cfg(not(target_os = "android"))]
    pub(crate) fn output(&self) -> Option<String> {
        self.output.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}
