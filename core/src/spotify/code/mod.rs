//! Spotify Codes: the Spotify logo and 23 bars a song, album, artist or
//! playlist is shared as, read from a picture of one.
//!
//! The bars are 23 heights of 0 to 7. The first, the middle and the last
//! are fixed (0, 7, 0); the other 20 carry 60 bits: a 37-bit "media
//! reference" with a CRC-8, convolutionally encoded, punctured, shuffled
//! and Gray-coded (worked out by Peter Boone and "Doyle" on Stack Overflow).
//! The reference is only a row in Spotify's own table: its private
//! `scannable-id` lookup turns it into a URI, asked once, never retried.
//!
//! `bars` turns a reference into its bars and back; `picture` finds a
//! code's bars in a picture; this asks Spotify what a reference is.

internal! {
    mod bars;
}
mod picture;

pub use bars::encode;

use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, GrayImage};
use picture::find;
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::{Album, Metadata, Playlist, Track};

use crate::model::ArtistRef;
use crate::model::catalog::SpotifyPlaylist;

/// A picture that may hold a Spotify Code: a dropped image file, what was
/// copied, or a camera's frame.
#[derive(Clone, PartialEq)]
pub enum CodeImage {
    /// An image file's bytes: PNG, JPEG or BMP.
    File(Vec<u8>),
    /// Pixels, four bytes each (red, green, blue, alpha), row by row.
    Pixels {
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// Greys, a byte each, row by row: a camera frame's brightness.
    Greys {
        width: u32,
        height: u32,
        greys: Vec<u8>,
    },
}

impl std::fmt::Debug for CodeImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(bytes) => write!(f, "CodeImage::File({} bytes)", bytes.len()),
            Self::Pixels { width, height, .. } => write!(f, "CodeImage::Pixels({width}x{height})"),
            Self::Greys { width, height, .. } => write!(f, "CodeImage::Greys({width}x{height})"),
        }
    }
}

/// What a code points to, as Browse opens it.
#[derive(Clone, Debug, PartialEq)]
pub enum CodeTarget {
    Artist(ArtistRef),
    /// An album, on its artist's page.
    Album { artist: ArtistRef, album: String },
    /// A song, on its album's artist's page.
    Track {
        artist: ArtistRef,
        album: String,
        track: String,
    },
    Playlist(SpotifyPlaylist),
}

const NOT_A_CODE: &str = "No Spotify Code found in the picture, or it isn't sharp enough.";

/// The media reference of the code in the picture.
pub fn read(image: &CodeImage) -> Result<u64> {
    let gray = match image {
        CodeImage::File(bytes) => image::load_from_memory(bytes).context("not a picture")?.to_luma8(),
        CodeImage::Pixels {
            width,
            height,
            rgba,
        } => DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(*width, *height, rgba.clone())
                .context("a picture of the wrong size")?,
        )
        .to_luma8(),
        CodeImage::Greys {
            width,
            height,
            greys,
        } => GrayImage::from_raw(*width, *height, greys.clone()).context("a picture of the wrong size")?,
    };
    find(&gray).ok_or_else(|| anyhow!(NOT_A_CODE))
}

/// The URI behind a media reference: one request to Spotify, not retried
/// whatever the answer (librespot's own requests retry, so it isn't used).
pub async fn look_up(session: &Session, http: &reqwest::Client, reference: u64) -> Result<String> {
    let base = session.spclient().base_url().await?;
    let token = session.login5().auth_token().await?;
    let client_token = session.spclient().client_token().await?;
    let response = http
        .get(format!("{base}/scannable-id/id/{reference}?format=json"))
        .header(
            "authorization",
            format!("{} {}", token.token_type, token.access_token),
        )
        .header("client-token", client_token)
        .header("accept", "application/json")
        // Not answered by then, it never will be: said, as it isn't retried.
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .context("Spotify couldn't be asked about the code")?;
    if !response.status().is_success() {
        bail!("Spotify didn't recognise this code.");
    }
    let answer: serde_json::Value = response.json().await?;
    let target = answer["target"]
        .as_str()
        .context("Spotify didn't recognise this code.")?;
    Ok(target.to_string())
}

/// Spotify's old playlist URIs name the owner too:
/// `spotify:user:<owner>:playlist:<id>` is `spotify:playlist:<id>` by
/// `<owner>`.
fn plain_uri(uri: &str) -> (String, Option<&str>) {
    match uri.split(':').collect::<Vec<_>>().as_slice() {
        ["spotify", "user", owner, "playlist", id] => {
            (format!("spotify:playlist:{id}"), Some(*owner))
        }
        _ => (uri.to_string(), None),
    }
}

/// What the URI the lookup gave, or a Spotify link, is, with what Browse
/// needs to open it.
pub async fn target(session: &Session, found: &str) -> Result<CodeTarget> {
    let (found, owner) = plain_uri(found.trim());
    let artist_ref = |artist: &librespot_metadata::artist::Artist| ArtistRef {
        uri: artist.id.to_uri(),
        name: artist.name.clone(),
        image: None,
    };
    let parsed = super::catalog::parse_uri(&found)?;
    // A link, as a URI from here on.
    let uri = parsed.to_uri();
    Ok(match parsed {
        SpotifyUri::Artist { .. } => {
            let artist = librespot_metadata::Artist::get(session, &parsed)
                .await
                .context("artist not found")?;
            CodeTarget::Artist(ArtistRef {
                uri: uri.to_string(),
                name: artist.name,
                image: None,
            })
        }
        SpotifyUri::Album { .. } => {
            let album = Album::get(session, &parsed)
                .await
                .context("album not found")?;
            let artist = album.artists.first().context("the album has no artist")?;
            CodeTarget::Album {
                artist: artist_ref(artist),
                album: uri.to_string(),
            }
        }
        SpotifyUri::Track { .. } => {
            let track = Track::get(session, &parsed)
                .await
                .context("song not found")?;
            let artist = (track.album.artists.first())
                .or(track.artists.first())
                .context("the song has no artist")?;
            CodeTarget::Track {
                artist: artist_ref(artist),
                album: track.album.id.to_uri(),
                track: uri.to_string(),
            }
        }
        SpotifyUri::Playlist { .. } => {
            let playlist = Playlist::get(session, &parsed)
                .await
                .context("playlist not found, or it's private")?;
            let owner = match owner.filter(|owner| *owner != session.username()) {
                Some(owner) => Some(super::catalog::display_name(session, owner).await),
                None => None,
            };
            CodeTarget::Playlist(SpotifyPlaylist {
                uri: uri.to_string(),
                name: playlist.name().to_string(),
                tracks: playlist.length.max(0) as usize,
                owner,
                private: false,
            })
        }
        _ => bail!("this is something Spotless can't open (a podcast?)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_playlist_uris_lose_their_owner() {
        assert_eq!(
            plain_uri("spotify:user:someone:playlist:7dCEN3pOke0Nv1Iz62VBnJ"),
            (
                "spotify:playlist:7dCEN3pOke0Nv1Iz62VBnJ".to_string(),
                Some("someone")
            )
        );
        assert_eq!(plain_uri("spotify:track:x"), ("spotify:track:x".into(), None));
    }

    #[test]
    fn a_picture_with_no_code_gives_nothing() {
        let blank = CodeImage::Pixels {
            width: 4,
            height: 4,
            rgba: vec![255; 64],
        };
        assert!(read(&blank).is_err());
    }
}
