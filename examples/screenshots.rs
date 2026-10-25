//! Draws the README's screenshots from made-up playlists and a made-up
//! artist, so no account, and nothing of anyone's, is ever in them.
//!
//! `cargo run --release --example screenshots -- docs/screenshots`

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use egui::{ColorImage, Key, UiBuilder, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use spotless::action::Actions;
use spotless::app::{Look, use_skin};
use spotless::backend::{SearchState, SessionState};
use spotless::browse::{Browse, Loadable, Page};
use spotless::lastfm::LastFmState;
use spotless::model::catalog::{Discography, Release, ReleaseGroup};
use spotless::model::{AlbumRef, ArtistRef, Library, Playlist, PlaylistId, TrackRef};
use spotless::paths::Paths;
use spotless::phosphor::Phosphor;
use spotless::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use spotless::skin::format::built_in::WINAMP_CLASSIC_KEY;
use spotless::store::Settings;
use spotless::ui::{self, Cover, Inputs, View};

/// The window's size, as Spotless opens at.
const SIZE: egui::Vec2 = vec2(960.0, 600.0);
/// Frames drawn before the picture is taken, for the tabs and the
/// display to settle.
const FRAMES: usize = 30;

#[derive(Clone, Copy)]
enum Screen {
    Playlists,
    Browse,
    /// The settings, on a category.
    Settings(&'static str),
}

struct Shot {
    file: &'static str,
    skin: Option<&'static str>,
    phosphor: Phosphor,
    screen: Screen,
}

const fn shot(
    file: &'static str,
    skin: Option<&'static str>,
    phosphor: Phosphor,
    screen: Screen,
) -> Shot {
    Shot {
        file,
        skin,
        phosphor,
        screen,
    }
}

const WINAMP: Option<&str> = Some(WINAMP_CLASSIC_KEY);

const SHOTS: [Shot; 6] = [
    shot("playlist.png", None, Phosphor::Green, Screen::Playlists),
    shot("browse-winamp.png", WINAMP, Phosphor::Green, Screen::Browse),
    shot(
        "playlist-winamp.png",
        WINAMP,
        Phosphor::Green,
        Screen::Playlists,
    ),
    shot("browse.png", None, Phosphor::Green, Screen::Browse),
    shot(
        "phosphor-amber.png",
        None,
        Phosphor::Amber,
        Screen::Playlists,
    ),
    shot(
        "settings.png",
        None,
        Phosphor::Green,
        Screen::Settings("Display"),
    ),
];

fn main() -> Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "docs/screenshots".into()),
    );
    std::fs::create_dir_all(&out)?;
    // The settings come from an empty folder: Spotless's defaults.
    let root = std::env::temp_dir().join(format!("spotless-screenshots-{}", std::process::id()));
    let paths = Paths::from_dirs(root.join("config"), root.join("cache"))?;
    let only: Vec<String> = std::env::args().skip(2).collect();
    for shot in &SHOTS {
        if !only.is_empty() && !only.iter().any(|name| shot.file.starts_with(name.as_str())) {
            continue;
        }
        let mut settings = Settings::load(&paths);
        settings.skin = shot.skin.map(String::from);
        settings.phosphor = shot.phosphor;
        settings.covers = true;
        settings.volume = 70;
        let demo = Demo::new(settings, shot.screen);
        let mut harness = Harness::builder()
            .with_size(SIZE)
            .with_pixels_per_point(1.0)
            .wgpu()
            .build_ui_state(|ui, demo: &mut Demo| demo.draw(ui), demo);
        harness.run_steps(2);
        if let Screen::Settings(category) = shot.screen {
            // As it would be opened: F10, then the category's name.
            harness.key_press(Key::F10);
            harness.run_steps(2);
            harness.get_by_label(category).click();
            harness.run_steps(2);
            harness.remove_cursor();
        }
        harness.run_steps(FRAMES);
        let image = harness
            .render()
            .map_err(|error| anyhow::anyhow!("drawing {}: {error}", shot.file))?;
        let to = out.join(shot.file);
        // No transparency: RGB keeps the files smaller.
        image::DynamicImage::from(image)
            .to_rgb8()
            .save(&to)
            .with_context(|| format!("writing {}", to.display()))?;
        println!("{}", to.display());
    }
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// Everything the window shows, made up.
struct Demo {
    settings: Settings,
    library: Library,
    playing: PlaylistId,
    browse: Browse,
    player: PlayerSnapshot,
    inputs: Inputs,
    /// The skin, loaded on the first frame, with the context.
    look: Option<Look>,
    covers: HashMap<String, Cover>,
}

