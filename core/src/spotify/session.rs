//! Sign-in and the librespot session.
//!
//! Playback signs in with Spotify's desktop client identity, the one
//! librespot streams with, through the browser (Authorization Code + PKCE on
//! a loopback port). The one-time access token is exchanged by the session
//! for a reusable credential, which librespot's cache stores in the config
//! folder. Later starts reconnect with that credential and no browser, as
//! does the backend when a session drops (librespot can't reuse one).
//!
//! The credential is kept in a private file in the config folder.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_oauth::OAuthClientBuilder;

use crate::browser::{self, Browser};
use crate::paths::Paths;
use crate::store::Settings;

/// Spotify's desktop client, as librespot uses it.
pub const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
/// A redirect the desktop client accepts. Port 8898 is the one spotifast
/// uses too, so only one of them can sign in at a time.
pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
pub const SCOPES: &[&str] = &["streaming"];

/// The audio cache as the settings had it when Spotless started: changed
/// since, it applies from the next start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioCache {
    pub on: bool,
    pub mb: u64,
}

impl From<&Settings> for AudioCache {
    fn from(settings: &Settings) -> Self {
        Self {
            on: settings.audio_cache,
            mb: settings.audio_cache_mb,
        }
    }
}

impl Default for AudioCache {
    fn default() -> Self {
        Self::from(&Settings::default())
    }
}

/// The credential's cache, with the audio cache off or up to its size.
fn open_cache(paths: &Paths, audio_cache: AudioCache) -> Result<Cache> {
    let audio = paths.audio_cache();
    Cache::new(
        Some(paths.config.as_path()),
        None,
        audio_cache.on.then_some(audio.as_path()),
        Some(audio_cache.mb * 1024 * 1024),
    )
    .context("unable to open the playback cache")
}

fn new_session(paths: &Paths, audio_cache: AudioCache) -> Result<Session> {
    Ok(Session::new(
        SessionConfig::default(),
        Some(open_cache(paths, audio_cache)?),
    ))
}

/// A session that isn't connected yet, with `config` and the saved
/// credential, for Spotify Connect, which connects it itself. None when
/// signed out.
#[cfg_attr(
    not(feature = "probes"),
    allow(dead_code, reason = "only a probe uses it")
)]
pub fn unconnected(
    paths: &Paths,
    audio_cache: AudioCache,
    config: SessionConfig,
) -> Result<Option<(Session, Credentials)>> {
    let cache = open_cache(paths, audio_cache)?;
    let Some(credentials) = cache.credentials() else {
        return Ok(None);
    };
    Ok(Some((Session::new(config, Some(cache)), credentials)))
}

/// Reconnects with the saved credential, if there is one.
pub async fn restore(paths: &Paths, audio_cache: AudioCache) -> Result<Option<Session>> {
    let Some(credentials) = open_cache(paths, audio_cache)?.credentials() else {
        return Ok(None);
    };
    let session = new_session(paths, audio_cache)?;
    session
        .connect(credentials, true)
        .await
        .context("unable to connect to Spotify")?;
    Ok(Some(session))
}

/// Whether a failed `restore` was Spotify refusing the saved credential,
/// rather than not being reached: signing in again is the only way on.
pub fn refused(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<librespot_core::Error>()
        .is_some_and(|e| e.kind == librespot_core::error::ErrorKind::PermissionDenied)
}

/// Opens Spotify's approval page, then connects and saves the reusable
/// credential.
pub async fn sign_in(paths: &Paths, audio_cache: AudioCache, browser: &Browser) -> Result<Session> {
    let browser = Arc::clone(browser);
    let client = OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .with_custom_message(&browser::oauth_message("Spotless is signed in."))
        .with_url_handler(move |url| browser(url))
        .build()
        .map_err(|e| anyhow!("unable to start sign-in: {e}"))?;
    let token = client
        .get_access_token_async()
        .await
        .map_err(|e| anyhow!("sign-in failed: {e}"))?;
    let session = new_session(paths, audio_cache)?;
    session
        .connect(Credentials::with_access_token(token.access_token), true)
        .await
        .context("unable to connect to Spotify")?;
    make_private(&paths.credentials());
    Ok(session)
}

/// Forgets the saved credential.
pub fn sign_out(paths: &Paths) {
    let _ = std::fs::remove_file(paths.credentials());
}

#[cfg(unix)]
fn make_private(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(error) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        log::warn!("unable to restrict {}: {error}", path.display());
    }
}

#[cfg(not(unix))]
fn make_private(_path: &std::path::Path) {}
