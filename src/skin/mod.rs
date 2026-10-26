//! Classic Winamp skins, read to be converted to Spotless's own format
//! (`format::convert`): that is how Winamp Classic is made from Winamp's
//! base skin. Spotless doesn't load them itself.
//!
//! A `.wsz` file is a zip of bitmaps and two small text files. Every skin
//! keeps its pieces at the same places ([`sprites`] says where); its
//! colours are guessed from them: the playlist's from `pledit.txt`, the
//! display's from the digits and the scrolling text, the window's from the
//! main window's body. Modern `.wal` skins are XML and scripts, and not
//! read.

pub mod font;
pub mod format;
pub mod sprites;
pub(crate) mod zip;

use std::collections::HashMap;
use std::path::Path;

use egui::Color32;
use thiserror::Error;

pub use sprites::{Sheet, Sprite};

use crate::ui::theme::Palette;

/// The playlist's colours.
const PLEDIT_TXT: &str = "pledit.txt";
/// The sheets the player strip is drawn from; it also needs one of the
/// two digit sheets.
const PLAYER_SHEETS: [Sheet; 6] = [
    Sheet::CButtons,
    Sheet::ShufRep,
    Sheet::PosBar,
    Sheet::Volume,
    Sheet::PlayPaus,
    Sheet::Text,
];

#[derive(Debug, Error)]
pub enum SkinError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a zip archive; a classic skin is a .wsz file or a folder of bitmaps")]
    NotAnArchive,
    #[error("{0}")]
    Archive(zip::ZipError),
    #[error("this is a modern Winamp skin; Spotless needs a classic one")]
    ModernSkin,
    #[error("no skin files were found inside")]
    Empty,
}

/// A skin read: its colours and its bitmaps.
pub struct Skin {
    pub palette: Palette,
    sheets: HashMap<Sheet, Bitmap>,
}

impl Skin {
    /// The skin at `path`, an archive or a folder.
    pub fn load(path: &Path) -> Result<Self, SkinError> {
        let files = if path.is_dir() {
            read_dir(path)?
        } else {
            read_archive(&std::fs::read(path)?)?
        };
        if files.is_empty() {
            return Err(SkinError::Empty);
        }
        Ok(Self::from_files(&files))
    }

    fn from_files(files: &Files) -> Self {
        let sheets: HashMap<Sheet, Bitmap> = Sheet::ALL
            .into_iter()
            .filter_map(|sheet| {
                let bitmap = Bitmap::decode(files.get(sheet.file_name())?)?;
                Some((sheet, bitmap))
            })
            .collect();
        let list = files
            .get(PLEDIT_TXT)
            .map_or(PlaylistColors::DEFAULT, |text| {
                PlaylistColors::parse(&String::from_utf8_lossy(text))
            });
        Self {
            palette: palette(&sheets, list),
            sheets,
        }
    }

    pub fn sheet(&self, sheet: Sheet) -> Option<&Bitmap> {
        self.sheets.get(&sheet)
    }

    /// The sheet the time's digits come from: the extended one when the
    /// skin has it, as in Winamp.
    pub fn digits(&self) -> Option<Sheet> {
        [Sheet::NumsEx, Sheet::Numbers]
            .into_iter()
            .find(|&sheet| self.sheets.contains_key(&sheet))
    }

    /// The files the player strip needs that the skin lacks. Without
    /// them, the skin only lends its colours.
    pub fn missing_art(&self) -> Vec<&'static str> {
        let mut missing: Vec<&str> = PLAYER_SHEETS
            .into_iter()
            .filter(|sheet| !self.sheets.contains_key(sheet))
            .map(Sheet::file_name)
            .collect();
        if self.digits().is_none() {
            missing.push(Sheet::Numbers.file_name());
        }
        missing
    }
}

type Files = HashMap<String, Vec<u8>>;

/// Whether a file in a skin is one Spotless reads.
fn wanted(name: &str) -> bool {
    name == PLEDIT_TXT || Sheet::ALL.iter().any(|sheet| sheet.file_name() == name)
}

fn read_archive(bytes: &[u8]) -> Result<Files, SkinError> {
    let archive = zip::Archive::parse(bytes).map_err(|error| match error {
        zip::ZipError::NotAnArchive => SkinError::NotAnArchive,
        error => SkinError::Archive(error),
    })?;
    let mut files = Files::new();
    for entry in archive.entries().iter().filter(|e| !e.is_dir()) {
        let name = entry.file_name();
        if name == "skin.xml" {
            return Err(SkinError::ModernSkin);
        }
        // Skins nest their files in a folder now and then; the first of a
        // name wins.
        if wanted(&name) && !files.contains_key(&name) {
            files.insert(name, archive.read(entry).map_err(SkinError::Archive)?);
        }
    }
    Ok(files)
}

