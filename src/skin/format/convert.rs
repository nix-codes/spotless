//! A classic Winamp skin turned into Spotless's format.
//!
//! Every classic skin keeps its pieces at the same places in the same
//! bitmaps, so one recipe cuts them all: the buttons, sliders, digits and
//! font as they are, and from the main window's background the boxes that
//! frame the time, the song and the bitrate. Where Winamp's art isn't one
//! piece, a small image is put together: the title bar's stripes with both
//! ends, the plate behind its name, and the time's box with its dotted
//! grid as a tile, the visualiser's axis and the colon left out.

use std::fmt::Write;

use egui::Color32;
use image::{Rgba, RgbaImage};

use super::MANIFEST;
use crate::skin::sprites::{self, Sheet, Sprite};
use crate::skin::{Bitmap, Skin};
use crate::ui::theme::Palette;

/// Winamp's main window at half again its size, as Spotless drew it.
const SCALE: f32 = 1.5;

/// Boxes in the main window's background, as (x, y, width, height).
const LCD: (u32, u32, u32, u32) = (11, 22, 93, 43);
const DISPLAY: (u32, u32, u32, u32) = (112, 23, 155, 14);
const SMALL_DISPLAY: (u32, u32, u32, u32) = (108, 40, 20, 12);

/// The title bar's stripes: the left end (the right one is it mirrored),
/// and the stripes that tile; then the title piece's ends around the name.
const TITLE_END: u32 = 25;
const TITLE_CAP: u32 = 5;
/// How far into the title piece its plain middle starts.
const TITLE_PLAIN: u32 = 5;
const TITLE_GAP: u32 = 8;

/// The skin's files in Spotless's format: `skin.toml` and its images.
/// A skin without the player's bitmaps gives only its colours.
pub fn convert(skin: &Skin, name: &str) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut manifest = String::new();
    let _ = writeln!(manifest, "# Converted from a classic Winamp skin.");
    let _ = writeln!(manifest, "format = 1");
    let _ = writeln!(manifest, "name = {}", quoted(name));
    let _ = writeln!(manifest, "scale = {SCALE}");
    let _ = writeln!(manifest, "filter = \"pixel\"");
    colors(&mut manifest, &skin.palette);

    let mut add = |path: &str, image: RgbaImage| files.push((path.to_string(), png(&image)));
    for (sheet, path) in [
        (Sheet::CButtons, "cbuttons.png"),
        (Sheet::ShufRep, "shufrep.png"),
        (Sheet::PosBar, "posbar.png"),
        (Sheet::Volume, "volume.png"),
        (Sheet::PlayPaus, "playpaus.png"),
        (Sheet::Text, "text.png"),
        (Sheet::PlEdit, "pledit.png"),
    ] {
        if let Some(bitmap) = skin.sheet(sheet) {
            add(path, image(bitmap));
        }
    }
    if let Some(digits) = skin.digits().and_then(|sheet| skin.sheet(sheet)) {
        add("digits.png", image(digits));
    }
    if let Some(main) = skin.sheet(Sheet::Main) {
        let main = image(main);
        add("lcd.png", lcd(&main));
        add("display.png", crop(&main, DISPLAY));
        add("small-display.png", crop(&main, SMALL_DISPLAY));
    }
    if let Some(pledit) = skin.sheet(Sheet::PlEdit) {
        let pledit = image(pledit);
        add("titlebar.png", titlebar(&pledit));
        add("title.png", title(&pledit));
    }

    if skin.missing_art().is_empty() {
        player(&mut manifest, skin);
    }
    if skin.sheet(Sheet::PlEdit).is_some() {
        playlist(&mut manifest);
    }
    files.push((MANIFEST.to_string(), manifest.into_bytes()));
    files
}

fn colors(manifest: &mut String, palette: &Palette) {
    let _ = writeln!(manifest, "\n[colors]");
    let colors = [
        ("desk", palette.desk),
        ("window", palette.frame),
        ("edge_light", palette.hi),
        ("edge_dark", palette.lo),
        ("chrome_text", palette.chrome),
        ("button", palette.button),
        ("lcd_bg", palette.lcd_bg),
        ("lcd_text", palette.lcd),
        ("list_bg", palette.list_bg),
        ("list_text", palette.list_fg),
        ("list_current", palette.current),
        ("list_selected_bg", palette.selected_bg),
        ("list_dim", palette.dim),
        ("accent", palette.accent),
        ("scroll_thumb", palette.thumb),
    ];
    for (key, color) in colors {
        let _ = writeln!(manifest, "{key} = \"{}\"", hex(color));
    }
}

