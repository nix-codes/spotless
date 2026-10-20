use egui::{Color32, Rect, pos2, vec2};

use super::*;
use crate::skin::zip;

/// A PNG of `width` by `height` in one colour.
fn png(width: u32, height: u32) -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    bytes.into_inner()
}

fn skin(manifest: &str, files: &[(&str, &[u8])]) -> Result<Skin, LoadError> {
    let mut all = vec![(MANIFEST, manifest.as_bytes())];
    all.extend_from_slice(files);
    Skin::built_in("Fallback", &all)
}

fn problems(skin: &Skin) -> Vec<&str> {
    skin.problems.iter().map(|p| p.at.as_str()).collect()
}

#[test]
fn a_skin_of_colours_alone_changes_only_those() {
    let skin = skin(
        "format = 1\nname = \"Night\"\n[colors]\nlcd_text = \"#ff8800\"\nwindow = \"#101010\"\n",
        &[],
    )
    .unwrap();
    assert_eq!(skin.name, "Night");
    assert!(skin.problems.is_empty(), "{:?}", skin.problems);
    assert_eq!(skin.palette.lcd, Color32::from_rgb(0xff, 0x88, 0x00));
    assert_eq!(skin.palette.frame, Color32::from_rgb(0x10, 0x10, 0x10));
    assert_eq!(skin.palette.list_bg, Palette::CLASSIC.list_bg);
    assert_eq!((skin.scale, skin.filter), (1.0, Filter::Pixel));
    assert!(skin.piece("play").is_none());
}

#[test]
fn only_the_format_can_stop_a_skin() {
    assert!(matches!(
        skin("name = \"x\"", &[]),
        Err(LoadError::NoFormat)
    ));
    assert!(matches!(
        skin("format = 2", &[]),
        Err(LoadError::NewerFormat(2))
    ));
    assert!(matches!(
        skin("format = 1\n[colors", &[]),
        Err(LoadError::Manifest(_))
    ));
    assert!(matches!(
        Skin::built_in("x", &[("other.toml", b"format = 1")]),
        Err(LoadError::NoManifest)
    ));
    let named = skin("format = 1", &[]).unwrap();
    assert_eq!(named.name, "Fallback");
}

#[test]
fn unknown_keys_and_bad_values_are_problems_by_name() {
    let skin = skin(
        "format = 1\nscale = -2\nfuture = true\n[colors]\nlcd_text = \"green\"\nsparkle = \"#ffffff\"\n[fonts.huge]\n[pieces.wobble]\nimage = \"a.png\"\n",
        &[],
    )
    .unwrap();
    assert_eq!(
        problems(&skin),
        [
            "future",
            "scale",
            "colors.lcd_text",
            "colors.sparkle",
            "fonts.huge",
            "pieces.wobble",
        ]
    );
    assert_eq!(skin.scale, 1.0);
}

#[test]
fn sprites_take_their_states_from_a_sheet() {
    let sheet = png(46, 36);
    let skin = skin(
        r#"
        format = 1
        scale = 1.5
        [pieces.play]
        normal = { image = "player.png", rect = [23, 0, 23, 18] }
        pressed = { image = "player.png", rect = [23, 18, 23, 18] }
        "#,
        &[("player.png", &sheet)],
    )
    .unwrap();
    assert!(skin.problems.is_empty(), "{:?}", skin.problems);
    let play = skin.piece("play").unwrap();
    assert_eq!(play.size(skin.scale), vec2(34.5, 27.0));
    let dst = Rect::from_min_size(pos2(0.0, 0.0), play.size(skin.scale));
    let (image, patches, dim) = play.patches(State::Pressed, dst, skin.scale);
    assert_eq!(image, "player.png");
    assert_eq!(patches[0].src.min, pos2(23.0, 18.0));
    assert!(!dim);
    // On falls back to Pressed, Disabled to Normal dimmed.
    assert_eq!(play.states.get(State::OnHover).0.area.y, 18);
    let (disabled, dim) = play.states.get(State::Disabled);
    assert_eq!((disabled.area.y, dim), (0, true));
    assert!(skin.images.contains_key("player.png"));
}

#[test]
fn a_whole_file_is_one_piece() {
    let lcd = png(20, 16);
    let skin = skin(
        "format = 1\n[pieces.lcd]\nimage = \"lcd.png\"\nslice = [3, 3, 3, 3]\ncenter = \"none\"\npadding = [5, 4, 5, 4]\n",
        &[("lcd.png", &lcd)],
    )
    .unwrap();
    assert!(skin.problems.is_empty(), "{:?}", skin.problems);
    let lcd = skin.piece("lcd").unwrap();
    assert_eq!(
        lcd.shape,
        Shape::Frame {
            slice: [3, 3, 3, 3],
            edges: Fill::Stretch,
            center: None,
            padding: [5, 4, 5, 4],
        }
    );
    let dst = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 60.0));
    assert_eq!(
        lcd.content(dst, 2.0),
        Rect::from_min_max(pos2(10.0, 8.0), pos2(190.0, 52.0))
    );
    // Eight patches: no centre.
    assert_eq!(lcd.patches(State::Normal, dst, 2.0).1.len(), 8);
}

