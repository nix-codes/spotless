//! Spotify Codes: the Spotify logo and 23 bars a song, album, artist or
//! playlist is shared as, read from a picture of one.
//!
//! The bars are 23 heights of 0 to 7. The first, the middle and the last
//! are fixed (0, 7, 0); the other 20 carry 60 bits: a 37-bit "media
//! reference" with a CRC-8, convolutionally encoded, punctured, shuffled
//! and Gray-coded (worked out by Peter Boone and "Doyle" on Stack Overflow).
//! The reference is only a row in Spotify's own table: its private
//! `scannable-id` lookup turns it into a URI, asked once, never retried.

use anyhow::{Context, Result, anyhow, bail};
use image::{DynamicImage, GrayImage};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::{Album, Metadata, Playlist, Track};

use crate::model::ArtistRef;
use crate::model::catalog::SpotifyPlaylist;

/// A picture that may hold a Spotify Code: a dropped image file, or what
/// was copied.
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
}

impl std::fmt::Debug for CodeImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File(bytes) => write!(f, "CodeImage::File({} bytes)", bytes.len()),
            Self::Pixels { width, height, .. } => write!(f, "CodeImage::Pixels({width}x{height})"),
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
    let image = match image {
        CodeImage::File(bytes) => image::load_from_memory(bytes).context("not a picture")?,
        CodeImage::Pixels {
            width,
            height,
            rgba,
        } => DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(*width, *height, rgba.clone())
                .context("a picture of the wrong size")?,
        ),
    };
    find(&image.to_luma8()).ok_or_else(|| anyhow!(NOT_A_CODE))
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

/// Bar height for each 3 bits: neighbouring heights differ in one bit, so
/// a bar misread by one step costs one bit.
const GRAY: [u8; 8] = [0, 1, 3, 2, 7, 6, 4, 5];
const GEN1: [u8; 7] = [1, 0, 1, 1, 0, 1, 1];
const GEN2: [u8; 7] = [1, 1, 1, 1, 0, 0, 1];

/// The 23 bar heights of a media reference.
pub fn encode(reference: u64) -> [u8; 23] {
    let bits = stream(reference);
    let mut heights = [0u8; 23];
    heights[11] = 7;
    for i in 0..20 {
        let n = (bits >> (3 * i) & 1) << 2 | (bits >> (3 * i + 1) & 1) << 1 | bits >> (3 * i + 2) & 1;
        heights[bar(i)] = GRAY[n as usize];
    }
    heights
}

/// Where the i-th of the 20 bars that carry bits is among the 23.
fn bar(i: usize) -> usize {
    if i < 10 { i + 1 } else { i + 2 }
}

/// The media reference the bars carry, allowing one bar misread by one
/// step when that is the only fix that makes them a code. None if nothing
/// fits.
pub fn decode(heights: &[u8; 23]) -> Option<u64> {
    if let Some(reference) = solve(heights) {
        return Some(reference);
    }
    let mut fits = Vec::new();
    for at in (0..20).map(bar) {
        for to in [heights[at].checked_sub(1), Some(heights[at] + 1)] {
            let Some(to) = to.filter(|to| *to <= 7) else {
                continue;
            };
            let mut tried = *heights;
            tried[at] = to;
            fits.extend(solve(&tried));
        }
    }
    (fits.len() == 1).then(|| fits[0])
}

/// The reference whose code is exactly these heights. The encoding is
/// affine over GF(2) (linear, plus the CRC's final inversion), so the 60
/// bits give 60 equations in the reference's 37 bits.
fn solve(heights: &[u8; 23]) -> Option<u64> {
    if heights[0] != 0 || heights[11] != 7 || heights[22] != 0 {
        return None;
    }
    let mut bits = 0u64;
    for i in 0..20 {
        let n = GRAY.iter().position(|g| *g == heights[bar(i)])? as u64;
        bits |= (n >> 2 & 1) << (3 * i) | (n >> 1 & 1) << (3 * i + 1) | (n & 1) << (3 * i + 2);
    }
    let zero = stream(0);
    let columns: Vec<u64> = (0..37).map(|b| stream(1 << b) ^ zero).collect();
    let target = bits ^ zero;
    // Rows: which bits of the reference, and the bit they must give.
    let mut rows: Vec<(u64, u64)> = (0..60)
        .map(|j| {
            let mask = (0..37).fold(0, |m, b| m | (columns[b] >> j & 1) << b);
            (mask, target >> j & 1)
        })
        .collect();
    for b in 0..37 {
        let pivot = (b..rows.len()).find(|r| rows[*r].0 >> b & 1 == 1)?;
        rows.swap(b, pivot);
        let (mask, value) = rows[b];
        for (r, row) in rows.iter_mut().enumerate() {
            if r != b && row.0 >> b & 1 == 1 {
                *row = (row.0 ^ mask, row.1 ^ value);
            }
        }
    }
    // The other 23 equations hold too, or these aren't a code.
    if rows[37..].iter().any(|(_, value)| *value != 0) {
        return None;
    }
    let reference = (0..37).fold(0, |r, b| r | rows[b].1 << b);
    (encode(reference) == *heights).then_some(reference)
}

