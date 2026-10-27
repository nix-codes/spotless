//! A made-up library and artist, for screenshots: none of these bands,
//! songs or covers exist, so nobody's account or library is ever in them.
//! [`crate::controller::Controller::demo`] starts from them.

use crate::images::CoverImage;
use crate::model::catalog::{Discography, Release, ReleaseGroup};
use crate::model::{AlbumRef, ArtistRef, EntryId, Library, PlaylistId, TrackRef};

/// Who is signed in.
pub const USER: &str = "listener";
/// The artist Browse shows.
pub const ARTIST: &str = "Paper Satellites";
pub const ARTIST_URI: &str = "spotify:artist:demo";
/// Where the playing song is, in ms.
pub const POSITION_MS: u32 = 68_000;
/// The playing song, in Night Drive.
const NOW_PLAYING: usize = 9;
/// The artist's picture, among the covers.
const PICTURE: usize = 99;
const COVER_PREFIX: &str = "demo:cover/";

/// The library, and where things are in it.
pub struct Demo {
    pub library: Library,
    /// Night Drive, which plays.
    pub playing: PlaylistId,
    /// Its song that plays.
    pub now: EntryId,
}

impl Demo {
    pub fn new() -> Self {
        let mut library = Library::new();
        fill(&mut library, PlaylistId::DEFAULT, DEFAULT_SONGS);
        let playing = library.create("Night Drive");
        fill(&mut library, playing, NIGHT_DRIVE);
        let artist_list = library.create(ARTIST);
        let artist_songs: Vec<Song> = DISCOGRAPHY
            .iter()
            .flat_map(|(album, _, _, _, songs)| {
                songs
                    .iter()
                    .map(move |(title, secs)| (ARTIST, *title, *album, *secs))
            })
            .collect();
        fill(&mut library, artist_list, &artist_songs);
        library.create("Sunday Morning");
        let list = library.get_mut(playing).expect("just made");
        let now = list.entries()[NOW_PLAYING].id;
        list.set_cursor(now);
        Self {
            library,
            playing,
            now,
        }
    }
}

impl Default for Demo {
    fn default() -> Self {
        Self::new()
    }
}

pub fn artist() -> ArtistRef {
    ArtistRef {
        uri: ARTIST_URI.into(),
        name: ARTIST.into(),
        image: Some(cover_url(PICTURE)),
    }
}

/// The artist's releases, oldest first.
pub fn discography() -> Discography {
    Discography {
        artist: artist(),
        releases: DISCOGRAPHY
            .iter()
            .enumerate()
            .map(|(n, (title, year, group, kind, songs))| Release {
                uri: format!("spotify:album:demo-{n}"),
                title: (*title).into(),
                artists: ARTIST.into(),
                year: *year,
                group: *group,
                kind: (*kind).into(),
                tracks: match songs.len() {
                    0 if *kind == "SINGLE" => 2,
                    0 if *kind == "EP" => 5,
                    0 => 9 + n % 5,
                    songs => songs,
                },
                cover: Some(cover_url(n)),
            })
            .collect(),
    }
}

/// The URLs of every cover there is, the artist's picture's included.
pub fn cover_urls() -> Vec<String> {
    (0..DISCOGRAPHY.len())
        .chain([PICTURE])
        .map(cover_url)
        .collect()
}

fn cover_url(n: usize) -> String {
    format!("{COVER_PREFIX}{n}")
}

/// A moment of the song playing, made up: interleaved stereo at the
/// player's rate, a chord and its overtones over a little noise, louder
/// low than high as music is, for the desktop deck's spectrum and levels.
pub fn sound() -> Vec<f32> {
    const FRAMES: usize = 2048;
    const NOTES: [(f32, f32); 10] = [
        (55.0, 0.30),
        (110.0, 0.22),
        (165.0, 0.12),
        (330.0, 0.10),
        (440.0, 0.08),
        (660.0, 0.05),
        (1320.0, 0.03),
        (2640.0, 0.015),
        (5280.0, 0.008),
        (10_560.0, 0.004),
    ];
    let rate = librespot_playback::SAMPLE_RATE as f32;
    // The noise from a fixed seed, so the same moment always sounds the same.
    let mut seed: u32 = 0x5107_1e55;
    let mut noise = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
    };
    let mut samples = Vec::with_capacity(FRAMES * 2);
    for frame in 0..FRAMES {
        let t = frame as f32 / rate;
        let chord: f32 = NOTES
            .iter()
            .map(|(hz, level)| level * (std::f32::consts::TAU * hz * t).sin())
            .sum();
        // The right a little quieter than the left, so the meters differ.
        samples.push(chord + 0.01 * noise());
        samples.push(0.8 * chord + 0.01 * noise());
    }
    samples
}

