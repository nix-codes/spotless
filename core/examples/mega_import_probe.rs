//! Check: does a public MEGA folder's link read as the playlists Browse
//! shows, with no account? Prints the folder's name and each playlist's
//! songs as its M3U gives them, those not in the folder marked.
//!
//!   RUST_LOG=spotless_core=info cargo run -p spotless-core --features probes --example mega_import_probe -- <folder link>

use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use spotless_core::mega::api::Api;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let link = std::env::args().nth(1).context("give a MEGA folder link")?;
    let began = Instant::now();
    let folder = spotless_core::mega::read_folder(&Api::new(), &link)
        .await
        .map_err(|e| anyhow!(e))?;
    println!(
        "“{}” read in {} ms",
        folder.name,
        began.elapsed().as_millis()
    );
    for playlist in &folder.playlists {
        println!("\n{} ({})", playlist.name, playlist.uri);
        for (n, song) in playlist.songs.iter().enumerate() {
            let artists: Vec<_> = song.track.artists.iter().map(|a| a.name.as_str()).collect();
            let seconds = song.track.duration_ms / 1000;
            println!(
                "{:>3}. {} - {} [{}:{:02}]{}",
                n + 1,
                artists.join(", "),
                song.track.title,
                seconds / 60,
                seconds % 60,
                if song.found {
                    ""
                } else {
                    "  (not in the folder)"
                }
            );
        }
    }
    Ok(())
}
