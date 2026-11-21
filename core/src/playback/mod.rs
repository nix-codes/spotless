//! Audio playback, outside Spotify Connect.

internal! {
    #[cfg(target_os = "android")]
    mod aaudio;
    mod analyser;
    mod audio_sink;
    #[cfg_attr(
        not(feature = "probes"),
        allow(dead_code, reason = "until the engine plays through it, only a probe does")
    )]
    mod decoder;
    mod engine;
    mod equaliser;
    #[cfg_attr(
        not(feature = "probes"),
        allow(dead_code, reason = "until the engine plays through it, only a probe does")
    )]
    mod flow;
    #[cfg_attr(
        not(feature = "probes"),
        allow(dead_code, reason = "until the engine plays through it, only a probe does")
    )]
    mod levelling;
    mod link;
    mod outputs;
    #[cfg_attr(
        not(feature = "probes"),
        allow(dead_code, reason = "until the engine plays through it, only a probe does")
    )]
    mod player;
    #[cfg(not(target_os = "android"))]
    mod sink;
    #[cfg_attr(
        not(feature = "probes"),
        allow(dead_code, reason = "until the engine plays through it, only a probe does")
    )]
    mod source;
    mod transport;
}

/// The rate the player plays at, and outputs open at.
pub(crate) const SAMPLE_RATE: u32 = 44_100;
/// Stereo, interleaved.
pub(crate) const CHANNELS: u16 = 2;

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
