//! Draws the README's desktop screenshots from the core's made-up
//! library and artist (`spotless_core::demo`), so no account, and nothing of
//! anyone's, is ever in them.
//!
//! `cargo run --release --example screenshots -- docs/screenshots`

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use egui::{ColorImage, Key, UiBuilder, vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use spotless::action::{Actions, BrowseAction, PlaybackAction, SettingsAction, ViewAction};
use spotless::app::{Look, use_skin};
use spotless::settings::DesktopSettings;
use spotless::skin::format::built_in::WINAMP_CLASSIC_KEY;
use spotless::ui::{self, Cover, Inputs, View};
use spotless_core::Controller;
use spotless_core::Paths;
use spotless_core::browse::Loadable;
use spotless_core::demo;
use spotless_core::settings::Phosphor;
use spotless_core::settings::{DeckMedia, DeckMeter, VuFace};

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
    /// The deck under the player (VIS), playing on this.
    deck: Option<DeckMedia>,
    /// The deck's VU meters, on this face, instead of the spectrum.
    vu: Option<VuFace>,
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
        deck: None,
        vu: None,
    }
}

/// The playlists with the deck showing, on `media`.
const fn deck(file: &'static str, media: DeckMedia) -> Shot {
    Shot {
        deck: Some(media),
        ..shot(file, None, Phosphor::Green, Screen::Playlists)
    }
}

/// The deck on `media`, with the VU meters on `face`.
const fn vu(file: &'static str, media: DeckMedia, face: VuFace) -> Shot {
    Shot {
        vu: Some(face),
        ..deck(file, media)
    }
}

const WINAMP: Option<&str> = Some(WINAMP_CLASSIC_KEY);

const SHOTS: [Shot; 11] = [
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
    deck("deck-cassette.png", DeckMedia::Cassette),
    deck("deck-reel.png", DeckMedia::Reel),
    deck("deck-cd.png", DeckMedia::Cd),
    deck("deck-vinyl.png", DeckMedia::Vinyl),
    vu("deck-vu.png", DeckMedia::Cassette, VuFace::Cream),
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
        let demo = Demo::new(paths.clone(), shot);
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

/// Spotless with the core's made-up library (`spotless_core::demo`), and the
/// window's own state.
struct Demo {
    controller: Controller,
    /// The settings only the desktop has: the skin.
    desktop: DesktopSettings,
    inputs: Inputs,
    /// The skin, loaded on the first frame, with the context.
    look: Option<Look>,
    covers: HashMap<String, Cover>,
    /// The music the deck hears, when it shows.
    sound: Option<Vec<f32>>,
}

impl Demo {
    fn new(paths: Paths, shot: &Shot) -> Self {
        let mut controller = Controller::demo(paths);
        let mut desktop = DesktopSettings::read(controller.state().settings.extra());
        desktop.skin = shot.skin.map(String::from);
        controller.apply(SettingsAction::SetPhosphor(shot.phosphor));
        controller.apply(PlaybackAction::Volume(70));
        if let Some(media) = shot.deck {
            if !controller.state().settings.deck_shown {
                controller.apply(ViewAction::ToggleDeck);
            }
            controller.apply(ViewAction::SetDeckMedia(media));
        }
        if let Some(face) = shot.vu {
            controller.apply(ViewAction::SetDeckMeter(DeckMeter::Vu));
            controller.apply(ViewAction::SetVuFace(face));
        }
        let mut inputs = Inputs {
            volume: 70,
            ..Inputs::default()
        };
        match shot.screen {
            Screen::Playlists => {
                // The playing song selected, as after a double-click.
                let state = controller.state();
                let now = state.list(state.viewed).and_then(|list| list.cursor());
                inputs.selection.rows.extend(now);
            }
            Screen::Browse => {
                // Found by a search, as it would be.
                inputs.query = demo::ARTIST.to_lowercase();
                controller.apply(BrowseAction::ToggleBrowse);
                controller.apply(BrowseAction::OpenArtist(demo::artist()));
            }
            Screen::Settings(_) => {}
        }
        Self {
            controller,
            desktop,
            inputs,
            look: None,
            covers: HashMap::new(),
            sound: shot.deck.map(|_| demo::sound()),
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let state = self.controller.state();
        let look = self.look.get_or_insert_with(|| {
            use_skin(&ctx, self.desktop.skin.as_deref(), state.settings.phosphor)
                .expect("the built-in skins load")
        });
        if self.covers.is_empty() {
            for url in demo::cover_urls() {
                let image = demo::cover(&url).expect("a demo cover");
                let texture = ctx.load_texture(
                    &url,
                    ColorImage::from_rgba_unmultiplied(
                        [image.width as usize, image.height as usize],
                        &image.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                );
                self.covers.insert(url, Cover::Ready(texture));
            }
        }
        // The song as it sounds now, as the audio output would feed it.
        if let Some(sound) = &self.sound {
            self.controller.audio().analyser.feed(sound, 1.0);
        }
        let player = state.now_playing();
        let tabs = state.tabs();
        let outputs = Loadable::Ready(None);
        let view = View {
            palette: look.palette,
            session: &state.session,
            player: &player,
            error: None,
            notice: None,
            can_undo_reorder: false,
            playlist: state.list(state.viewed).expect("the demo's"),
            left_ms: state.left_ms(state.viewed),
            shuffle: state.settings.shuffle,
            repeat: state.settings.repeat,
            stop_after: false,
            library: &state.library,
            sync: &state.sync,
            sync_name_taken: state.sync_name_taken,
            tabs: &tabs,
            playing_list: state.playing_list(),
            spotify: None,
            spotify_list: &state.spotify_list,
            browse: &state.browse,
            covers: &self.covers,
            search: &state.search,
            lastfm: &state.lastfm,
            loved: &state.loved,
            web_client_id: None,
            settings: &state.settings,
            desktop: &self.desktop,
            analyser: &self.controller.audio().analyser,
            storage: None,
            outputs: &outputs,
            tray_shown: false,
            art: look.art.as_deref(),
            skin_note: None,
            skin_error: None,
            update: &spotless::update::Status::Unknown,
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
