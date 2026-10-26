//! Audio playback through librespot, outside Spotify Connect.

#[cfg(target_os = "android")]
pub mod aaudio;
pub mod analyser;
pub mod engine;
pub mod equaliser;
pub mod outputs;
pub mod sink;
pub mod transport;
