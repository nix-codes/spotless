//! Sign-in and the librespot session.
//!
//! Playback signs in with Spotify's desktop client identity, the one
//! librespot streams with, through the browser (Authorization Code + PKCE on
//! a loopback port). The one-time access token is exchanged by the session
//! for a reusable credential, which librespot's cache stores in the config
//! folder. Later starts reconnect with that credential and no browser.
//!
//! The credential is kept in a private file in the config folder.

use anyhow::{Context, Result, anyhow};
use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_oauth::OAuthClientBuilder;

use crate::paths::Paths;
use crate::store::Settings;

/// Spotify's desktop client, as librespot uses it.
pub const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
/// A redirect the desktop client accepts. Port 8898 is the one spotifast
/// uses too, so only one of them can sign in at a time.
pub const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
pub const SCOPES: &[&str] = &["streaming"];

/// The audio cache as the settings have it when the session starts: off, or
/// up to its size.
fn open_cache(paths: &Paths) -> Result<Cache> {
    let settings = Settings::load(paths);
    let audio = paths.audio_cache();
    Cache::new(
        Some(paths.config.as_path()),
        None,
        settings.audio_cache.then_some(audio.as_path()),
        Some(settings.audio_cache_mb * 1024 * 1024),
    )
    .context("unable to open the playback cache")
}

fn new_session(paths: &Paths) -> Result<Session> {
    Ok(Session::new(
        SessionConfig::default(),
        Some(open_cache(paths)?),
    ))
}

/// Reconnects with the saved credential, if there is one.
pub async fn restore(paths: &Paths) -> Result<Option<Session>> {
    let Some(credentials) = open_cache(paths)?.credentials() else {
        return Ok(None);
    };
    let session = new_session(paths)?;
    session
        .connect(credentials, true)
        .await
        .context("unable to connect to Spotify")?;
    Ok(Some(session))
}

/// Opens the browser for Spotify's approval, then connects and saves the
/// reusable credential.
pub async fn sign_in(paths: &Paths) -> Result<Session> {
    let client = OAuthClientBuilder::new(CLIENT_ID, REDIRECT_URI, SCOPES.to_vec())
        .open_in_browser()
        .with_custom_message("Spotless is signed in. You can close this tab.")
        .build()
        .map_err(|e| anyhow!("unable to start sign-in: {e}"))?;
    let token = client
        .get_access_token_async()
        .await
        .map_err(|e| anyhow!("sign-in failed: {e}"))?;
    let session = new_session(paths)?;
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