/// The cover at a demo URL: two colours split by a slant, and a disc.
/// None for any other URL.
pub fn cover(url: &str) -> Option<CoverImage> {
    const SIDE: u32 = 96;
    const COLOURS: [[u8; 3]; 8] = [
        [214, 69, 65],
        [38, 70, 83],
        [233, 196, 106],
        [42, 157, 143],
        [244, 162, 97],
        [94, 84, 142],
        [230, 230, 220],
        [20, 20, 24],
    ];
    let n: usize = url.strip_prefix(COVER_PREFIX)?.parse().ok()?;
    let side = SIDE as f32;
    let back = COLOURS[n % COLOURS.len()];
    let front = COLOURS[(n * 3 + 2) % COLOURS.len()];
    let disc = COLOURS[(n * 5 + 5) % COLOURS.len()];
    let slant = 0.3 + (n % 4) as f32 * 0.15;
    let centre = (side * (0.3 + (n % 3) as f32 * 0.2), side * 0.4);
    let radius = side * (0.16 + (n % 2) as f32 * 0.08);
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (x, y) = (x as f32, y as f32);
            let below = y > side * (1.0 - slant) + (x - side / 2.0) * 0.35;
            let in_disc = (x - centre.0).powi(2) + (y - centre.1).powi(2) < radius * radius;
            let colour = if in_disc {
                disc
            } else if below {
                front
            } else {
                back
            };
            rgba.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    Some(CoverImage {
        width: SIDE,
        height: SIDE,
        rgba,
    })
}

fn fill(library: &mut Library, id: PlaylistId, songs: &[Song]) {
    let slug = |name: &str| name.to_lowercase().replace(' ', "-");
    let entries: Vec<_> = songs
        .iter()
        .enumerate()
        .map(|(n, (artist, title, album, secs))| {
            library.entry(TrackRef {
                uri: format!("spotify:track:demo-{}-{n}", id.0),
                title: (*title).into(),
                artists: vec![ArtistRef {
                    uri: format!("spotify:artist:{}", slug(artist)),
                    name: (*artist).into(),
                    image: None,
                }],
                album: AlbumRef {
                    uri: format!("spotify:album:{}", slug(album)),
                    name: (*album).into(),
                },
                duration_ms: secs * 1000,
            })
        })
        .collect();
    library.get_mut(id).expect("made").append(entries);
}

/// Artist, title, album and length in seconds.
type Song = (&'static str, &'static str, &'static str, u32);

/// A run of songs from each album, as albums are added.
const NIGHT_DRIVE: &[Song] = &[
    ("The Low Hum", "Copper Wire", "Static Bloom", 214),
    ("The Low Hum", "Pilot Light", "Static Bloom", 192),
    ("The Low Hum", "Night Shift", "Static Bloom", 185),
    ("The Low Hum", "Last Train Home", "Static Bloom", 271),
    (
        "Marigold Engine",
        "Headlights on the Ridge",
        "Long Way Round",
        187,
    ),
    ("Marigold Engine", "Gasoline Hymn", "Long Way Round", 224),
    ("Marigold Engine", "Dust Devils", "Long Way Round", 202),
    ("Marigold Engine", "Rearview", "Long Way Round", 210),
    ("Paper Satellites", "Barometer", "Weather for Machines", 189),
    (
        "Paper Satellites",
        "Weather for Machines",
        "Weather for Machines",
        141,
    ),
    (
        "Paper Satellites",
        "Cold Front",
        "Weather for Machines",
        216,
    ),
    (
        "Paper Satellites",
        "Static Rain",
        "Weather for Machines",
        233,
    ),
    ("Glass Arcade", "Neon Tide", "Afterhours", 199),
    ("Glass Arcade", "Insert Coin", "Afterhours", 158),
    ("Glass Arcade", "High Score", "Afterhours", 166),
    ("Glass Arcade", "Continue?", "Afterhours", 95),
    (
        "Kestrel & the Tide",
        "Salt in the Gears",
        "Harbour Lights",
        231,
    ),
    (
        "Kestrel & the Tide",
        "Lighthouse Keeper",
        "Harbour Lights",
        248,
    ),
    ("Kestrel & the Tide", "Undertow", "Harbour Lights", 257),
    ("Kestrel & the Tide", "Tide Tables", "Harbour Lights", 188),
    ("Northbound Static", "Mile Marker 9", "Interstate", 176),
    ("Northbound Static", "Rest Stop Radio", "Interstate", 133),
    ("Northbound Static", "Exit Only", "Interstate", 148),
    (
        "Northbound Static",
        "Morning Fog Over 101",
        "Interstate",
        304,
    ),
    ("Hollow Pines", "Fog Machine", "Evergreen Noise", 263),
    ("Hollow Pines", "Antlers", "Evergreen Noise", 211),
    ("Hollow Pines", "Bonfire Season", "Evergreen Noise", 239),
    ("Hollow Pines", "Woodsmoke", "Evergreen Noise", 177),
    (
        "Velvet Oscillator",
        "Slow Burn Signal",
        "Warm Circuits",
        205,
    ),
    (
        "Velvet Oscillator",
        "Tape Hiss Lullaby",
        "Warm Circuits",
        297,
    ),
    (
        "Velvet Oscillator",
        "Ghost in the Mixer",
        "Warm Circuits",
        226,
    ),
    ("Velvet Oscillator", "Dawn Patrol", "Warm Circuits", 245),
];

const DEFAULT_SONGS: &[Song] = &[
    ("Glass Arcade", "Neon Tide", "Afterhours", 199),
    ("Hollow Pines", "Antlers", "Evergreen Noise", 211),
    ("The Low Hum", "Copper Wire", "Static Bloom", 214),
    ("Velvet Oscillator", "Warm Circuits", "Warm Circuits", 238),
    (
        "Kestrel & the Tide",
        "Harbour Lights",
        "Harbour Lights",
        226,
    ),
];

/// Title, year, group, kind and songs (title, seconds).
type Album = (
    &'static str,
    i32,
    ReleaseGroup,
    &'static str,
    &'static [(&'static str, u32)],
);

