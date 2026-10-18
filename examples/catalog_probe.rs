//! M0 spike (docs/PLAN.md, section 6): what can the playback session alone
//! give the catalog view?
//!
//! 1. An artist's discography, grouped by release type, with years.
//! 2. Search: librespot has no search of its own, so this tries the Web API's
//!    `search` with the Login5 token the session mints (no second sign-in),
//!    and the session's own `searchview` endpoint. If either works, Spotless
//!    needs no Web API grant at all.
//! 3. Search with a personal Spotify app: the `web_client_id` in
//!    `~/.config/spotless/settings.json`. Opens the browser for approval on
//!    port 8989, so spotifast must be closed.
//!
//!   cargo run --example catalog_probe -- [artist URI or link] [search text]
//!   cargo run --example catalog_probe -- --personal [search text]
//!
//! Sign in through the app first: this reuses its saved credential.
//! Tokens are never printed.

use anyhow::{Context, Result, bail};
use librespot_core::session::Session;
use spotless::paths::Paths;
use spotless::spotify::{catalog, session};

/// Boards of Canada.
const DEFAULT_ARTIST: &str = "spotify:artist:2VAvhf61GgLYmC6C8anyX1";
const DEFAULT_QUERY: &str = "boards of canada";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = std::env::args().skip(1).peekable();
    let personal_only = args.next_if_eq("--personal").is_some();
    if personal_only {
        let query = args.next().unwrap_or_else(|| DEFAULT_QUERY.into());
        return personal_search(&Paths::new()?, &query).await;
    }
    let artist = args.next().unwrap_or_else(|| DEFAULT_ARTIST.into());
    let query = args.next().unwrap_or_else(|| DEFAULT_QUERY.into());

    let paths = Paths::new()?;
    let Some(session) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    println!("Signed in as {}\n", session.username());

    discography(&session, &artist).await?;
    println!();
    search(&session, &query).await;
    println!();
    if let Err(error) = personal_search(&paths, &query).await {
        println!("  FAILED: {error:#}");
    }
    Ok(())
}

/// Spotify's redirect for a personal app, as spotifast registers it.
const PERSONAL_REDIRECT_URI: &str = "http://127.0.0.1:8989/login";

async fn personal_search(paths: &Paths, query: &str) -> Result<()> {
    println!("== Search {query:?} through the Web API with your personal app");
    let settings: serde_json::Value = std::fs::read(paths.config.join("settings.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let Some(client_id) = settings["web_client_id"]
        .as_str()
        .filter(|id| !id.is_empty())
    else {
        bail!("no web_client_id in settings.json");
    };
    println!("  approve the app in your browser…");
    let token = librespot_oauth::OAuthClientBuilder::new(
        client_id,
        PERSONAL_REDIRECT_URI,
        vec!["user-read-private"],
    )
    .open_in_browser()
    .with_custom_message("Spotless probe signed in. You can close this tab.")
    .build()
    .map_err(|e| anyhow::anyhow!("{e}"))?
    .get_access_token_async()
    .await
    .map_err(|e| anyhow::anyhow!("sign-in failed: {e}"))?;
    web_api_search(&token.access_token, query).await;
    Ok(())
}

async fn discography(session: &Session, artist: &str) -> Result<()> {
    let uri = catalog::parse_uri(artist)?;
    let started = std::time::Instant::now();
    let discography = catalog::discography(session, &uri).await?;
    let releases = &discography.releases;
    println!(
        "== Discography of {}: {} releases in {:.1}s",
        discography.artist.name,
        releases.len(),
        started.elapsed().as_secs_f32()
    );
    for release in releases {
        println!(
            "  {:<12} {}  {:<45} {:>3} tracks  ({})",
            format!("{:?}", release.group),
            release.year,
            release.title,
            release.tracks,
            release.kind
        );
    }
    Ok(())
}

async fn search(session: &Session, query: &str) {
    println!("== Search {query:?} through the Web API with a Login5 token");
    // The keymaster token is gone: Spotify answers 403 for it.
    match session.login5().auth_token().await {
        Ok(token) => web_api_search(&token.access_token, query).await,
        Err(error) => println!("  could not get a token: {error:#}"),
    }

    println!("\n== Search {query:?} through the session's searchview endpoint");
    let endpoint = format!(
        "/searchview/km/v4/search/{}?limit=3&entityVersion=2",
        query.replace(' ', "%20")
    );
    match session
        .spclient()
        .request_as_json(&reqwest::Method::GET, &endpoint, None, None)
        .await
    {
        Ok(body) => {
            let text = String::from_utf8_lossy(&body);
            println!("  OK, {} bytes. Start of the answer:", body.len());
            println!("    {}", text.chars().take(600).collect::<String>());
        }
        Err(error) => println!("  FAILED: {error}"),
    }
}

/// Up to three attempts, waiting out a short Retry-After between them, to
/// tell a passing throttle from a quota that is always exhausted.
async fn web_api_search(token: &str, query: &str) {
    for attempt in 1..=3 {
        match search_with(token, query).await {
            Ok(Ok(lines)) => {
                println!("  attempt {attempt}: OK");
                for line in lines {
                    println!("    {line}");
                }
                return;
            }
            Ok(Err((message, Some(wait)))) if wait <= 120 && attempt < 3 => {
                println!("  attempt {attempt}: {message}; waiting {wait}s");
                tokio::time::sleep(std::time::Duration::from_secs(wait + 1)).await;
            }
            Ok(Err((message, _))) => {
                println!("  attempt {attempt}: FAILED: {message}");
                return;
            }
            Err(error) => {
                println!("  attempt {attempt}: FAILED: {error:#}");
                return;
            }
        }
    }
}

/// The result lines, or Spotify's refusal with its Retry-After in seconds.
type Answer = std::result::Result<Vec<String>, (String, Option<u64>)>;

async fn search_with(token: &str, query: &str) -> Result<Answer> {
    let response = reqwest::Client::new()
        .get("https://api.spotify.com/v1/search")
        .query(&[("q", query), ("type", "artist,album,track"), ("limit", "3")])
        .bearer_auth(token)
        .send()
        .await
        .context("request failed")?;
    let status = response.status();
    let retry = response
        .headers()
        .get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = body["error"]["message"].as_str().unwrap_or("no message");
        return Ok(Err((format!("HTTP {status}: {message}"), retry)));
    }
    let mut lines = Vec::new();
    for (kind, key) in [
        ("artist", "artists"),
        ("album", "albums"),
        ("track", "tracks"),
    ] {
        for item in body[key]["items"].as_array().into_iter().flatten() {
            lines.push(format!(
                "{kind:<6} {}",
                item["name"].as_str().unwrap_or("?")
            ));
        }
    }
    Ok(Ok(lines))
}
