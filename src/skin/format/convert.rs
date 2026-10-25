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
    let palette = skin.palette;

    let mut add = |path: &str, image: RgbaImage| files.push((path.to_string(), png(&image)));
    let mut sheets = Vec::new();
    for (sheet, path, size) in [
        (Sheet::CButtons, "cbuttons.png", (136, 36)),
        (Sheet::ShufRep, "shufrep.png", (92, 85)),
        (Sheet::PosBar, "posbar.png", (307, 10)),
        (Sheet::Volume, "volume.png", (68, 433)),
        (Sheet::PlayPaus, "playpaus.png", (42, 9)),
        (Sheet::Text, "text.png", (155, 18)),
        (Sheet::PlEdit, "pledit.png", (280, 186)),
        (Sheet::EqMain, "eqmain.png", (275, 315)),
    ] {
        if let Some(bitmap) = skin.sheet(sheet) {
            sheets.push((sheet, path, padded(&image(bitmap), size)));
        }
    }
    cut_backgrounds(&mut sheets);
    // Of the equaliser's window, only its switches and thumb are used.
    for (sheet, _, image) in &mut sheets {
        if *sheet == Sheet::EqMain {
            *image = only(image, &EQ_PIECES);
        }
    }
    colors(&mut manifest, &palette);
    for (_, path, image) in sheets {
        add(path, image);
    }
    if let Some(sheet) = skin.digits()
        && let Some(digits) = skin.sheet(sheet)
    {
        let width = if sheet == Sheet::NumsEx { 108 } else { 99 };
        add("digits.png", padded(&image(digits), (width, 13)));
    }
    if let Some(main) = skin.sheet(Sheet::Main) {
        let main = image(main);
        add("lcd.png", lcd(&main));
        add("display.png", crop(&main, DISPLAY));
        add("small-display.png", crop(&main, SMALL_DISPLAY));
    }
    if let Some(eqmain) = skin.sheet(Sheet::EqMain) {
        add(
            "eq-slider.png",
            eq_slider(&padded(&image(eqmain), (275, 315))),
        );
    }
    if let Some(pledit) = skin.sheet(Sheet::PlEdit) {
        let pledit = image(pledit);
        add("titlebar.png", titlebar(&pledit));
        add("title.png", title(&pledit));
    }

    if skin.missing_art().is_empty() {
        player(&mut manifest, skin);
    }
    chrome(&mut manifest, skin.sheet(Sheet::Main).is_some());
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
        ("title_text", palette.title),
        ("button_text", palette.button_text),
        ("button_light", palette.button_hi),
        ("button_dark", palette.button_lo),
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
        ("eq", [s::EQ, s::EQ_PRESSED, s::EQ_LIT, s::EQ_LIT_PRESSED]),
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
    // Winamp redraws the whole volume bar in one of its shades, from
    // silent to full, each 15 pixels below the last.
    let _ = writeln!(
        manifest,
        "volume.track = {{ image = {}, slice = [2, 2], levels = {{ count = {}, step = 15 }} }}",
        reference(s::volume_frame(0)),
        sprites::SLIDER_FRAMES,
    );
    states(
        manifest,
        "volume.thumb",
        &[
            ("normal", s::VOLUME_THUMB),
            ("pressed", s::VOLUME_THUMB_PRESSED),
        ],
    );
    if skin.sheet(Sheet::EqMain).is_some() {
        equaliser(manifest);
    }
}

/// The equaliser's switches, sliders and thumb. Winamp redraws a slider
/// in a colour for its value, green at the bottom to red at the top;
/// the colours are stacked in `eq-slider.png`.
fn equaliser(manifest: &mut String) {
    use sprites as s;
    let states = |manifest: &mut String, name: &str, states: &[(&str, Sprite)]| {
        let list: Vec<String> = states
            .iter()
            .map(|(state, sprite)| format!("{state} = {}", reference(*sprite)))
            .collect();
        let _ = writeln!(manifest, "{name} = {{ {} }}", list.join(", "));
    };
    states(
        manifest,
        "equaliser.on",
        &[
            ("normal", s::EQ_ON),
            ("pressed", s::EQ_ON_PRESSED),
            ("on", s::EQ_ON_LIT),
            ("on_pressed", s::EQ_ON_LIT_PRESSED),
        ],
    );
    states(
        manifest,
        "equaliser.presets",
        &[
            ("normal", s::EQ_PRESETS),
            ("pressed", s::EQ_PRESETS_PRESSED),
        ],
    );
    states(
        manifest,
        "equaliser.thumb",
        &[("normal", s::EQ_THUMB), ("pressed", s::EQ_THUMB_PRESSED)],
    );
    let frame = s::eq_slider_frame(0);
    let _ = writeln!(
        manifest,
        "equaliser.slider = {{ image = {{ image = \"eq-slider.png\", rect = [0, 0, {w}, {h}] }}, slice = [0, 0], levels = {{ count = {}, step = {h} }} }}",
        sprites::SLIDER_FRAMES,
        w = frame.width,
        h = frame.height,
    );
}

