//! Spotless: a minimal Spotify player in the spirit of Winamp.
//!
//! The desktop app: the window, the tray, media keys and skins around
//! `spotless-core`, whose modules are re-exported here as they were.

pub use spotless_core::{
    backend, browse, controller, demo, images, lastfm, model, paths, phosphor, playback, spotify,
    store,
};

pub mod action;
pub mod app;
pub mod media;
pub mod skin;
pub mod tray;
pub mod ui;