/// The artist's releases, oldest first; the songs of the first few make
/// up their playlist.
const DISCOGRAPHY: &[Album] = &[
    (
        "Low Orbit",
        2004,
        ReleaseGroup::Album,
        "ALBUM",
        &[
            ("Countdown", 72),
            ("Low Orbit", 242),
            ("Tin Can Telephone", 198),
            ("Gravity Well", 231),
            ("Re-entry", 207),
            ("Splashdown", 264),
        ],
    ),
    (
        "Weather for Machines",
        2006,
        ReleaseGroup::Album,
        "ALBUM",
        &[
            ("Barometer", 189),
            ("Weather for Machines", 141),
            ("Cold Front", 216),
            ("Static Rain", 233),
            ("Isobars", 176),
            ("Forecast", 249),
        ],
    ),
    (
        "Signal Fires",
        2008,
        ReleaseGroup::Album,
        "ALBUM",
        &[
            ("Kindling", 158),
            ("Signal Fires", 219),
            ("Smoke and Mirrors", 204),
            ("Embers", 281),
        ],
    ),
    ("The Quiet Year", 2010, ReleaseGroup::Album, "ALBUM", &[]),
    (
        "Live at the Lantern (Live)",
        2011,
        ReleaseGroup::Album,
        "ALBUM",
        &[],
    ),
    (
        "Paper Moons: B-Sides",
        2013,
        ReleaseGroup::Compilation,
        "COMPILATION",
        &[],
    ),
    ("Glasshouse", 2015, ReleaseGroup::Album, "ALBUM", &[]),
    ("Undertow Sessions", 2016, ReleaseGroup::Single, "EP", &[]),
    ("Satellite Hearts", 2018, ReleaseGroup::Album, "ALBUM", &[]),
    ("Northern Hum", 2020, ReleaseGroup::Single, "SINGLE", &[]),
    (
        "Everything Is Static",
        2022,
        ReleaseGroup::Album,
        "ALBUM",
        &[],
    ),
    ("Telemetry", 2025, ReleaseGroup::Album, "ALBUM", &[]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller::Controller;
    use crate::paths::Paths;
    use crate::playback::engine::Status;

    #[test]
    fn every_cover_is_drawn() {
        for url in cover_urls() {
            let image = cover(&url).expect(&url);
            assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
        }
        assert!(cover("https://i.scdn.co/image/a").is_none());
    }

    #[test]
    fn the_demo_plays_night_drive_signed_in() {
        let root = std::env::temp_dir().join(format!("spotless-demo-{}", std::process::id()));
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let controller = Controller::demo(paths);
        let state = controller.state();
        let playing = state.playing_list().expect("a song plays");
        assert_eq!(state.list(playing).unwrap().name, "Night Drive");
        let player = state.now_playing();
        assert_eq!(player.status, Status::Playing);
        assert_eq!(player.track.unwrap().title, "Weather for Machines");
        assert_eq!(player.position_ms, POSITION_MS);
        assert!(matches!(
            state.browse.artists.get(ARTIST_URI),
            Some(crate::browse::Loadable::Ready(_))
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