/// The player strip's pieces and fonts.
fn player(manifest: &mut String, skin: &Skin) {
    let digits = match skin.digits() {
        Some(Sheet::NumsEx) => "0123456789 -",
        _ => "0123456789",
    };
    let _ = writeln!(
        manifest,
        "\n[fonts.digits]\nbitmap = {{ image = \"digits.png\", cell = [9, 13], rows = [\"{digits}\"] }}"
    );
    // Winamp's font: capitals, digits, punctuation, three accented
    // letters. The cells it leaves blank are skipped with a character no
    // title has.
    let _ = writeln!(
        manifest,
        "\n[fonts.display]\nbitmap = {{ image = \"text.png\", cell = [5, 6], rows = [\n  {},\n  {},\n  {},\n] }}",
        quoted("ABCDEFGHIJKLMNOPQRSTUVWXYZ\"@\u{1}\u{1} "),
        quoted("0123456789\u{2026}.:()-'!_+\\/[]^&%,=$#"),
        quoted("\u{c5}\u{d6}\u{c4}?*"),
    );

    let _ = writeln!(manifest, "\n[pieces]");
    let frame = |manifest: &mut String, name: &str, image: &str, padding: &str, center: &str| {
        let _ = writeln!(
            manifest,
            "{name} = {{ image = \"{image}\", slice = [1, 1, 1, 1], padding = {padding}, center = \"{center}\" }}"
        );
    };
    frame(manifest, "lcd", "lcd.png", "[1, 1, 1, 1]", "tile");
    frame(
        manifest,
        "display",
        "display.png",
        "[3, 3, 3, 3]",
        "stretch",
    );
    frame(
        manifest,
        "small_display",
        "small-display.png",
        "[2, 3, 2, 3]",
        "stretch",
    );

    let states = |manifest: &mut String, name: &str, states: &[(&str, Sprite)]| {
        let list: Vec<String> = states
            .iter()
            .map(|(state, sprite)| format!("{state} = {}", reference(*sprite)))
            .collect();
        let _ = writeln!(manifest, "{name} = {{ {} }}", list.join(", "));
    };
    use sprites as s;
    let buttons = [
        ("previous", s::PREVIOUS, s::PREVIOUS_PRESSED),
        ("play", s::PLAY, s::PLAY_PRESSED),
        ("pause", s::PAUSE, s::PAUSE_PRESSED),
        ("stop", s::STOP, s::STOP_PRESSED),
        ("next", s::NEXT, s::NEXT_PRESSED),
        ("eject", s::EJECT, s::EJECT_PRESSED),
    ];
    for (name, normal, pressed) in buttons {
        states(manifest, name, &[("normal", normal), ("pressed", pressed)]);
    }
    let toggles = [
        (
            "shuffle",
            [
                s::SHUFFLE_OFF,
                s::SHUFFLE_OFF_PRESSED,
                s::SHUFFLE_ON,
                s::SHUFFLE_ON_PRESSED,
            ],
        ),
        (
            "repeat",
            [
                s::REPEAT_OFF,
                s::REPEAT_OFF_PRESSED,
                s::REPEAT_ON,
                s::REPEAT_ON_PRESSED,
            ],
        ),
        ("pl", [s::PL, s::PL_PRESSED, s::PL_LIT, s::PL_LIT_PRESSED]),
    ];
    for (name, [normal, pressed, on, on_pressed]) in toggles {
        let all = [
            ("normal", normal),
            ("pressed", pressed),
            ("on", on),
            ("on_pressed", on_pressed),
        ];
        states(manifest, name, &all);
    }
    let _ = writeln!(
        manifest,
        "status.playing = {{ image = {} }}\nstatus.paused = {{ image = {} }}\nstatus.stopped = {{ image = {} }}",
        reference(s::STATUS_PLAYING),
        reference(s::STATUS_PAUSED),
        reference(s::STATUS_STOPPED),
    );
    // The seek track's ends stay as they are so its shading doesn't
    // stretch: an eighth of it each, as Spotless drew it.
    let ends = s::POSITION_TRACK.width / 8;
    let _ = writeln!(
        manifest,
        "seek.track = {{ image = {}, slice = [{ends}, {ends}] }}",
        reference(s::POSITION_TRACK)
    );
    states(
        manifest,
        "seek.thumb",
        &[
            ("normal", s::POSITION_THUMB),
            ("pressed", s::POSITION_THUMB_PRESSED),
        ],
    );
    // Winamp redraws the whole volume bar in one of 28 shades; here the
    // loudest is drawn over the quietest up to the level.
    let _ = writeln!(
        manifest,
        "volume.track = {{ image = {}, slice = [2, 2] }}\nvolume.fill = {{ image = {}, slice = [2, 2] }}",
        reference(s::volume_frame(0)),
        reference(s::volume_frame(sprites::SLIDER_FRAMES - 1)),
    );
    states(
        manifest,
        "volume.thumb",
        &[
            ("normal", s::VOLUME_THUMB),
            ("pressed", s::VOLUME_THUMB_PRESSED),
        ],
    );
}

