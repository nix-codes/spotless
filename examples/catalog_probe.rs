//! M0 spike (docs/PLAN.md, section 6): what can the playback session alone
//! give the catalog view?
//!
//! 1. An artist's discography, grouped by release type, with years.
//! 2. Search: librespot has no search of its own, so this tries the Web API's
//!    `search` with the two tokens the session can mint without a second
//!    sign-in: a Login5 token and a keymaster token. If either works, Spotless
//!    needs no Web API grant at all.
//!
//!   cargo run --example catalog_probe -- [artist URI or link] [search text]
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
    let mut args = std::env::args().skip(1);
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
    Ok(())
}

async fn discography(session: &Session, artist: &str) -> Result<()> {
    let uri = catalog::parse_uri(artist)?;
    let started = std::time::Instant::now();
    let (name, releases) = catalog::discography(session, &uri).await?;
    println!(
        "== Discography of {name}: {} releases in {:.1}s",
        releases.len(),
        started.elapsed().as_secs_f32()
    );
    for release in &releases {
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
    println!("== Search {query:?} through the Web API");
    let login5 = session.login5().auth_token().await.map(|t| t.access_token);
    try_search("Login5 token", login5.map_err(Into::into), query).await;
    let keymaster = session
        .token_provider()
        .get_token("user-read-private")
        .await
        .map(|t| t.access_token);
    try_search("keymaster token", keymaster.map_err(Into::into), query).await;
}

async fn try_search(label: &str, token: Result<String>, query: &str) {
    let token = match token {
        Ok(token) => token,
        Err(error) => {
            println!("  {label}: could not get a token: {error:#}");
            return;
        }
    };
    match search_with(&token, query).await {
        Ok(lines) => {
            println!("  {label}: OK");
            for line in lines {
                println!("    {line}");
            }
        }
        Err(error) => println!("  {label}: FAILED: {error:#}"),
    }
}

async fn search_with(token: &str, query: &str) -> Result<Vec<String>> {
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
        .map(str::to_owned);
    let body: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = body["error"]["message"].as_str().unwrap_or("no message");
        match retry {
            Some(retry) => bail!("HTTP {status}: {message} (retry after {retry}s)"),
            None => bail!("HTTP {status}: {message}"),
        }
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
    Ok(lines)
}
