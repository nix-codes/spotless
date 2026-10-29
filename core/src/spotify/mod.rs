//! Everything that talks to Spotify: sign-in, the session, catalogue reads,
//! the account's playlists, the account's other devices, and the Spotify
//! tab's queue.

pub mod catalog;
pub mod devices;
pub mod playlists;
pub mod queue;
pub mod remote;
pub mod session;
pub mod synced;
pub mod webapi;

/// The web link that opens this song, album, artist or playlist in
/// Spotify, to share it as the Spotify app does. None for what has none:
/// Liked Songs, a local file.
pub fn link(uri: &str) -> Option<String> {
    let (kind, id) = uri.strip_prefix("spotify:")?.split_once(':')?;
    let shareable = matches!(
        kind,
        "track" | "album" | "artist" | "playlist" | "episode" | "show"
    );
    (shareable && !id.is_empty() && !id.contains(':'))
        .then(|| format!("https://open.spotify.com/{kind}/{id}"))
}

/// The links of these, one a line, leaving out what has none.
pub fn links<'a>(uris: impl IntoIterator<Item = &'a str>) -> String {
    uris.into_iter()
        .filter_map(link)
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_albums_artists_and_playlists_have_links() {
        assert_eq!(
            link("spotify:track:4uLU6hMCjMI75M1A2tKUQC").as_deref(),
            Some("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC")
        );
        assert_eq!(
            link("spotify:playlist:37i9dQZF1DXcBWIGoYBM5M").as_deref(),
            Some("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M")
        );
    }

    #[test]
    fn liked_songs_and_local_files_have_none() {
        assert_eq!(link(catalog::LIKED_SONGS), None);
        assert_eq!(link("spotify:local:Artist:Album:Title:200"), None);
        assert_eq!(link("not a uri"), None);
    }

    #[test]
    fn several_links_go_one_a_line() {
        let uris = [
            "spotify:track:a",
            "spotify:local:x:y:z:1",
            "spotify:track:b",
        ];
        assert_eq!(
            links(uris),
            "https://open.spotify.com/track/a\nhttps://open.spotify.com/track/b"
        );
    }
}