/// The title bars and the scrollbar, from the playlist editor.
fn playlist(manifest: &mut String) {
    let _ = writeln!(
        manifest,
        "\n[pieces.titlebar]\nnormal = {{ image = \"titlebar.png\", rect = [0, 0, {w}, 14] }}\nunlit = {{ image = \"titlebar.png\", rect = [0, 14, {w}, 14] }}\nslice = [{TITLE_END}, 0, {TITLE_END}, 0]\ncenter = \"tile\"",
        w = 3 * TITLE_END,
    );
    let _ = writeln!(
        manifest,
        "\n[pieces.title]\nnormal = {{ image = \"title.png\", rect = [0, 0, {w}, 14] }}\nunlit = {{ image = \"title.png\", rect = [0, 14, {w}, 14] }}\nslice = [{TITLE_CAP}, {TITLE_CAP}]\ntitle_gap = {TITLE_GAP}",
        w = 2 * TITLE_CAP + 1,
    );
    let _ = writeln!(
        manifest,
        "\n[pieces.scrollbar]\ntrack = {{ image = {}, slice = [0, 0], middle = \"tile\" }}\nthumb = {{ normal = {}, pressed = {}, slice = [0, 0] }}",
        reference(sprites::SCROLL_SLOT),
        reference(sprites::SCROLL_THUMB),
        reference(sprites::SCROLL_THUMB_PRESSED),
    );
}

/// Where a sprite is in the converted images.
fn reference(sprite: Sprite) -> String {
    let image = match sprite.sheet {
        Sheet::CButtons => "cbuttons.png",
        Sheet::ShufRep => "shufrep.png",
        Sheet::PosBar => "posbar.png",
        Sheet::Volume => "volume.png",
        Sheet::PlayPaus => "playpaus.png",
        Sheet::Text => "text.png",
        Sheet::PlEdit => "pledit.png",
        Sheet::Numbers | Sheet::NumsEx => "digits.png",
        Sheet::Main => "main.png",
    };
    format!(
        "{{ image = \"{image}\", rect = [{}, {}, {}, {}] }}",
        sprite.x, sprite.y, sprite.width, sprite.height
    )
}

/// The time's box as a 4x4 image: its corners, its edges from the
/// middle of each side (clear of what's drawn inside near the corners),
/// and a 2x2 tile of the dotted grid, in step with the box's inner corner.
fn lcd(main: &RgbaImage) -> RgbaImage {
    let (x, y, w, h) = LCD;
    let at = |dx: u32, dy: u32| pixel(main, x + dx, y + dy);
    let (right, bottom, mid_x, mid_y) = (w - 1, h - 1, w / 2, h / 2);
    let columns = [0, mid_x, mid_x + 1, right];
    let rows = [0, mid_y, mid_y + 1, bottom];
    RgbaImage::from_fn(4, 4, |ox, oy| {
        let inner = (1..=2).contains(&ox) && (1..=2).contains(&oy);
        if inner {
            at(ox, oy)
        } else {
            at(columns[ox as usize], rows[oy as usize])
        }
    })
}

/// The title bar's stripes for a lit and an unlit window: the left end,
/// a tile of stripes, the left end mirrored.
fn titlebar(pledit: &RgbaImage) -> RgbaImage {
    let mut out = RgbaImage::new(3 * TITLE_END, 28);
    for (row, y) in [
        (0, sprites::TITLE_LEFT.y),
        (14, sprites::TITLE_LEFT_UNLIT.y),
    ] {
        for dy in 0..14 {
            for dx in 0..TITLE_END {
                let end = pixel(pledit, dx, y + dy);
                let tile = pixel(pledit, sprites::TITLE_TILE.x + dx, y + dy);
                out.put_pixel(dx, row + dy, end);
                out.put_pixel(TITLE_END + dx, row + dy, tile);
                out.put_pixel(3 * TITLE_END - 1 - dx, row + dy, end);
            }
        }
    }
    out
}

/// The plate behind a title bar's name: the title piece's two ends, and
/// one column of its plain middle between them to stretch.
fn title(pledit: &RgbaImage) -> RgbaImage {
    let piece = sprites::TITLE_MIDDLE;
    let width = 2 * TITLE_CAP + 1;
    let mut out = RgbaImage::new(width, 28);
    for (row, y) in [(0, piece.y), (14, sprites::TITLE_MIDDLE_UNLIT.y)] {
        for dy in 0..14 {
            for dx in 0..TITLE_CAP {
                out.put_pixel(dx, row + dy, pixel(pledit, piece.x + dx, y + dy));
                let end = piece.x + piece.width - TITLE_CAP + dx;
                out.put_pixel(TITLE_CAP + 1 + dx, row + dy, pixel(pledit, end, y + dy));
            }
            let plain = pixel(pledit, piece.x + TITLE_PLAIN, y + dy);
            out.put_pixel(TITLE_CAP, row + dy, plain);
        }
    }
    out
}

