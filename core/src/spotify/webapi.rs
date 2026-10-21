//! Search through the Web API with the user's personal Spotify app.
//!
//! The playback session can't search, as `examples/catalog_probe.rs` found:
//! Spotify refuses the session's keymaster token (403), always rate-limits
//! its Login5 token on the Web API (429: the desktop client ID's quota is
//! shared by every librespot app), and rejects the session's own
//! `searchview` endpoint (400). So search signs in separately with the
//! personal app's client ID (`web_client_id` in settings.json). The browser
//! approval happens once; the refresh token it gives is kept in a private
//! file and renewed as Spotify rotates it.
//! Spotless never shares it with spotifast, since a rotation would sign the
//! other app out.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use librespot_oauth::{OAuthClient, OAuthClientBuilder, OAuthToken};
use serde_json::Value;

use crate::model::catalog::{Release, ReleaseGroup, SearchResults};
use crate::model::{AlbumRef, ArtistRef, TrackRef};
use crate::store::save_private;

/// The redirect a personal app registers (spotifast uses the same one, so
/// only one of them can sign in at a time).
pub const REDIRECT_URI: &str = "http://127.0.0.1:8989/login";
/// Search needs no scope; one is asked for because the sign-in needs some.
const SCOPES: &[&str] = &["user-read-private"];

const ARTIST_LIMIT: usize = 5;
const RESULT_LIMIT: &str = "10";

pub struct WebApi {
    client_id: String,
    /// Where the refresh token is kept.
    path: PathBuf,
    refresh_token: Option<String>,
    access: Option<(String, Instant)>,
    http: reqwest::Client,
}

impl WebApi {
    pub fn new(client_id: String, path: PathBuf) -> Self {
        let refresh_token = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|saved| saved["refresh_token"].as_str().map(String::from))
            .filter(|token| !token.is_empty());
        Self {
            client_id,
            path,
            refresh_token,
            access: None,
            http: reqwest::Client::new(),
        }
    }

    /// Search has been approved once.
    pub fn signed_in(&self) -> bool {
        self.refresh_token.is_some()
    }

    /// The browser approval, to run without holding the client: it waits
    /// for as long as the user takes. Hand its token to [`Self::keep`].
    pub fn approval(&self) -> Result<OAuthClient> {
        self.client(true)
    }

    pub fn sign_out(&mut self) {
        self.refresh_token = None;
        self.access = None;
        let _ = std::fs::remove_file(&self.path);
    }

    pub async fn search(&mut self, query: &str) -> Result<SearchResults> {
        let token = self.access_token().await?;
        let response = self
            .http
            .get("https://api.spotify.com/v1/search")
            .query(&[
                ("q", query),
                ("type", "artist,album,track"),
                ("limit", RESULT_LIMIT),
            ])
            .bearer_auth(token)
            .send()
            .await
            .context("Spotify can't be reached")?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let wait = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            bail!(match wait {
                Some(seconds) => format!("Spotify is busy. Try again in {seconds} s."),
                None => "Spotify is busy. Try again shortly.".into(),
            });
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.access = None;
        }
        let body: Value = response.json().await.unwrap_or_default();
        if !status.is_success() {
            let message = body["error"]["message"].as_str().unwrap_or("no message");
            bail!("search failed: HTTP {status}: {message}");
        }
        Ok(parse_search(&body))
    }

    async fn access_token(&mut self) -> Result<String> {
        if let Some((token, expires)) = &self.access
            && *expires > Instant::now() + Duration::from_secs(60)
        {
            return Ok(token.clone());
        }
        let Some(refresh_token) = self.refresh_token.clone() else {
            bail!("search isn't connected");
        };
        match self
            .client(false)?
            .refresh_token_async(&refresh_token)
            .await
        {
            Ok(token) => {
                self.keep(token)?;
                Ok(self
                    .access
                    .as_ref()
                    .map(|(t, _)| t.clone())
                    .unwrap_or_default())
            }
            Err(error) => {
                log::warn!("search token refresh failed: {error}");
                // A revoked, rotated-away or missing token: approve again
                // on the next search.
                let message = error.to_string();
                if message.contains("invalid_grant") || message.contains("invalid_request") {
                    self.sign_out();
                }
                bail!("search sign-in expired: {error}")
            }
        }
    }

    fn client(&self, browser: bool) -> Result<OAuthClient> {
        let mut builder = OAuthClientBuilder::new(&self.client_id, REDIRECT_URI, SCOPES.to_vec())
            .with_custom_message("Spotless search is connected. You can close this tab.");
        if browser {
            builder = builder.open_in_browser();
        }
        builder.build().map_err(|e| anyhow!("{e}"))
    }

    pub fn keep(&mut self, token: OAuthToken) -> Result<()> {
        self.access = Some((token.access_token, token.expires_at));
        // A refresh may not rotate the refresh token: Spotify then sends
        // none, which librespot reports as empty. The old one stays valid.
        if token.refresh_token.is_empty() {
            return Ok(());
        }
        if self.refresh_token.as_deref() != Some(token.refresh_token.as_str()) {
            self.refresh_token = Some(token.refresh_token.clone());
            save_private(
                &self.path,
                &serde_json::to_vec(&serde_json::json!({ "refresh_token": token.refresh_token }))?,
            )?;
        }
        Ok(())
    }
}

