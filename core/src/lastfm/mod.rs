//! Scrobbling to Last.fm, straight from Spotless.
//!
//! Spotify's own Last.fm link only hears of plays through Spotify Connect,
//! and Spotless stays out of Connect so it never takes over the account's
//! playback and queue. So Spotless scrobbles itself, through an API
//! account the user makes on Last.fm (its key and shared secret) and a
//! one-time approval in the browser.
//!
//! The API account is the user's own because Last.fm signs every call with
//! the app's shared secret and has no sign-in without one. A key shipped
//! with Spotless would publish that secret, share one allowance among all
//! users, and stop everyone's scrobbling if Last.fm revoked it.
//!
//! The sign-in is kept in a private `lastfm.json`; listens Last.fm hasn't
//! taken yet wait in `lastfm-queue.json` and are sent when it can be
//! reached again. The songs loved there are kept in `lastfm-loved.json`,
//! read again in full now and then and their newest more often.

pub mod api;
pub mod listen;
pub mod loved;
mod scrobbler;

pub use loved::Loved;
pub use scrobbler::{Report, Scrobbler, start};

/// Where Last.fm stands, as Settings shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastFmState {
    pub status: LastFmStatus,
    /// The API account's key and secret, as last saved (empty: none).
    pub api_key: String,
    pub secret: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LastFmStatus {
    #[default]
    Off,
    /// Waiting for the browser approval.
    SigningIn,
    Connected {
        user: String,
    },
}