fn read_dir(dir: &Path) -> Result<Files, SkinError> {
    let mut files = Files::new();
    for entry in std::fs::read_dir(dir)?.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name == "skin.xml" {
            return Err(SkinError::ModernSkin);
        }
        if wanted(&name) {
            files.insert(name, std::fs::read(entry.path())?);
        }
    }
    Ok(files)
}

/// The colours of `pledit.txt`'s `[Text]` section, the playlist's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaylistColors {
    normal: Color32,
    current: Color32,
    background: Color32,
    selected: Color32,
}

impl PlaylistColors {
    /// What Winamp used when a skin had no `pledit.txt`.
    const DEFAULT: Self = Self {
        normal: Color32::from_rgb(0x00, 0xff, 0x00),
        current: Color32::WHITE,
        background: Color32::BLACK,
        selected: Color32::from_rgb(0x00, 0x00, 0xff),
    };

    /// An INI file; anything unreadable keeps its default, as in Winamp.
    fn parse(text: &str) -> Self {
        let mut colors = Self::DEFAULT;
        let mut in_text = false;
        for line in text.lines().map(str::trim) {
            if let Some(section) = line.strip_prefix('[') {
                in_text = section
                    .strip_suffix(']')
                    .unwrap_or(section)
                    .trim()
                    .eq_ignore_ascii_case("text");
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| in_text) else {
                continue;
            };
            let slot = match key.trim().to_ascii_lowercase().as_str() {
                "normal" => &mut colors.normal,
                "current" => &mut colors.current,
                "normalbg" => &mut colors.background,
                "selectedbg" => &mut colors.selected,
                _ => continue,
            };
            *slot = hex(value).unwrap_or(*slot);
        }
        colors
    }
}

/// A `#RRGGBB` colour. Winamp took it without the hash and with extra
/// digits after the six. This reads bytes, so a multi-byte character is
/// no colour rather than a panic.
fn hex(value: &str) -> Option<Color32> {
    let digits = value.trim().trim_start_matches('#').as_bytes();
    let nibble = |at: usize| digits.get(at).and_then(|byte| (*byte as char).to_digit(16));
    let channel = |at: usize| Some((nibble(at)? << 4 | nibble(at + 1)?) as u8);
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
}

/// A decoded bitmap: RGBA, rows top to bottom.
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Bitmap {
    /// A BMP or PNG, whatever the file is called: skins mislabel them.
    fn decode(bytes: &[u8]) -> Option<Self> {
        let image = image::load_from_memory(bytes).ok()?.into_rgba8();
        Some(Self {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        })
    }

    /// The opaque pixels of a `(x, y, width, height)` area, clipped to the
    /// bitmap.
    fn pixels(&self, (x, y, w, h): (u32, u32, u32, u32)) -> impl Iterator<Item = Color32> + '_ {
        let rows = y.min(self.height)..(y + h).min(self.height);
        rows.flat_map(move |row| {
            (x.min(self.width)..(x + w).min(self.width)).filter_map(move |column| {
                let at = ((row * self.width + column) * 4) as usize;
                let [r, g, b, a] = self.rgba[at..at + 4] else {
                    return None;
                };
                (a > 0).then(|| Color32::from_rgb(r, g, b))
            })
        })
    }
}

/// Colours counted in buckets of near-identical ones, so a gradient
/// still has a most common colour.
#[derive(Default)]
struct Histogram {
    buckets: HashMap<(u8, u8, u8), (u32, [u32; 3])>,
}

impl Histogram {
    fn of(colors: impl Iterator<Item = Color32>) -> Self {
        let mut histogram = Self::default();
        for c in colors {
            let (count, sums) = histogram
                .buckets
                .entry((c.r() >> 3, c.g() >> 3, c.b() >> 3))
                .or_default();
            *count += 1;
            for (sum, value) in sums.iter_mut().zip([c.r(), c.g(), c.b()]) {
                *sum += u32::from(value);
            }
        }
        histogram
    }