fn parse_search(body: &Value) -> SearchResults {
    let items = |key: &str| body[key]["items"].as_array().cloned().unwrap_or_default();
    SearchResults {
        artists: items("artists")
            .iter()
            .filter_map(artist_ref)
            .take(ARTIST_LIMIT)
            .collect(),
        albums: items("albums").iter().filter_map(release).collect(),
        songs: items("tracks").iter().filter_map(track_ref).collect(),
    }
}

fn artist_ref(item: &Value) -> Option<ArtistRef> {
    Some(ArtistRef {
        uri: item["uri"].as_str()?.into(),
        name: item["name"].as_str()?.into(),
    })
}

fn artist_names(item: &Value) -> String {
    item["artists"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn release(item: &Value) -> Option<Release> {
    let tracks = item["total_tracks"].as_u64().unwrap_or(0) as usize;
    let album_type = item["album_type"].as_str().unwrap_or("album");
    Some(Release {
        uri: item["uri"].as_str()?.into(),
        title: item["name"].as_str()?.into(),
        artists: artist_names(item),
        year: item["release_date"]
            .as_str()
            .and_then(|date| date.get(..4))
            .and_then(|year| year.parse().ok())
            .unwrap_or(0),
        group: match album_type {
            "single" => ReleaseGroup::Single,
            "compilation" => ReleaseGroup::Compilation,
            _ => ReleaseGroup::Album,
        },
        kind: Release::kind_label(album_type, tracks),
        tracks,
        cover: cover_url(item),
    })
}

/// The image nearest 300 px wide.
fn cover_url(item: &Value) -> Option<String> {
    item["images"]
        .as_array()?
        .iter()
        .filter_map(|image| Some((image["width"].as_i64().unwrap_or(0), image["url"].as_str()?)))
        .min_by_key(|(width, _)| (width - 300).abs())
        .map(|(_, url)| url.to_string())
}

fn track_ref(item: &Value) -> Option<TrackRef> {
    Some(TrackRef {
        uri: item["uri"].as_str()?.into(),
        title: item["name"].as_str()?.into(),
        artists: item["artists"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(artist_ref)
            .collect(),
        album: AlbumRef {
            uri: item["album"]["uri"].as_str().unwrap_or_default().into(),
            name: item["album"]["name"].as_str().unwrap_or_default().into(),
        },
        duration_ms: item["duration_ms"].as_u64().unwrap_or(0) as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(refresh: &str) -> OAuthToken {
        OAuthToken {
            access_token: "access".into(),
            refresh_token: refresh.into(),
            expires_at: Instant::now() + Duration::from_secs(3600),
            token_type: "Bearer".into(),
            scopes: vec![],
        }
    }

    #[test]
    fn a_refresh_without_a_new_refresh_token_keeps_the_old_one() {
        let dir = std::env::temp_dir().join(format!("spotless-webapi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("search-credentials.json");
        let mut web = WebApi::new("id".into(), path.clone());
        web.keep(token("first")).unwrap();
        web.keep(token("")).unwrap();
        let again = WebApi::new("id".into(), path.clone());
        assert_eq!(again.refresh_token.as_deref(), Some("first"));
        web.keep(token("second")).unwrap();
        assert_eq!(
            WebApi::new("id".into(), path.clone())
                .refresh_token
                .as_deref(),
            Some("second")
        );
        // A file saved empty by the old bug counts as not approved.
        std::fs::write(&path, br#"{"refresh_token":""}"#).unwrap();
        assert!(!WebApi::new("id".into(), path).signed_in());
    }

    #[test]
    fn search_results_parse_into_three_sections() {
        let body = serde_json::json!({
            "artists": {"items": [{"uri": "spotify:artist:a", "name": "Boards of Canada"}]},
            "albums": {"items": [{
                "uri": "spotify:album:b", "name": "Twoism", "album_type": "single",
                "total_tracks": 8, "release_date": "1995-11-01",
                "images": [
                    {"url": "https://i.scdn.co/image/big", "width": 640},
                    {"url": "https://i.scdn.co/image/mid", "width": 300},
                    {"url": "https://i.scdn.co/image/small", "width": 64}
                ],
                "artists": [{"uri": "spotify:artist:a", "name": "Boards of Canada"}]
            }]},
            "tracks": {"items": [{
                "uri": "spotify:track:c", "name": "Roygbiv", "duration_ms": 151000,
                "album": {"uri": "spotify:album:d", "name": "Music Has the Right to Children"},
                "artists": [{"uri": "spotify:artist:a", "name": "Boards of Canada"}]
            }, {"name": "no uri"}]}
        });
        let results = parse_search(&body);
        assert_eq!(results.artists[0].name, "Boards of Canada");
        let album = &results.albums[0];
        assert_eq!((album.year, album.kind.as_str()), (1995, "EP"));
        assert_eq!(album.group, ReleaseGroup::Single);
        assert_eq!(album.cover.as_deref(), Some("https://i.scdn.co/image/mid"));
        assert_eq!(results.songs.len(), 1);
        assert_eq!(
            results.songs[0].album.name,
            "Music Has the Right to Children"
        );
    }
}