/// The pieces of the equaliser's window used as they are.
const EQ_PIECES: [Sprite; 8] = [
    sprites::EQ_ON,
    sprites::EQ_ON_PRESSED,
    sprites::EQ_ON_LIT,
    sprites::EQ_ON_LIT_PRESSED,
    sprites::EQ_PRESETS,
    sprites::EQ_PRESETS_PRESSED,
    sprites::EQ_THUMB,
    sprites::EQ_THUMB_PRESSED,
];

/// `image` with only `pieces` left, the rest clear.
fn only(image: &RgbaImage, pieces: &[Sprite]) -> RgbaImage {
    let mut kept = RgbaImage::new(image.width(), image.height());
    for piece in pieces {
        let cut = crop(image, (piece.x, piece.y, piece.width, piece.height));
        image::imageops::replace(&mut kept, &cut, i64::from(piece.x), i64::from(piece.y));
    }
    kept
}

/// The equaliser's slider colours, one under the other from -12 dB to +12.
fn eq_slider(eqmain: &RgbaImage) -> RgbaImage {
    let first = sprites::eq_slider_frame(0);
    let mut stacked = RgbaImage::new(first.width, first.height * sprites::SLIDER_FRAMES);
    for n in 0..sprites::SLIDER_FRAMES {
        let frame = sprites::eq_slider_frame(n);
        let cut = crop(eqmain, (frame.x, frame.y, frame.width, frame.height));
        image::imageops::replace(&mut stacked, &cut, 0, i64::from(n * first.height));
    }
    stacked
}

