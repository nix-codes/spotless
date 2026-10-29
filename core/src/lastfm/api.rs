//! Last.fm's API: signed calls, and the browser sign-in.
//!
//! Every call names the user's own API account (its key, signed with its
//! shared secret). The sign-in opens Last.fm's approval page with a loopback
//! callback; Last.fm sends the browser back there with a token, which is
//! traded for a session key that doesn't expire.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::listen::{Scrobble, Song};
use super::loved::LovedSong;
use crate::browser::{self, Browser};

const API_URL: &str = "https://ws.audioscrobbler.com/2.0/";
const AUTH_URL: &str = "https://www.last.fm/api/auth";
/// Last.fm takes up to this many scrobbles in one call.
pub const BATCH: usize = 50;
/// Similar artists asked for at once.
const SIMILAR_LIMIT: &str = "20";
/// How long the browser approval may take.
const APPROVAL_WAIT: Duration = Duration::from_secs(300);

/// The user's API account on Last.fm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keys {
    pub api_key: String,
    pub secret: String,
}

/// What a failed call means for what comes next.
#[derive(Debug)]
pub enum Failure {
    /// The approval was withdrawn or the keys changed: sign in again.
    SignedOut(String),
    /// Last.fm or the network is down for now: try again later.
    Later(String),
    /// Last.fm won't take this request; trying again won't help.
    Refused(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::SignedOut(m) | Failure::Later(m) | Failure::Refused(m) => f.write_str(m),
        }
    }
}

/// Opens Last.fm's approval page and waits for the user to allow
/// Spotless. Returns the session key and the Last.fm user name.
pub async fn sign_in(
    http: &reqwest::Client,
    keys: &Keys,
    browser: &Browser,
) -> Result<(String, String)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("unable to listen for Last.fm's answer")?;
    let port = listener.local_addr()?.port();
    let callback = format!("http://127.0.0.1:{port}/lastfm");
    let url = reqwest::Url::parse_with_params(
        AUTH_URL,
        [
            ("api_key", keys.api_key.as_str()),
            ("cb", callback.as_str()),
        ],
    )?;
    browser(url.as_str());
    let token = tokio::time::timeout(APPROVAL_WAIT, wait_for_token(&listener))
        .await
        .map_err(|_| anyhow!("Last.fm wasn't approved in time"))??;

    let body = call(http, keys, "auth.getSession", &[("token", token)], None)
        .await
        .map_err(|failure| anyhow!("Last.fm sign-in failed: {failure}"))?;
    let session = &body["session"];
    let key = session["key"].as_str().context("Last.fm sent no session")?;
    let name = session["name"].as_str().unwrap_or_default();
    Ok((key.to_owned(), name.to_owned()))
}

/// Answers the browser's visits until one brings the token.
async fn wait_for_token(listener: &TcpListener) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut buffer = vec![0; 4096];
        let read = stream.read(&mut buffer).await.unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target = request.split_whitespace().nth(1).unwrap_or_default();
        let token = reqwest::Url::parse(&format!("http://localhost{target}"))
            .ok()
            .filter(|url| url.path() == "/lastfm")
            .and_then(|url| {
                url.query_pairs()
                    .find(|(name, _)| name == "token")
                    .map(|(_, value)| value.into_owned())
            });
        let (status, page) = match &token {
            Some(_) => (
                "200 OK",
                browser::done_page("Spotless is connected to Last.fm."),
            ),
            None => ("404 Not Found", "Not found.".to_string()),
        };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        if let Some(token) = token {
            return Ok(token);
        }
    }
}

/// Tells Last.fm what is playing now.
pub async fn now_playing(
    http: &reqwest::Client,
    keys: &Keys,
    session: &str,
    song: &Song,
) -> Result<(), Failure> {
    let mut params = vec![
        ("artist", song.artist.clone()),
        ("track", song.title.clone()),
        ("duration", song.duration_s.to_string()),
    ];
    if !song.album.is_empty() {
        params.push(("album", song.album.clone()));
    }
    call(http, keys, "track.updateNowPlaying", &params, Some(session))
        .await
        .map(drop)
}

/// Sends up to [`BATCH`] listens.
pub async fn scrobble(
    http: &reqwest::Client,
    keys: &Keys,
    session: &str,
    listens: &[Scrobble],
) -> Result<(), Failure> {
    let mut params = Vec::new();
    for (i, listen) in listens.iter().take(BATCH).enumerate() {
        let song = &listen.song;
        params.push((format!("artist[{i}]"), song.artist.clone()));
        params.push((format!("track[{i}]"), song.title.clone()));
        params.push((format!("timestamp[{i}]"), listen.timestamp.to_string()));
        params.push((format!("duration[{i}]"), song.duration_s.to_string()));
        if !song.album.is_empty() {
            params.push((format!("album[{i}]"), song.album.clone()));
        }
    }
    let params: Vec<(&str, String)> = params
        .iter()
        .map(|(k, v)| (k.as_str(), v.clone()))
        .collect();
    call(http, keys, "track.scrobble", &params, Some(session))
        .await
        .map(drop)
}