    /// The most common colour among those `keep` accepts: its bucket's
    /// average.
    fn most_common(&self, keep: impl Fn(Color32) -> bool) -> Option<Color32> {
        self.buckets
            .values()
            .map(|&(count, [r, g, b])| {
                let color =
                    Color32::from_rgb((r / count) as u8, (g / count) as u8, (b / count) as u8);
                (count, color)
            })
            .filter(|&(_, color)| keep(color))
            .max_by_key(|&(count, color)| (count, color.r(), color.g(), color.b()))
            .map(|(_, color)| color)
    }
}

/// A bitmap's background and the colour drawn on it: the most common
/// colour, and the most common one that stands out from it.
fn ink(bitmap: &Bitmap, area: (u32, u32, u32, u32)) -> Option<(Color32, Color32)> {
    let histogram = Histogram::of(bitmap.pixels(area));
    let background = histogram.most_common(|_| true)?;
    let ink = histogram.most_common(|c| contrast(c, background) >= 3.0)?;
    Some((background, ink))
}

/// The pieces of `pledit.bmp` that are plain frame: the left and right
/// edges and the bottom's filler.
const FRAME_TILES: [(u32, u32, u32, u32); 3] =
    [(0, 42, 12, 29), (31, 42, 20, 29), (179, 0, 25, 38)];
/// The playlist scrollbar's thumb in `pledit.bmp`.
const THUMB_AREA: (u32, u32, u32, u32) = (52, 53, 8, 18);
/// The scrolling text's alphabet in `text.bmp`; some skins add more below.
const TEXT_AREA: (u32, u32, u32, u32) = (0, 0, 155, 18);
const EVERYTHING: (u32, u32, u32, u32) = (0, 0, u32::MAX, u32::MAX);
/// Plain body in every main window: the band between the seek bar and
/// the buttons.
const BODY_AREA: (u32, u32, u32, u32) = (16, 83, 248, 5);
/// Inside the Stop button, clear of its edges: mostly its face.
const BUTTON_FACE: (u32, u32, u32, u32) = (71, 2, 19, 14);
/// The plain middle of the playlist editor's title piece, the name on it.
const TITLE_AREA: (u32, u32, u32, u32) = (31, 0, 90, 14);

fn palette(sheets: &HashMap<Sheet, Bitmap>, list: PlaylistColors) -> Palette {
    let classic = Palette::CLASSIC;
    let bitmap = |sheet: Sheet| sheets.get(&sheet);

    // The main window's own body, else the playlist editor's edges.
    let frame = bitmap(Sheet::Main)
        .and_then(|b| Histogram::of(b.pixels(BODY_AREA)).most_common(|_| true))
        .or_else(|| {
            bitmap(Sheet::PlEdit).and_then(|b| {
                Histogram::of(FRAME_TILES.iter().flat_map(|&t| b.pixels(t))).most_common(|_| true)
            })
        })
        .unwrap_or(classic.frame);
    let text = bitmap(Sheet::Text).and_then(|b| ink(b, TEXT_AREA));
    let digits = bitmap(Sheet::NumsEx)
        .or_else(|| bitmap(Sheet::Numbers))
        .and_then(|b| ink(b, EVERYTHING));
    let lcd_bg = text
        .or(digits)
        .map_or(classic.lcd_bg, |(background, _)| background);
    let lcd = [digits, text]
        .into_iter()
        .flatten()
        .map(|(_, ink)| ink)
        .chain([list.normal])
        .find(|&ink| contrast(ink, lcd_bg) >= 3.0)
        .unwrap_or(classic.lcd);

    let thumb = bitmap(Sheet::PlEdit)
        .and_then(|b| Histogram::of(b.pixels(THUMB_AREA)).most_common(|_| true))
        .unwrap_or(classic.thumb);

    let hi = mix(frame, Color32::WHITE, 0.2);
    let chrome = text_on(frame);
    // Text buttons in the colours of the skin's own buttons.
    let face = bitmap(Sheet::CButtons)
        .and_then(|b| Histogram::of(b.pixels(BUTTON_FACE)).most_common(|_| true));
    let (button, button_text, button_hi, button_lo) = match face {
        Some(face) => (
            face,
            text_on(face),
            mix(face, Color32::WHITE, 0.55),
            mix(face, Color32::BLACK, 0.6),
        ),
        None => (
            mix(frame, hi, 0.35),
            chrome,
            hi,
            mix(frame, Color32::BLACK, 0.6),
        ),
    };
    let title = bitmap(Sheet::PlEdit)
        .and_then(|b| Histogram::of(b.pixels(TITLE_AREA)).most_common(|_| true))
        .map_or(chrome, text_on);
    Palette {
        desk: mix(frame, Color32::BLACK, 0.75),
        frame,
        hi,
        lo: mix(frame, Color32::BLACK, 0.6),
        chrome,
        button,
        lcd_bg,
        lcd,
        list_bg: list.background,
        list_fg: list.normal,
        current: list.current,
        selected_bg: list.selected,
        dim: mix(list.normal, list.background, 0.35),
        accent: lcd,
        thumb,
        title,
        button_text,
        button_hi,
        button_lo,
    }
}