impl Demo {
    fn new(settings: Settings, screen: Screen) -> Self {
        let mut library = Library::new();
        fill(&mut library, PlaylistId::DEFAULT, DEFAULT_SONGS);
        let playing = library.create("Night Drive");
        fill(&mut library, playing, NIGHT_DRIVE);
        let artist_list = library.create(ARTIST);
        let artist_songs: Vec<_> = DISCOGRAPHY
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
        let now = list.entries()[NOW_PLAYING].clone();
        list.set_cursor(now.id);
        let player = PlayerSnapshot {
            status: Status::Playing,
            track: Some(TrackInfo {
                uri: now.track.uri.clone(),
                title: now.track.title.clone(),
                artists: now.track.artists[0].name.clone(),
                main_artist: now.track.artists[0].name.clone(),
                album: now.track.album.name.clone(),
                duration_ms: now.track.duration_ms,
                cover_url: None,
            }),
            position_ms: 68_000,
            reported_at: None,
            error: None,
        };

        let mut inputs = Inputs {
            volume: settings.volume,
            ..Inputs::default()
        };
        let mut browse = Browse::new(playing, settings.covers);
        match screen {
            Screen::Playlists => {
                inputs.selection.rows.insert(now.id);
            }
            Screen::Browse => {
                let uri = "spotify:artist:demo".to_string();
                browse.open = true;
                browse.target = artist_list;
                browse.go(Page::Search(ARTIST.to_lowercase()));
                browse.go(Page::Artist(uri.clone()));
                browse
                    .artists
                    .insert(uri.clone(), Loadable::Ready(discography(uri)));
                inputs.query = ARTIST.to_lowercase();
            }
            Screen::Settings(_) => {}
        }
        Self {
            settings,
            library,
            playing,
            browse,
            player,
            inputs,
            look: None,
            covers: HashMap::new(),
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.look.is_none() {
            let look = use_skin(&ctx, self.settings.skin.as_deref(), self.settings.phosphor)
                .expect("the built-in skins load");
            self.look = Some(look);
            for n in (0..DISCOGRAPHY.len()).chain([PICTURE]) {
                let texture =
                    ctx.load_texture(cover_url(n), cover_image(n), egui::TextureOptions::LINEAR);
                self.covers.insert(cover_url(n), Cover::Ready(texture));
            }
        }
        let look = self.look.as_ref().expect("loaded above");
        let tabs: Vec<_> = self.library.playlists().iter().map(|l| l.id).collect();
        let shown = if self.browse.open {
            self.browse.target
        } else {
            self.playing
        };
        let spotify_list = Playlist::new(PlaylistId::SPOTIFY, "Spotify");
        let outputs = Loadable::Ready(None);
        let lastfm = LastFmState::default();
        let view = View {
            palette: look.palette,
            session: &SessionState::Ready {
                user: "listener".into(),
            },
            player: &self.player,
            error: None,
            notice: None,
            playlist: self.library.get(shown).expect("made in new"),
            shuffle: false,
            repeat: false,
            library: &self.library,
            tabs: &tabs,
            playing_list: Some(self.playing),
            spotify: None,
            spotify_list: &spotify_list,
            browse: &self.browse,
            covers: &self.covers,
            search: &SearchState::Ready,
            lastfm: &lastfm,
            web_client_id: None,
            settings: &self.settings,
            storage: None,
            outputs: &outputs,
            tray_shown: false,
            art: look.art.as_deref(),
            skin_note: None,
            skin_error: None,
        };
        // The window's whole area, without the margin the harness keeps.
        let full = ctx.content_rect();
        ui.scope_builder(UiBuilder::new().max_rect(full), |ui| {
            ui.set_clip_rect(full);
            // What the views ask for is dropped: the picture stays as made.
            ui::draw(ui, &view, &mut self.inputs, &mut Actions::default());
        });
    }
}

fn fill(library: &mut Library, id: PlaylistId, songs: &[(&str, &str, &str, u32)]) {
    let entries: Vec<_> = songs
        .iter()
        .enumerate()
        .map(|(n, (artist, title, album, secs))| {
            library.entry(TrackRef {
                uri: format!("spotify:track:demo-{}-{n}", id.0),
                title: (*title).into(),
                artists: vec![ArtistRef {
                    uri: format!("spotify:artist:{}", artist.to_lowercase().replace(' ', "-")),
                    name: (*artist).into(),
                    image: None,
                }],
                album: AlbumRef {
                    uri: format!("spotify:album:{}", album.to_lowercase().replace(' ', "-")),
                    name: (*album).into(),
                },
                duration_ms: secs * 1000,
            })
        })
        .collect();
    library.get_mut(id).expect("made").append(entries);
}

fn discography(uri: String) -> Discography {
    Discography {
        artist: ArtistRef {
            uri,
            name: ARTIST.into(),
            image: Some(cover_url(PICTURE)),
        },
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

fn cover_url(n: usize) -> String {
    format!("demo:cover/{n}")
}

/// An album cover: two colours split by a slant, and a disc.
fn cover_image(n: usize) -> ColorImage {
    const SIDE: usize = 96;
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
    let back = COLOURS[n % COLOURS.len()];
    let front = COLOURS[(n * 3 + 2) % COLOURS.len()];
    let disc = COLOURS[(n * 5 + 5) % COLOURS.len()];
    let slant = 0.3 + (n % 4) as f32 * 0.15;
    let centre = (
        SIDE as f32 * (0.3 + (n % 3) as f32 * 0.2),
        SIDE as f32 * 0.4,
    );
    let radius = SIDE as f32 * (0.16 + (n % 2) as f32 * 0.08);
    let mut pixels = Vec::with_capacity(SIDE * SIDE * 4);
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (fx, fy) = (x as f32, y as f32);
            let below = fy > SIDE as f32 * (1.0 - slant) + (fx - SIDE as f32 / 2.0) * 0.35;
            let in_disc = (fx - centre.0).powi(2) + (fy - centre.1).powi(2) < radius * radius;
            let colour = if in_disc {
                disc
            } else if below {
                front
            } else {
                back
            };
            pixels.extend_from_slice(&[colour[0], colour[1], colour[2], 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([SIDE, SIDE], &pixels)
}

/// The artist Browse shows; none of these exist.
const ARTIST: &str = "Paper Satellites";

/// The artist's picture, among the covers.
const PICTURE: usize = 99;

/// The song playing, in Night Drive.
const NOW_PLAYING: usize = 9;

type Song = (&'static str, &'static str, &'static str, u32);

const NIGHT_DRIVE: &[Song] = &[
    ("The Low Hum", "Copper Wire", "Static Bloom", 214),
    (
        "Marigold Engine",
        "Headlights on the Ridge",
        "Long Way Round",
        187,
    ),
    ("Paper Satellites", "Low Orbit", "Low Orbit", 242),
    ("Glass Arcade", "Neon Tide", "Afterhours", 199),
    (
        "Kestrel & the Tide",
        "Salt in the Gears",
        "Harbour Lights",
        231,
    ),
    ("Northbound Static", "Mile Marker 9", "Interstate", 176),
    ("Hollow Pines", "Fog Machine", "Evergreen Noise", 263),
    (
        "Velvet Oscillator",
        "Slow Burn Signal",
        "Warm Circuits",
        205,
    ),
    ("The Low Hum", "Pilot Light", "Static Bloom", 192),
    (
        "Paper Satellites",
        "Weather for Machines",
        "Weather for Machines",
        141,
    ),
    ("Glass Arcade", "Insert Coin", "Afterhours", 158),
    ("Marigold Engine", "Gasoline Hymn", "Long Way Round", 224),
    (
        "Kestrel & the Tide",
        "Lighthouse Keeper",
        "Harbour Lights",
        248,
    ),
    ("Northbound Static", "Rest Stop Radio", "Interstate", 133),
    ("Hollow Pines", "Antlers", "Evergreen Noise", 211),
    (
        "Velvet Oscillator",
        "Tape Hiss Lullaby",
        "Warm Circuits",
        297,
    ),
    ("The Low Hum", "Night Shift", "Static Bloom", 185),
    ("Paper Satellites", "Signal Fires", "Signal Fires", 219),
    ("Glass Arcade", "High Score", "Afterhours", 166),
    ("Marigold Engine", "Dust Devils", "Long Way Round", 202),
    ("Kestrel & the Tide", "Undertow", "Harbour Lights", 257),
    ("Northbound Static", "Exit Only", "Interstate", 148),
    ("Hollow Pines", "Bonfire Season", "Evergreen Noise", 239),
    (
        "Velvet Oscillator",
        "Ghost in the Mixer",
        "Warm Circuits",
        226,
    ),
    ("The Low Hum", "Last Train Home", "Static Bloom", 271),
    ("Paper Satellites", "The Quiet Year", "The Quiet Year", 233),
    ("Glass Arcade", "Continue?", "Afterhours", 95),
    ("Marigold Engine", "Rearview", "Long Way Round", 210),
    ("Kestrel & the Tide", "Tide Tables", "Harbour Lights", 188),
    (
        "Northbound Static",
        "Morning Fog Over 101",
        "Interstate",
        304,
    ),
    ("Hollow Pines", "Woodsmoke", "Evergreen Noise", 177),
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

type Album = (
    &'static str,
    i32,
    ReleaseGroup,
    &'static str,
    &'static [(&'static str, u32)],
);

/// Paper Satellites' releases, oldest first; the songs of the first few
/// make up their playlist.
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