/// Artists Last.fm's listeners play alongside `artist`, closest first, by
/// name. Needs only the API key: nothing is signed.
pub async fn similar_artists(
    http: &reqwest::Client,
    api_key: &str,
    artist: &str,
) -> Result<Vec<String>, Failure> {
    let answered = answer(http.get(API_URL).query(&[
        ("method", "artist.getSimilar"),
        ("artist", artist),
        ("autocorrect", "1"),
        ("limit", SIMILAR_LIMIT),
        ("api_key", api_key),
        ("format", "json"),
    ]))
    .await;
    match answered {
        Ok(body) => Ok(similar_names(&body, artist)),
        // Error 6: Last.fm has no such artist, so none like it.
        Err(Failure::Refused(message)) if message.starts_with("Last.fm error 6:") => Ok(Vec::new()),
        Err(failure) => Err(failure),
    }
}

/// The names in an `artist.getSimilar` answer, once each, without the
/// artist asked about.
fn similar_names(body: &Value, artist: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::from([artist.to_lowercase()]);
    body["similarartists"]["artist"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
        .map(str::trim)
        .filter(|name| !name.is_empty() && seen.insert(name.to_lowercase()))
        .map(String::from)
        .collect()
}

/// Loves a song on Last.fm, or doesn't any more.
pub async fn love(
    http: &reqwest::Client,
    keys: &Keys,
    session: &str,
    artist: &str,
    title: &str,
    loved: bool,
) -> Result<(), Failure> {
    let method = if loved { "track.love" } else { "track.unlove" };
    let params = [("artist", artist.to_owned()), ("track", title.to_owned())];
    call(http, keys, method, &params, Some(session))
        .await
        .map(drop)
}

/// A page of the songs `user` loved, newest first, `limit` to a page; and
/// how many pages there are.
pub async fn loved_page(
    http: &reqwest::Client,
    api_key: &str,
    user: &str,
    page: u32,
    limit: u32,
) -> Result<(Vec<LovedSong>, u32), Failure> {
    let body = answer(http.get(API_URL).query(&[
        ("method", "user.getLovedTracks"),
        ("user", user),
        ("page", &page.to_string()),
        ("limit", &limit.to_string()),
        ("api_key", api_key),
        ("format", "json"),
    ]))
    .await?;
    Ok(loved_songs(&body))
}

/// The songs in a `user.getLovedTracks` answer, and its number of pages.
fn loved_songs(body: &Value) -> (Vec<LovedSong>, u32) {
    let loved = &body["lovedtracks"];
    // One song comes alone rather than in a list.
    let tracks = match &loved["track"] {
        Value::Array(tracks) => tracks.iter().collect(),
        track @ Value::Object(_) => vec![track],
        _ => Vec::new(),
    };
    let songs = tracks
        .into_iter()
        .filter_map(|track| {
            let date = track["date"]["uts"].as_str();
            Some(LovedSong {
                artist: track["artist"]["name"].as_str()?.to_owned(),
                title: track["name"].as_str()?.to_owned(),
                date: date.and_then(|uts| uts.parse().ok()).unwrap_or(0),
            })
        })
        .collect();
    let pages = loved["@attr"]["totalPages"]
        .as_str()
        .and_then(|pages| pages.parse().ok())
        .unwrap_or(1);
    (songs, pages)
}

/// A signed POST to the API.
async fn call(
    http: &reqwest::Client,
    keys: &Keys,
    method: &str,
    params: &[(&str, String)],
    session: Option<&str>,
) -> Result<Value, Failure> {
    let mut form: BTreeMap<&str, String> = params.iter().cloned().collect();
    form.insert("method", method.into());
    form.insert("api_key", keys.api_key.clone());
    if let Some(session) = session {
        form.insert("sk", session.into());
    }
    form.insert("api_sig", signature(&form, &keys.secret));
    form.insert("format", "json".into());

    answer(http.post(API_URL).form(&form)).await
}

/// Sends a request and reads Last.fm's answer, or its error.
async fn answer(request: reqwest::RequestBuilder) -> Result<Value, Failure> {
    let response = request
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| Failure::Later(format!("Last.fm can't be reached: {e}")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_default();
    if let Some(code) = body["error"].as_u64() {
        let message = body["message"].as_str().unwrap_or("no message").to_owned();
        return Err(failure(code, message));
    }
    if !status.is_success() {
        return Err(Failure::Later(format!("Last.fm answered HTTP {status}")));
    }
    Ok(body)
}

/// Last.fm's error codes, sorted by what to do about them.
fn failure(code: u64, message: String) -> Failure {
    match code {
        // Invalid session key, invalid API key, unauthorised token.
        4 | 9 | 10 | 14 | 15 | 26 => Failure::SignedOut(match code {
            10 | 26 => "Last.fm doesn't know that API key.".into(),
            _ => format!("Last.fm needs approving again: {message}"),
        }),
        13 => Failure::SignedOut("Last.fm refused the shared secret.".into()),
        // Operation failed, service offline, temporarily unavailable,
        // rate limit exceeded.
        8 | 11 | 16 | 29 => Failure::Later(format!("Last.fm is busy: {message}")),
        _ => Failure::Refused(format!("Last.fm error {code}: {message}")),
    }
}

/// The parameters by name, each name followed by its value, then the
/// secret, through MD5. `format` and `callback` are left out, as Last.fm
/// asks.
fn signature(form: &BTreeMap<&str, String>, secret: &str) -> String {
    let mut hasher = Md5::new();
    for (name, value) in form {
        if *name != "format" && *name != "callback" {
            hasher.update(name.as_bytes());
            hasher.update(value.as_bytes());
        }
    }
    hasher.update(secret.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Last.fm's keys and secrets: 32 hexadecimal digits.
pub fn is_key(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Checks the keys look right before a sign-in is tried.
pub fn check_keys(keys: &Keys) -> Result<()> {
    if !is_key(&keys.api_key) {
        bail!("That isn't an API key: it is 32 letters and digits.");
    }
    if !is_key(&keys.secret) {
        bail!("That isn't a shared secret: it is 32 letters and digits.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_signature_sorts_by_name_and_skips_format() {
        let mut form = BTreeMap::new();
        form.insert("token", "t".to_string());
        form.insert("method", "auth.getSession".to_string());
        form.insert("api_key", "k".to_string());
        form.insert("format", "json".to_string());
        // md5("api_keykmethodauth.getSessiontokentsecret")
        let expected: String = Md5::digest(b"api_keykmethodauth.getSessiontokentsecret")
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(signature(&form, "secret"), expected);
        assert_eq!(expected.len(), 32);
    }

    #[test]
    fn error_codes_say_what_to_do() {
        assert!(matches!(failure(9, String::new()), Failure::SignedOut(_)));
        assert!(matches!(failure(13, String::new()), Failure::SignedOut(_)));
        assert!(matches!(failure(29, String::new()), Failure::Later(_)));
        assert!(matches!(failure(6, String::new()), Failure::Refused(_)));
    }

    #[test]
    fn similar_artists_come_once_each_without_the_artist() {
        let body = serde_json::json!({"similarartists": {"artist": [
            {"name": "Tycho", "match": "1"},
            {"name": "boards of canada", "match": "0.9"},
            {"name": "Bibio", "match": "0.8"},
            {"name": "tycho", "match": "0.7"},
            {"name": " ", "match": "0.6"},
            {"match": "0.5"}
        ]}});
        assert_eq!(similar_names(&body, "Boards of Canada"), ["Tycho", "Bibio"]);
        assert!(similar_names(&serde_json::json!({}), "x").is_empty());
    }

    #[test]
    fn loved_songs_come_as_artist_and_title_with_the_pages() {
        let body = serde_json::json!({"lovedtracks": {
            "track": [
                {"name": "Roygbiv", "artist": {"name": "Boards of Canada"},
                 "date": {"uts": "1761700000", "#text": "29 Oct 2025, 01:06"}},
                {"name": "Creep - Live", "artist": {"name": "Radiohead"}},
                {"name": "No artist"}
            ],
            "@attr": {"page": "1", "totalPages": "3", "total": "2050"}
        }});
        let (songs, pages) = loved_songs(&body);
        let found: Vec<_> = songs
            .iter()
            .map(|s| (s.artist.as_str(), s.title.as_str(), s.date))
            .collect();
        assert_eq!(
            found,
            [
                ("Boards of Canada", "Roygbiv", 1761700000),
                ("Radiohead", "Creep - Live", 0)
            ]
        );
        assert_eq!(pages, 3);

        let one = serde_json::json!({"lovedtracks": {
            "track": {"name": "Roygbiv", "artist": {"name": "Boards of Canada"}},
            "@attr": {"totalPages": "1"}
        }});
        assert_eq!(loved_songs(&one).0.len(), 1);
        assert_eq!(loved_songs(&serde_json::json!({})), (Vec::new(), 1));
    }

    #[test]
    fn keys_are_32_hex_digits() {
        assert!(is_key("0123456789abcdef0123456789abcdef"));
        assert!(!is_key("0123456789abcdef"));
        assert!(
            check_keys(&Keys {
                api_key: "0123456789abcdef0123456789abcdef".into(),
                secret: "nope".into(),
            })
            .is_err()
        );
    }
}
