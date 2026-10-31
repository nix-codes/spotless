//! Audio playback through librespot, outside Spotify Connect.

internal! {
    #[cfg(target_os = "android")]
    mod aaudio;
    mod analyser;
    mod engine;
    mod equaliser;
    mod link;
    mod outputs;
    #[cfg(not(target_os = "android"))]
    mod sink;
    mod transport;
}

/// Told why the output failed, in words for the status bar.
pub(crate) type ErrorHook = Box<dyn Fn(String) + Send>;

pub(crate) const NO_DEVICE: &str =
    "No audio output was found. Connect or enable one, then press Play.";
pub(crate) const STALLED: &str = "The audio output stopped working. Press Play to try again.";

pub use analyser::{Analyser, BANDS, Reading, SILENT, centre, place};
pub use engine::{PlayerSnapshot, Status, TrackInfo};
pub use equaliser::{Equaliser, PRESETS, Preset, RANGE};
pub use link::AudioLink;
pub use outputs::Output;
