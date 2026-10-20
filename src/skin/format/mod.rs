//! Spotless's own skins: a `skin.toml` manifest and PNG images, in a
//! folder or a zip of one.
//!
//! A skin gives pieces that know how to stretch, and Spotless keeps the
//! layout: fixed sprites (buttons, icons, thumbs), bars with fixed ends
//! (seek, volume, scrollbars), nine-slice frames for whatever takes the
//! window's size, and fonts for four text roles. Every piece is optional;
//! one a skin leaves out, or gets wrong, is drawn from its colours as
//! Classic draws it. What went wrong is kept as problems, by name, to show
//! in Settings.

mod geometry;
mod manifest;
mod source;

use std::collections::HashMap;
use std::path::Path;

use egui::{Rect, Vec2, pos2, vec2};
use thiserror::Error;

pub use geometry::{Fill, Patch};

use super::Bitmap;
use crate::ui::theme::Palette;

/// The manifest's name inside a skin.
pub const MANIFEST: &str = "skin.toml";
/// The format this Spotless reads.
pub const FORMAT: i64 = 1;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("not a zip archive; a skin is a folder, or a .zip of one")]
    NotAnArchive,
    #[error("the archive can't be read: {0}")]
    Archive(String),
    #[error("there is no skin.toml inside")]
    NoManifest,
    #[error("skin.toml can't be read: {0}")]
    Manifest(String),
    #[error("skin.toml needs format = 1 at the top")]
    NoFormat,
    #[error("made for a newer Spotless (format {0}); this one reads format 1")]
    NewerFormat(i64),
    #[error("the skin is bigger than {} MB", source::MAX_BYTES / 1024 / 1024)]
    TooBig,
}

/// Something wrong with one part of a skin, which is drawn as Classic
/// instead (or, for a warning, is only ignored).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// Where, as the manifest names it: `pieces.play`, `fonts.list`.
    pub at: String,
    pub message: String,
}

/// How the images are scaled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Pixel art: enlarged unsmoothed, then scaled smoothly.
    Pixel,
    /// Painted art, scaled smoothly.
    Smooth,
}

/// A rectangle of an image, in its pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Area {
    pub fn rect(self) -> Rect {
        Rect::from_min_size(
            pos2(self.x as f32, self.y as f32),
            vec2(self.width as f32, self.height as f32),
        )
    }

    pub fn size(self) -> Vec2 {
        vec2(self.width as f32, self.height as f32)
    }
}

/// Part of one of the skin's images.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRef {
    /// The image's path inside the skin.
    pub image: String,
    pub area: Area,
}

/// What a control is doing, which picks the image drawn for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Normal,
    Hover,
    Pressed,
    /// Can't be used; drawn from Normal at half opacity when not given.
    Disabled,
    /// A toggle that is on.
    On,
    OnHover,
    OnPressed,
    /// A title bar in a window that isn't focused.
    Unlit,
}

impl State {
    pub const ALL: [State; 8] = [
        State::Normal,
        State::Hover,
        State::Pressed,
        State::Disabled,
        State::On,
        State::OnHover,
        State::OnPressed,
        State::Unlit,
    ];

    pub fn key(self) -> &'static str {
        match self {
            State::Normal => "normal",
            State::Hover => "hover",
            State::Pressed => "pressed",
            State::Disabled => "disabled",
            State::On => "on",
            State::OnHover => "on_hover",
            State::OnPressed => "on_pressed",
            State::Unlit => "unlit",
        }
    }

    /// The state drawn in this one's place when a skin doesn't give it.
    fn fallback(self) -> State {
        match self {
            State::OnHover | State::OnPressed => State::On,
            State::On => State::Pressed,
            _ => State::Normal,
        }
    }
}

/// A piece's images, by state. Normal is always there.
#[derive(Clone, Debug, PartialEq)]
pub struct States {
    images: HashMap<State, ImageRef>,
}