/// Text fields in the song's display box.
fn chrome(manifest: &mut String, display: bool) {
    if display {
        let _ = writeln!(
            manifest,
            "\n[pieces.field]\nimage = \"display.png\"\nslice = [1, 1, 1, 1]\npadding = [4, 3, 4, 3]",
        );
    }
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
        Sheet::EqMain => "eqmain.png",
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

/// How close a pixel must be to the one beside it to be the same
/// background, and to the background's average along the piece's edge.
const STEP: u8 = 12;
const SPREAD: u8 = 60;
/// Background cut from a button only when the transport's buttons lose at
/// least this share each, as round buttons on a backdrop do; square
/// buttons that fill their cell lose almost none, or all of it when the
/// cell is one colour, and keep it all.
const SHAPED: f32 = 0.15;
/// The most of a piece that may be cut: more is the piece itself. A bar's
/// groove is thin, so most of a bar may go.
const MOST: f32 = 0.6;
const MOST_OF_A_BAR: f32 = 0.85;
/// An island on a piece's edge smaller than this share of it is a scrap
/// of background.
const SCRAP: f32 = 0.1;

/// Makes the backgrounds painted around a shaped skin's buttons, sliders
/// and thumbs clear, so they sit on Spotless's window as they sat on
/// Winamp's. A BMP has no transparency: the background is whatever joins
/// a piece's edge in smooth steps of colour, up to its outline.
fn cut_backgrounds(sheets: &mut [(Sheet, &str, RgbaImage)]) {
    use sprites as s;
    let Some((_, _, buttons)) = sheets.iter().find(|(sheet, ..)| *sheet == Sheet::CButtons) else {
        return;
    };
    let mut shares: Vec<f32> = [s::PREVIOUS, s::PLAY, s::PAUSE, s::STOP, s::NEXT]
        .into_iter()
        .map(|sprite| background(buttons, sprite).len() as f32 / area(sprite))
        .collect();
    shares.sort_by(f32::total_cmp);
    if !(SHAPED..=MOST).contains(&shares[shares.len() / 2]) {
        return;
    }
    let volume: Vec<Sprite> = (0..sprites::SLIDER_FRAMES).map(s::volume_frame).collect();
    let pieces: [(Sheet, &[Sprite], f32); 7] = [
        (
            Sheet::CButtons,
            &[
                s::PREVIOUS,
                s::PREVIOUS_PRESSED,
                s::PLAY,
                s::PLAY_PRESSED,
                s::PAUSE,
                s::PAUSE_PRESSED,
                s::STOP,
                s::STOP_PRESSED,
                s::NEXT,
                s::NEXT_PRESSED,
                s::EJECT,
                s::EJECT_PRESSED,
            ],
            MOST,
        ),
        (
            Sheet::ShufRep,
            &[
                s::SHUFFLE_OFF,
                s::SHUFFLE_OFF_PRESSED,
                s::SHUFFLE_ON,
                s::SHUFFLE_ON_PRESSED,
                s::REPEAT_OFF,
                s::REPEAT_OFF_PRESSED,
                s::REPEAT_ON,
                s::REPEAT_ON_PRESSED,
                s::PL,
                s::PL_PRESSED,
                s::PL_LIT,
                s::PL_LIT_PRESSED,
                s::EQ,
                s::EQ_PRESSED,
                s::EQ_LIT,
                s::EQ_LIT_PRESSED,
            ],
            MOST,
        ),
        (Sheet::EqMain, &EQ_PIECES, MOST),
        (
            Sheet::Volume,
            &[s::VOLUME_THUMB, s::VOLUME_THUMB_PRESSED],
            MOST,
        ),
        (Sheet::Volume, &volume, MOST_OF_A_BAR),
        (
            Sheet::PosBar,
            &[s::POSITION_THUMB, s::POSITION_THUMB_PRESSED],
            MOST,
        ),
        (Sheet::PosBar, &[s::POSITION_TRACK], MOST_OF_A_BAR),
    ];
    for (sheet, sprites, most) in pieces {
        let Some((_, _, image)) = sheets.iter_mut().find(|(s, ..)| *s == sheet) else {
            continue;
        };
        for &sprite in sprites {
            let cut = background(image, sprite);
            if cut.len() as f32 / area(sprite) <= most {
                for (x, y) in cut {
                    image.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                }
                clear_scraps(image, sprite);
            }
        }
    }
}

/// Islands left on a piece's edge once its background is cut, such as a
/// backdrop's highlight that wasn't a small enough step to follow: each
/// small one that touches the edge goes. The piece itself, the largest,
/// and what it encloses (a toggle's light) stay.
fn clear_scraps(image: &mut RgbaImage, sprite: Sprite) {
    let x1 = (sprite.x + sprite.width).min(image.width());
    let y1 = (sprite.y + sprite.height).min(image.height());
    let (x0, y0) = (sprite.x, sprite.y);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let width = (x1 - x0) as usize;
    let index = |x: u32, y: u32| (y - y0) as usize * width + (x - x0) as usize;
    let mut seen = vec![false; width * (y1 - y0) as usize];
    let mut islands = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            if seen[index(x, y)] || image.get_pixel(x, y).0[3] == 0 {
                continue;
            }
            seen[index(x, y)] = true;
            let (mut island, mut queue, mut on_edge) = (Vec::new(), vec![(x, y)], false);
            while let Some((x, y)) = queue.pop() {
                island.push((x, y));
                on_edge |= x == x0 || y == y0 || x == x1 - 1 || y == y1 - 1;
                let near = [
                    (x + 1, y),
                    (x.wrapping_sub(1), y),
                    (x, y + 1),
                    (x, y.wrapping_sub(1)),
                ];
                for (nx, ny) in near {
                    if (x0..x1).contains(&nx)
                        && (y0..y1).contains(&ny)
                        && !seen[index(nx, ny)]
                        && image.get_pixel(nx, ny).0[3] != 0
                    {
                        seen[index(nx, ny)] = true;
                        queue.push((nx, ny));
                    }
                }
            }
            islands.push((island, on_edge));
        }
    }
    let largest = islands
        .iter()
        .map(|(island, _)| island.len())
        .max()
        .unwrap_or(0);
    let small = area(sprite) * SCRAP;
    for (island, on_edge) in islands {
        if on_edge && island.len() < largest && (island.len() as f32) < small {
            for (x, y) in island {
                image.put_pixel(x, y, Rgba([0, 0, 0, 0]));
            }
        }
    }
}

fn area(sprite: Sprite) -> f32 {
    (sprite.width * sprite.height).max(1) as f32
}