fn crop(image: &RgbaImage, (x, y, w, h): (u32, u32, u32, u32)) -> RgbaImage {
    RgbaImage::from_fn(w, h, |dx, dy| pixel(image, x + dx, y + dy))
}

/// A pixel, or clear past the image's edge: some skins' bitmaps are short.
fn pixel(image: &RgbaImage, x: u32, y: u32) -> Rgba<u8> {
    if x < image.width() && y < image.height() {
        *image.get_pixel(x, y)
    } else {
        Rgba([0, 0, 0, 0])
    }
}

fn image(bitmap: &Bitmap) -> RgbaImage {
    RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba.clone())
        .unwrap_or_else(|| RgbaImage::new(1, 1))
}

fn png(image: &RgbaImage) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    // Encoding an image in memory only fails on a zero-sized image.
    let _ = image.write_to(&mut bytes, image::ImageFormat::Png);
    bytes.into_inner()
}

fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
}

/// A TOML basic string.
fn quoted(text: &str) -> String {
    let mut out = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::format;

    /// A classic skin of plain bitmaps at Winamp's sizes.
    fn classic() -> Skin {
        let bmp = |width: u32, height: u32| {
            let image = image::RgbImage::from_fn(width, height, |x, y| {
                image::Rgb([(x % 251) as u8, (y % 251) as u8, 40])
            });
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Bmp).unwrap();
            bytes.into_inner()
        };
        let sheets = [
            ("main.bmp", bmp(275, 116)),
            ("cbuttons.bmp", bmp(136, 36)),
            ("shufrep.bmp", bmp(92, 85)),
            ("posbar.bmp", bmp(307, 10)),
            ("volume.bmp", bmp(68, 433)),
            ("playpaus.bmp", bmp(42, 9)),
            ("numbers.bmp", bmp(99, 13)),
            ("text.bmp", bmp(155, 18)),
            ("pledit.bmp", bmp(280, 186)),
        ];
        let entries: Vec<(&str, &[u8], bool)> = sheets
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice(), false))
            .collect();
        let dir = std::env::temp_dir().join(format!("spotless-wsz-{}.wsz", fastrand::u64(..)));
        std::fs::write(&dir, crate::skin::zip::write(&entries)).unwrap();
        let skin = Skin::load(&dir).unwrap();
        std::fs::remove_file(&dir).unwrap();
        skin
    }

    #[test]
    fn a_classic_skin_converts_without_a_problem() {
        let files = convert(&classic(), "Winamp \"Classic\"");
        let refs: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(path, bytes)| (path.as_str(), bytes.as_slice()))
            .collect();
        let skin = format::Skin::built_in("x", &refs).unwrap();
        assert!(skin.problems.is_empty(), "{:#?}", skin.problems);
        assert_eq!(skin.name, "Winamp \"Classic\"");
        assert_eq!(skin.scale, SCALE);
        for (name, _) in format::PIECES {
            let expected = !matches!(
                *name,
                "window"
                    | "statusbar"
                    | "seek.fill"
                    | "panel"
                    | "tab"
                    | "row_selected"
                    | "button"
                    | "small_button"
                    | "field"
                    | "switch"
                    | "radio"
            );
            assert_eq!(skin.piece(name).is_some(), expected, "{name}");
        }
        let display = &skin.fonts[&format::Role::Display];
        assert_eq!(display.glyph(' ').map(|a| (a.x, a.y)), Some((150, 0)));
        assert_eq!(display.glyph('#').map(|a| (a.x, a.y)), Some((150, 6)));
        assert_eq!(skin.fonts[&format::Role::Digits].glyph('9').unwrap().x, 81);
    }

    #[test]
    fn a_skin_without_the_players_bitmaps_gives_its_colours() {
        let files = convert(&Skin::from_files(&Default::default()), "Bare");
        assert_eq!(files.len(), 1);
        let manifest = String::from_utf8(files[0].1.clone()).unwrap();
        assert!(manifest.contains("[colors]") && !manifest.contains("[pieces"));
    }

    #[test]
    fn the_time_box_is_cut_to_a_tile_of_its_grid() {
        let mut main = RgbaImage::from_pixel(275, 116, Rgba([1, 1, 1, 255]));
        let (x, y, ..) = LCD;
        main.put_pixel(x + 1, y + 2, Rgba([9, 9, 9, 255]));
        let tile = lcd(&main);
        assert_eq!(tile.dimensions(), (4, 4));
        assert_eq!(*tile.get_pixel(1, 2), Rgba([9, 9, 9, 255]));
    }
}