#[test]
fn a_piece_that_doesnt_fit_its_image_is_left_to_classic() {
    let sheet = png(30, 30);
    let skin = skin(
        r#"
        format = 1
        [pieces.play]
        image = { image = "sheet.png", rect = [20, 20, 23, 18] }
        [pieces.panel]
        image = "sheet.png"
        slice = [20, 2, 20, 2]
        [pieces.seek.track]
        image = "sheet.png"
        [pieces.stop]
        image = "missing.png"
        [pieces.status.playing]
        image = "sheet.png"
        [pieces.button]
        image = "sheet.png"
        slice = [2, 2, 2, 2]
        edges = "wobble"
        "#,
        &[("sheet.png", &sheet)],
    )
    .unwrap();
    assert_eq!(
        problems(&skin),
        [
            "pieces.play.image",
            "pieces.panel",
            "pieces.seek.track",
            "pieces.stop.image",
            "pieces.button",
        ]
    );
    for name in ["play", "panel", "seek.track", "stop", "button"] {
        assert!(skin.piece(name).is_none(), "{name}");
    }
    assert!(skin.piece("status.playing").is_some());
}

#[test]
fn some_pieces_stand_in_for_others() {
    let sheet = png(8, 8);
    let skin = skin(
        "format = 1\n[pieces.titlebar]\nimage = \"t.png\"\nslice = [2, 0, 2, 0]\nedges = \"tile\"\n[pieces.title]\nimage = \"t.png\"\nslice = [2, 2]\ntitle_gap = 6\n",
        &[("t.png", &sheet)],
    )
    .unwrap();
    assert!(skin.problems.is_empty(), "{:?}", skin.problems);
    assert_eq!(skin.piece("separator"), skin.piece("titlebar"));
    assert_eq!(skin.piece("title").unwrap().title_gap, 6);
    assert!(skin.piece("small_button").is_none());
}

#[test]
fn bars_know_their_direction_from_their_name() {
    let sheet = png(8, 18);
    let skin = skin(
        "format = 1\n[pieces.scrollbar.thumb]\nimage = \"s.png\"\nslice = [3, 3]\nmiddle = \"tile\"\n",
        &[("s.png", &sheet)],
    )
    .unwrap();
    let thumb = skin.piece("scrollbar.thumb").unwrap();
    assert_eq!(
        thumb.shape,
        Shape::Bar {
            vertical: true,
            ends: [3, 3],
            middle: Fill::Tile,
        }
    );
}

#[test]
fn bitmap_fonts_are_cut_from_their_grid() {
    let font = png(140, 13);
    let skin = skin(
        r#"
        format = 1
        [fonts.display]
        bitmap = { image = "font.png", cell = [5, 6], rows = ["ABC", "012"], origin = [10, 1] }
        tint = true
        [fonts.digits]
        bitmap = { image = "font.png", cell = [9, 13], rows = ["0123456789 -", "x"] }
        "#,
        &[("font.png", &font)],
    )
    .unwrap();
    // Two rows of 13 pixels don't fit in 13.
    assert_eq!(problems(&skin), ["fonts.digits.bitmap"]);
    let display = &skin.fonts[&Role::Display];
    assert_eq!(
        display.glyph('2'),
        Some(Area {
            x: 20,
            y: 7,
            width: 5,
            height: 6,
        })
    );
    // Lower case borrows upper case; what the font lacks is left out.
    assert_eq!(display.glyph('b').map(|a| a.x), Some(15));
    assert_eq!(display.glyph('é'), None);
    assert!(matches!(display, Font::Bitmap { tint: true, .. }));
}

#[test]
fn font_files_must_be_fonts() {
    let skin = skin(
        "format = 1\n[fonts.list]\nfile = \"a.ttf\"\nsize = 13\n[fonts.chrome]\nfile = \"b.ttf\"\n",
        &[("a.ttf", b"\0\x01\0\0rest"), ("b.ttf", b"<html>")],
    )
    .unwrap();
    assert_eq!(problems(&skin), ["fonts.chrome"]);
    assert_eq!(
        skin.fonts[&Role::List],
        Font::File {
            bytes: b"\0\x01\0\0rest".to_vec(),
            size: 13.0,
        }
    );
}

#[test]
fn a_skin_loads_from_a_folder_or_a_zip() {
    let dir = std::env::temp_dir().join(format!("spotless-skin-{}", fastrand::u64(..)));
    std::fs::create_dir_all(dir.join("img")).unwrap();
    std::fs::write(
        dir.join(MANIFEST),
        "format = 1\n[pieces.eject]\nimage = \"img/eject.png\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("img/eject.png"), png(22, 16)).unwrap();
    let folder = Skin::load(&dir).unwrap();
    assert!(folder.problems.is_empty(), "{:?}", folder.problems);
    assert_eq!(folder.name, dir.file_name().unwrap().to_string_lossy());
    assert!(folder.piece("eject").is_some());

    let eject = png(22, 16);
    let zipped = zip::write(&[
        (
            "chip/skin.toml",
            b"format = 1\nname = \"Chip\"\n[pieces.eject]\nimage = \"img/eject.png\"\n",
            true,
        ),
        ("chip/img/eject.png", &eject, false),
    ]);
    let path = dir.with_extension("zip");
    std::fs::write(&path, zipped).unwrap();
    let archive = Skin::load(&path).unwrap();
    assert_eq!(archive.name, "Chip");
    assert!(archive.piece("eject").is_some());

    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_file(&path).unwrap();
}