/// The pixels of a piece's background: from its edge inwards, each a
/// small step of colour from the last and near the edge's average.
fn background(image: &RgbaImage, sprite: Sprite) -> Vec<(u32, u32)> {
    let (x0, y0) = (sprite.x, sprite.y);
    let x1 = (x0 + sprite.width).min(image.width());
    let y1 = (y0 + sprite.height).min(image.height());
    if x0 >= x1 || y0 >= y1 {
        return Vec::new();
    }
    let inside = |x: u32, y: u32| (x0..x1).contains(&x) && (y0..y1).contains(&y);
    let edge: Vec<(u32, u32)> = (x0..x1)
        .flat_map(|x| [(x, y0), (x, y1 - 1)])
        .chain((y0..y1).flat_map(|y| [(x0, y), (x1 - 1, y)]))
        .collect();
    let mut sums = [0u32; 3];
    for &(x, y) in &edge {
        for (sum, value) in sums.iter_mut().zip(image.get_pixel(x, y).0) {
            *sum += u32::from(value);
        }
    }
    let average = sums.map(|sum| (sum / edge.len() as u32) as u8);
    let far = |a: [u8; 3], b: [u8; 3]| {
        a.iter()
            .zip(b)
            .map(|(a, b)| a.abs_diff(b))
            .max()
            .unwrap_or(0)
    };
    let rgb = |x: u32, y: u32| {
        let [r, g, b, _] = image.get_pixel(x, y).0;
        [r, g, b]
    };
    let width = (x1 - x0) as usize;
    let index = |x: u32, y: u32| (y - y0) as usize * width + (x - x0) as usize;
    let mut seen = vec![false; width * (y1 - y0) as usize];
    let mut queue = std::collections::VecDeque::new();
    for (x, y) in edge {
        if !seen[index(x, y)] && far(rgb(x, y), average) <= SPREAD {
            seen[index(x, y)] = true;
            queue.push_back((x, y));
        }
    }
    let mut found = Vec::new();
    while let Some((x, y)) = queue.pop_front() {
        found.push((x, y));
        let near = [
            (x + 1, y),
            (x.wrapping_sub(1), y),
            (x, y + 1),
            (x, y.wrapping_sub(1)),
        ];
        for (nx, ny) in near {
            if inside(nx, ny)
                && !seen[index(nx, ny)]
                && far(rgb(nx, ny), rgb(x, y)) <= STEP
                && far(rgb(nx, ny), average) <= SPREAD
            {
                seen[index(nx, ny)] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    found
}

/// A sheet at least Winamp's size for it, clear where the skin's bitmap
/// is short, as Winamp drew what such a bitmap lacked.
fn padded(image: &RgbaImage, (width, height): (u32, u32)) -> RgbaImage {
    let (width, height) = (width.max(image.width()), height.max(image.height()));
    RgbaImage::from_fn(width, height, |x, y| pixel(image, x, y))
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
            // One row short, as some skins have it.
            ("posbar.bmp", bmp(307, 9)),
            ("volume.bmp", bmp(68, 433)),
            ("playpaus.bmp", bmp(42, 9)),
            ("numbers.bmp", bmp(99, 13)),
            ("text.bmp", bmp(155, 18)),
            ("pledit.bmp", bmp(280, 186)),
            ("eqmain.bmp", bmp(275, 315)),
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
                    | "volume.fill"
                    | "panel"
                    | "tab"
                    | "row_selected"
                    | "button"
                    | "small_button"
                    | "switch"
                    | "radio"
            );
            assert_eq!(skin.piece(name).is_some(), expected, "{name}");
        }
        // The volume in its shades: silent at the top, full 405 below.
        let volume = skin.piece("volume.track").unwrap();
        let dst = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(68.0, 13.0));
        let (_, quiet, _) = volume.patches_at(format::State::Normal, 0.0, dst, 1.0);
        let (_, loud, _) = volume.patches_at(format::State::Normal, 1.0, dst, 1.0);
        assert_eq!(loud[0].src.top() - quiet[0].src.top(), 405.0);
        // An equaliser slider in its colours: -12 dB at the top, +12 below.
        let slider = skin.piece("equaliser.slider").unwrap();
        let dst = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(14.0, 63.0));
        let (_, low, _) = slider.patches_at(format::State::Normal, 0.0, dst, 1.0);
        let (_, high, _) = slider.patches_at(format::State::Normal, 1.0, dst, 1.0);
        assert_eq!(high[0].src.top() - low[0].src.top(), 27.0 * 63.0);
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

    /// A button sheet whose transport cells hold a ring on a grey
    /// background, or fill their cell when `square`.
    fn buttons(square: bool) -> RgbaImage {
        RgbaImage::from_fn(136, 36, |x, y| {
            let (cx, cy) = (x % 23, y % 18);
            let (dx, dy) = (cx as f32 - 11.0, cy as f32 - 9.0);
            let inside = square || dx * dx + dy * dy <= 72.0;
            if inside {
                Rgba([250, 250, 250, 255])
            } else {
                // A gentle gradient, as backdrops are.
                Rgba([140 + cy as u8, 145 + cy as u8, 160, 255])
            }
        })
    }

    #[test]
    fn round_buttons_lose_their_background_and_square_ones_keep_it() {
        for square in [false, true] {
            let mut sheets = vec![(Sheet::CButtons, "cbuttons.png", buttons(square))];
            cut_backgrounds(&mut sheets);
            // Of the equaliser's window, only its switches and thumb are used.
            for (sheet, _, image) in &mut sheets {
                if *sheet == Sheet::EqMain {
                    *image = only(image, &EQ_PIECES);
                }
            }
            let image = &sheets[0].2;
            let corner = image.get_pixel(sprites::PLAY.x, sprites::PLAY.y).0[3];
            let middle = image.get_pixel(sprites::PLAY.x + 11, sprites::PLAY.y + 9).0[3];
            assert_eq!(corner, if square { 255 } else { 0 }, "square: {square}");
            assert_eq!(middle, 255);
        }
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