/// Text that reads on `background`: a light or dark shade of it.
fn text_on(background: Color32) -> Color32 {
    if luminance(background) < 0.3 {
        mix(background, Color32::WHITE, 0.75)
    } else {
        mix(background, Color32::BLACK, 0.8)
    }
}

/// `t` of the way from `a` to `b`.
pub(crate) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgb(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
    )
}

/// WCAG's relative luminance, 0 for black to 1 for white.
pub(crate) fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// WCAG's contrast ratio, 1 to 21.
pub(crate) fn contrast(a: Color32, b: Color32) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bitmap as a PNG, as some skins ship them.
    fn png(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let image = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(paint(x, y)));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn pledit_colours_are_read_forgivingly() {
        let colors = PlaylistColors::parse(
            "[Text]\nNormal=#00FF00\nCurrent=FFFFFF00\nNormalBG=#nope\nSelectedBG=#0000C6\nFont=Arial\n",
        );
        assert_eq!(colors.normal, Color32::from_rgb(0, 255, 0));
        assert_eq!(colors.current, Color32::WHITE);
        assert_eq!(colors.background, PlaylistColors::DEFAULT.background);
        assert_eq!(colors.selected, Color32::from_rgb(0, 0, 0xc6));
        assert_eq!(hex("#ÿÿÿÿÿÿ"), None);
    }

    #[test]
    fn keys_outside_the_text_section_are_ignored() {
        let colors = PlaylistColors::parse("[Other]\nNormal=#123456\n");
        assert_eq!(colors, PlaylistColors::DEFAULT);
    }

    #[test]
    fn a_skin_gives_its_colours_and_keeps_classic_for_the_rest() {
        // Digits: amber on dark blue, with dim unlit segments.
        let numbers = png(99, 13, |x, y| match (x % 9, y % 4) {
            (0..=2, 0) => [0xff, 0xb0, 0x00],
            (3, _) => [0x10, 0x10, 0x30],
            _ => [0x00, 0x00, 0x40],
        });
        let pledit = png(280, 186, |_, _| [0x60, 0x20, 0x20]);
        let archive = zip::write(&[
            (
                "Skin/PLEDIT.TXT",
                b"[Text]\nNormal=#C0C0C0\nCurrent=#FFFF00\nNormalBG=#101010\nSelectedBG=#404080\n",
                false,
            ),
            ("Skin/numbers.bmp", &numbers, true),
            ("Skin/PLEDIT.BMP", &pledit, true),
        ]);
        let palette = Skin::from_files(&read_archive(&archive).unwrap()).palette;
        assert_eq!(palette.list_fg, Color32::from_rgb(0xc0, 0xc0, 0xc0));
        assert_eq!(palette.current, Color32::from_rgb(0xff, 0xff, 0));
        assert_eq!(palette.list_bg, Color32::from_rgb(0x10, 0x10, 0x10));
        assert_eq!(palette.frame, Color32::from_rgb(0x60, 0x20, 0x20));
        assert_eq!(palette.lcd_bg, Color32::from_rgb(0, 0, 0x40));
        assert_eq!(palette.lcd, Color32::from_rgb(0xff, 0xb0, 0));
        assert_eq!(palette.accent, palette.lcd);
    }

    #[test]
    fn digits_too_faint_to_read_give_way_to_the_playlist_colour() {
        let numbers = png(
            99,
            13,
            |x, _| if x % 2 == 0 { [0, 0, 0] } else { [8, 8, 8] },
        );
        let archive = zip::write(&[("numbers.bmp", &numbers, false)]);
        let palette = Skin::from_files(&read_archive(&archive).unwrap()).palette;
        assert_eq!(palette.lcd, PlaylistColors::DEFAULT.normal);
    }

    #[test]
    fn modern_skins_and_other_files_are_refused() {
        let modern = zip::write(&[("skin.xml", b"<WinampAbstractionLayer/>", false)]);
        assert!(matches!(read_archive(&modern), Err(SkinError::ModernSkin)));
        assert!(matches!(
            read_archive(b"BM a bitmap"),
            Err(SkinError::NotAnArchive)
        ));
    }
}
