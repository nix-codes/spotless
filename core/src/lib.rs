//! Spotless's core: the Spotify session, playback, playlists, settings and
//! the [`controller::Controller`] that ties them together, with no
//! interface. The desktop app draws it with egui; an Android app will
//! draw it too.

pub mod action;
pub mod backend;
pub mod browse;
pub mod browser;
pub mod controller;
pub mod images;
pub mod lastfm;
pub mod model;
pub mod paths;
pub mod playback;
pub mod spotify;
pub mod store;