impl States {
    /// The image for `state`, and whether to dim it: a Disabled the skin
    /// lacks is its Normal, dimmed.
    pub fn get(&self, state: State) -> (&ImageRef, bool) {
        let mut at = state;
        loop {
            if let Some(image) = self.images.get(&at) {
                return (image, state == State::Disabled && at != State::Disabled);
            }
            at = at.fallback();
        }
    }

    fn normal(&self) -> &ImageRef {
        &self.images[&State::Normal]
    }
}

/// How a piece stretches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    /// Drawn at its own size.
    Sprite,
    /// Fixed ends along its length, the middle filled.
    Bar {
        vertical: bool,
        ends: [u32; 2],
        middle: Fill,
    },
    /// Fixed corners, edges along their length, a centre or none; the
    /// content goes `padding` in from the sides.
    Frame {
        slice: [u32; 4],
        edges: Fill,
        center: Option<Fill>,
        padding: [u32; 4],
    },
}

/// The kinds of piece, for the names Spotless looks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sprite,
    Bar { vertical: bool },
    Frame,
}

/// Every piece Spotless draws from a skin, and its kind.
pub const PIECES: &[(&str, Kind)] = &[
    ("window", Kind::Frame),
    ("titlebar", Kind::Frame),
    ("separator", Kind::Frame),
    ("statusbar", Kind::Frame),
    ("lcd", Kind::Frame),
    ("display", Kind::Frame),
    ("small_display", Kind::Frame),
    ("status.playing", Kind::Sprite),
    ("status.paused", Kind::Sprite),
    ("status.stopped", Kind::Sprite),
    ("previous", Kind::Sprite),
    ("play", Kind::Sprite),
    ("pause", Kind::Sprite),
    ("stop", Kind::Sprite),
    ("next", Kind::Sprite),
    ("eject", Kind::Sprite),
    ("shuffle", Kind::Sprite),
    ("repeat", Kind::Sprite),
    ("pl", Kind::Sprite),
    ("seek.track", Kind::Bar { vertical: false }),
    ("seek.fill", Kind::Bar { vertical: false }),
    ("seek.thumb", Kind::Sprite),
    ("volume.track", Kind::Bar { vertical: false }),
    ("volume.fill", Kind::Bar { vertical: false }),
    ("volume.thumb", Kind::Sprite),
    ("panel", Kind::Frame),
    ("tab", Kind::Frame),
    ("row_selected", Kind::Frame),
    ("scrollbar.track", Kind::Bar { vertical: true }),
    ("scrollbar.thumb", Kind::Bar { vertical: true }),
    ("button", Kind::Frame),
    ("small_button", Kind::Frame),
    ("field", Kind::Frame),
    ("switch", Kind::Sprite),
    ("radio", Kind::Sprite),
];

/// Pieces that take another's place when a skin leaves them out.
const STAND_INS: [(&str, &str); 2] = [("separator", "titlebar"), ("small_button", "button")];

/// One piece of a skin.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub states: States,
    pub shape: Shape,
    /// For a title bar: plain space either side of the name, in image
    /// pixels.
    pub title_gap: u32,
}

impl Piece {
    /// Its size on screen when drawn at its own size.
    pub fn size(&self, scale: f32) -> Vec2 {
        self.states.normal().area.size() * scale
    }

    /// What to draw for `state` over `dst`: the image, the patches, and
    /// whether to dim them.
    pub fn patches(&self, state: State, dst: Rect, scale: f32) -> (&str, Vec<Patch>, bool) {
        let (image, dim) = self.states.get(state);
        let src = image.area.rect();
        let patches = match self.shape {
            Shape::Sprite => vec![Patch { src, dst }],
            Shape::Bar {
                vertical,
                ends,
                middle,
            } => geometry::bar(src, ends, vertical, middle, dst, scale),
            Shape::Frame {
                slice,
                edges,
                center,
                ..
            } => geometry::frame(src, slice, edges, center, dst, scale),
        };
        (&image.image, patches, dim)
    }

