//! Spotless's core: the Spotify session, playback, playlists, settings and
//! the [`Controller`] that ties them together, with no interface. The
//! desktop app draws it with egui, the Android app with Compose.
//!
//! An app names what it uses as `spotless_core::<topic>::<Item>`, never
//! deeper: each topic module shows only what the apps use, and the rest
//! stays private. The `probes` feature makes the internals public too,
//! for the live probes in `examples/` only.

#![warn(unnameable_types)]

/// Declares modules the core's own, or public with the `probes` feature.
macro_rules! internal {
    ($($(#[$meta:meta])* mod $name:ident;)*) => {$(
        $(#[$meta])*
        #[cfg(feature = "probes")]
        pub mod $name;
        $(#[$meta])*
        #[cfg(not(feature = "probes"))]
        pub(crate) mod $name;
    )*};
}

pub mod action;
pub mod browse;
pub mod demo;
pub mod lastfm;
pub mod model;
pub mod playback;
pub mod session;
pub mod settings;
pub mod spotify;

internal! {
    mod backend;
    mod browser;
    mod controller;
    mod images;
    mod paths;
    mod phosphor;
    mod store;
}

pub use controller::{Controller, State, SyncStatus};
pub use images::{Cover, CoverImage};
pub use paths::Paths;