/// The 60 bits the bars carry, in bar order (bit 3i is the i-th bar's top
/// bit).
fn stream(reference: u64) -> u64 {
    let mut data = [0u8; 45];
    for (i, bit) in data.iter_mut().take(37).enumerate() {
        *bit = (reference >> i & 1) as u8;
    }
    let check = crc(&data[..37]);
    data[37..].copy_from_slice(&check);
    // Tail-biting: the register starts with the last 6 bits.
    let mut padded = [0u8; 51];
    padded[..6].copy_from_slice(&data[39..]);
    padded[6..].copy_from_slice(&data);
    let mut encoded = Vec::with_capacity(60);
    for k in 0..45 {
        let parity = |g: &[u8; 7]| (0..7).fold(0, |p, j| p ^ padded[k + j] & g[6 - j]);
        // Punctured: of every three bits, the third is dropped.
        for (i, bit) in [parity(&GEN1), parity(&GEN2)].into_iter().enumerate() {
            if (2 * k + i) % 3 != 2 {
                encoded.push(bit);
            }
        }
    }
    (0..60).fold(0, |s, i| s | (encoded[7 * i % 60] as u64) << i)
}

/// CRC-8, polynomial 0x07, bytes reflected in and out, inverted.
fn crc(bits: &[u8]) -> [u8; 8] {
    let mut bytes = [0u8; 5];
    for (i, bit) in bits.iter().enumerate() {
        bytes[i / 8] |= bit << (7 - i % 8);
    }
    let mut register = 0u8;
    for byte in bytes {
        register ^= byte.reverse_bits();
        for _ in 0..8 {
            register = if register & 0x80 != 0 {
                register << 1 ^ 0x07
            } else {
                register << 1
            };
        }
    }
    let check = register.reverse_bits() ^ 0xff;
    std::array::from_fn(|i| check >> (7 - i) & 1)
}

