//! Does Spotify close the connection a few seconds after a fresh sign-in?
//!
//! Signs in through the browser into a throwaway config folder, watches the
//! session for a while, then reconnects with the credential that sign-in
//! saved and watches that session the same way, to compare the two.
//!
//!   cargo run -p spotless-core --example signin_probe -- <folder> [seconds]
//!
//! The folder should be new or empty: the app's own credential is not used.
//! Close spotifast first, which signs in on the same port.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use librespot_core::session::Session;
use spotless_core::browser;
use spotless_core::paths::Paths;
use spotless_core::spotify::session;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut args = std::env::args().skip(1);
    let folder = PathBuf::from(args.next().context("give a throwaway folder")?);
    let seconds = args.next().map_or(Ok(60), |s| s.parse())?;
    let paths = Paths::from_dirs(folder.join("config"), folder.join("cache"))?;

    println!("signing in: approve in the browser");
    let signed_in = session::sign_in(&paths, &browser::system()).await?;
    watch("fresh sign-in", &signed_in, seconds).await;
    signed_in.shutdown();

    let restored = session::restore(&paths)
        .await?
        .context("sign-in saved no credential")?;
    watch("saved credential", &restored, seconds).await;
    Ok(())
}

/// Prints when the session drops, if it does within `seconds`.
async fn watch(name: &str, session: &Session, seconds: u64) {
    println!("{name}: connected as {}", session.username());
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(seconds) {
        if session.is_invalid() {
            println!(
                "{name}: DROPPED after {:.1} s",
                began.elapsed().as_secs_f32()
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("{name}: still connected after {seconds} s");
}