    /// Where a frame's content goes inside `dst`.
    pub fn content(&self, dst: Rect, scale: f32) -> Rect {
        match self.shape {
            Shape::Frame { padding, .. } => geometry::inset(dst, padding, scale),
            _ => dst,
        }
    }
}

/// The text roles a skin gives fonts for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// The time.
    Digits,
    /// The song's lines, kbps and kHz, the length.
    Display,
    /// Buttons, title bars, tabs, labels.
    Chrome,
    /// Playlists, Browse, Settings.
    List,
}

impl Role {
    pub const ALL: [Role; 4] = [Role::Digits, Role::Display, Role::Chrome, Role::List];

    pub fn key(self) -> &'static str {
        match self {
            Role::Digits => "digits",
            Role::Display => "display",
            Role::Chrome => "chrome",
            Role::List => "list",
        }
    }
}

/// A font for one role.
#[derive(Clone, Debug, PartialEq)]
pub enum Font {
    /// Glyphs cut from a grid of equal cells.
    Bitmap {
        /// The image, and where the grid starts in it.
        image: String,
        origin: [u32; 2],
        cell: [u32; 2],
        /// Points between glyphs, in image pixels.
        spacing: u32,
        /// Where each character's cell is, as (column, row).
        glyphs: HashMap<char, (u32, u32)>,
        /// The glyphs are white, to be drawn in the role's colour; else
        /// they keep their own.
        tint: bool,
    },
    /// A TrueType or OpenType file.
    File { bytes: Vec<u8>, size: f32 },
}

impl Font {
    /// A bitmap font's cell for `character`, if it has one. Lower case
    /// falls back to upper case, as in Winamp's font; anything else the
    /// font lacks is for Spotless's own font to draw.
    pub fn glyph(&self, character: char) -> Option<Area> {
        let Font::Bitmap {
            origin: [x, y],
            cell: [width, height],
            glyphs,
            ..
        } = self
        else {
            return None;
        };
        let upper = character.to_uppercase().next().unwrap_or(character);
        let &(column, row) = glyphs.get(&character).or_else(|| glyphs.get(&upper))?;
        Some(Area {
            x: x + column * width,
            y: y + row * height,
            width: *width,
            height: *height,
        })
    }
}

/// A skin read.
pub struct Skin {
    pub name: String,
    pub author: Option<String>,
    /// Points per image pixel at an interface size of 100%.
    pub scale: f32,
    pub filter: Filter,
    /// Classic's colours, with the skin's in their place.
    pub palette: Palette,
    pub fonts: HashMap<Role, Font>,
    pieces: HashMap<&'static str, Piece>,
    /// The images the pieces and fonts use, by path.
    pub images: HashMap<String, Bitmap>,
    /// What was wrong with it, part by part.
    pub problems: Vec<Problem>,
}

impl Skin {
    /// The skin at `path`: a folder, or a zip of one.
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let source = if path.is_dir() {
            source::Source::folder(path)
        } else {
            source::Source::archive(&std::fs::read(path)?)?
        };
        manifest::read(&source, &fallback_name(path))
    }

    /// A skin compiled into Spotless: its files by path.
    pub fn built_in(name: &str, files: &[(&str, &[u8])]) -> Result<Self, LoadError> {
        manifest::read(&source::Source::files(files), name)
    }

    /// The piece called `name`, or the one that stands in for it.
    pub fn piece(&self, name: &str) -> Option<&Piece> {
        self.pieces.get(name).or_else(|| {
            let (_, stand_in) = STAND_INS.iter().find(|(piece, _)| *piece == name)?;
            self.pieces.get(stand_in)
        })
    }
}

/// A skin's name when its manifest has none: its file's, without the
/// extension.
fn fallback_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Skin".into(), |stem| stem.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests;