/// A shape in the picture: its bounding box.
#[derive(Clone, Copy, Debug)]
struct Shape {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

impl Shape {
    fn width(&self) -> f32 {
        (self.right - self.left + 1) as f32
    }
    fn height(&self) -> f32 {
        (self.bottom - self.top + 1) as f32
    }
    fn middle(&self) -> f32 {
        (self.top + self.bottom) as f32 / 2.0
    }
}

/// The reference of a code anywhere in the picture: a round logo with 23
/// bars after it, light on dark or dark on light.
fn find(gray: &GrayImage) -> Option<u64> {
    let threshold = otsu(gray);
    [true, false].into_iter().find_map(|light| {
        let shapes = shapes(gray, |luma| (luma > threshold) == light);
        shapes.iter().find_map(|logo| code_after(logo, &shapes))
    })
}

/// The code whose logo `logo` would be, if the 23 shapes after it are its
/// bars.
fn code_after(logo: &Shape, shapes: &[Shape]) -> Option<u64> {
    let size = logo.width().max(logo.height());
    let round = (logo.width() - logo.height()).abs() <= size * 0.15;
    if !round || size < 8.0 {
        return None;
    }
    let mut bars: Vec<&Shape> = (shapes.iter())
        .filter(|s| s.left > logo.right && (s.left - logo.right) as f32 <= size * 14.0)
        .filter(|s| (s.middle() - logo.middle()).abs() <= size * 0.15)
        .filter(|s| s.height() <= size * 1.2 && s.width() <= size * 0.35)
        .collect();
    bars.sort_by_key(|s| s.left);
    let bars = bars.get(..23)?;
    // The first bar is near the logo, the bars evenly spaced.
    let step = (bars[22].left - bars[0].left) as f32 / 22.0;
    if (bars[0].left - logo.right) as f32 > step * 2.5 {
        return None;
    }
    let low = (bars[0].height() + bars[22].height()) / 2.0;
    let high = bars[11].height();
    if high <= low * 2.0 {
        return None;
    }
    let heights: [u8; 23] = std::array::from_fn(|i| {
        ((bars[i].height() - low) / (high - low) * 7.0)
            .round()
            .clamp(0.0, 7.0) as u8
    });
    decode(&heights)
}

/// The shapes of the pixels `ink` picks, joined where they touch (up,
/// down, left, right).
fn shapes(gray: &GrayImage, ink: impl Fn(u8) -> bool) -> Vec<Shape> {
    let (width, height) = gray.dimensions();
    let mut seen = vec![false; (width * height) as usize];
    let mut found = Vec::new();
    let mut stack = Vec::new();
    for start in 0..(width * height) {
        if seen[start as usize] || !ink(gray.as_raw()[start as usize]) {
            continue;
        }
        seen[start as usize] = true;
        stack.push(start);
        let mut shape = Shape {
            left: u32::MAX,
            top: u32::MAX,
            right: 0,
            bottom: 0,
        };
        while let Some(at) = stack.pop() {
            let (x, y) = (at % width, at / width);
            shape.left = shape.left.min(x);
            shape.right = shape.right.max(x);
            shape.top = shape.top.min(y);
            shape.bottom = shape.bottom.max(y);
            let next = [
                (x > 0).then(|| at - 1),
                (x + 1 < width).then(|| at + 1),
                (y > 0).then(|| at - width),
                (y + 1 < height).then(|| at + width),
            ];
            for n in next.into_iter().flatten() {
                if !seen[n as usize] && ink(gray.as_raw()[n as usize]) {
                    seen[n as usize] = true;
                    stack.push(n);
                }
            }
        }
        found.push(shape);
    }
    found
}

/// The grey that best splits the picture in two (Otsu's method).
fn otsu(gray: &GrayImage) -> u8 {
    let mut histogram = [0u64; 256];
    for luma in gray.as_raw() {
        histogram[*luma as usize] += 1;
    }
    let total = gray.as_raw().len() as f64;
    let sum: f64 = (0..256).map(|i| i as f64 * histogram[i] as f64).sum();
    let (mut below, mut below_sum, mut best, mut threshold) = (0.0, 0.0, 0.0, 127);
    for (i, count) in histogram.iter().enumerate() {
        below += *count as f64;
        if below == 0.0 || below == total {
            continue;
        }
        below_sum += i as f64 * *count as f64;
        let mean_below = below_sum / below;
        let mean_above = (sum - below_sum) / (total - below);
        let between = below * (total - below) * (mean_below - mean_above).powi(2);
        if between > best {
            best = between;
            threshold = i as u8;
        }
    }
    threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boone's example: "Today's Top Hits".
    const HEIGHTS: [u8; 23] = [
        0, 6, 0, 2, 4, 5, 1, 4, 5, 2, 3, 7, 3, 7, 1, 5, 6, 2, 5, 7, 4, 3, 0,
    ];
    const REFERENCE: u64 = 57268659651;

    #[test]
    fn the_published_example_encodes_and_decodes() {
        assert_eq!(encode(REFERENCE), HEIGHTS);
        assert_eq!(decode(&HEIGHTS), Some(REFERENCE));
    }

    #[test]
    fn one_bar_misread_by_a_step_is_put_right() {
        let mut misread = HEIGHTS;
        misread[18] = 4;
        assert_eq!(decode(&misread), Some(REFERENCE));
    }

    #[test]
    fn bars_that_arent_a_code_give_nothing() {
        assert_eq!(decode(&[0; 23]), None);
        let mut scrambled = HEIGHTS;
        scrambled.swap(1, 2);
        scrambled.swap(3, 4);
        assert_eq!(decode(&scrambled), None);
    }

    #[test]
    fn many_references_go_round() {
        for reference in [0, 1, 12345, 16862907074, 75845227563, (1 << 37) - 1] {
            assert_eq!(decode(&encode(reference)), Some(reference));
        }
    }

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

    /// A code drawn as Spotify draws one: a round logo, then 23 rounded
    /// bars, `ink` on `paper`, in a picture `margin` pixels larger.
    fn drawn(reference: u64, ink: u8, paper: u8, margin: u32) -> GrayImage {
        let (unit, gap, logo) = (8u32, 6u32, 50u32);
        let width = 2 * margin + logo + 20 + 23 * (unit + gap);
        let height = 2 * margin + logo;
        let mut picture = GrayImage::from_pixel(width, height, image::Luma([paper]));
        let middle = (margin + logo / 2) as f32;
        let r = logo as f32 / 2.0;
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as f32 - (margin as f32 + r), y as f32 - middle);
                // The logo, with a wave cut through it.
                let in_logo = dx * dx + dy * dy <= r * r && !(dy.abs() < 2.0 && dx.abs() < r * 0.6);
                if in_logo {
                    picture.put_pixel(x, y, image::Luma([ink]));
                }
            }
        }
        for (i, level) in encode(reference).into_iter().enumerate() {
            let half = (unit as f32 + level as f32 * 5.0) / 2.0;
            let left = margin + logo + 20 + i as u32 * (unit + gap);
            for y in 0..height {
                if (y as f32 - middle).abs() > half {
                    continue;
                }
                for x in left..left + unit {
                    picture.put_pixel(x, y, image::Luma([ink]));
                }
            }
        }
        picture
    }

    #[test]
    fn a_code_is_found_dark_on_light_and_light_on_dark() {
        assert_eq!(find(&drawn(REFERENCE, 0, 255, 10)), Some(REFERENCE));
        assert_eq!(find(&drawn(16862907074, 255, 0, 10)), Some(16862907074));
    }

    #[test]
    fn a_code_is_found_among_other_things() {
        let mut picture = drawn(REFERENCE, 20, 230, 60);
        // Something else above it, as in a screenshot.
        for x in 5..picture.width() - 5 {
            for y in 5..20 {
                picture.put_pixel(x, y, image::Luma([20]));
            }
        }
        assert_eq!(find(&picture), Some(REFERENCE));
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
