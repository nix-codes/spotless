//! A skin's bitmaps as textures, and painting their sprites.
//!
//! Sprites are pixel art, drawn `unit` points to a skin pixel. That is
//! seldom a whole number of screen pixels, so each sheet is first enlarged
//! `ENLARGE` times unsmoothed and then scaled down smoothly: every skin
//! pixel comes out the same size, only its edges a little soft.

use std::collections::HashMap;

use egui::emath::GuiRounding;
use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};

use crate::skin::{Sheet, Skin, Sprite, font};

/// Points per skin pixel, at most: half again Winamp's size, at an
/// interface size of 100%.
const SCALE: f32 = 1.5;
/// How much the sheets are enlarged before they are scaled down.
const ENLARGE: usize = 4;

/// The textures of a skin that has everything the player strip draws.
pub struct SkinArt {
    sheets: HashMap<Sheet, (TextureHandle, [u32; 2])>,
    /// The sheet the time's digits come from.
    pub digits: Sheet,
}

impl SkinArt {
    /// None when the skin lacks some of the player's files.
    pub fn new(ctx: &egui::Context, skin: &Skin) -> Option<Self> {
        if !skin.missing_art().is_empty() {
            return None;
        }
        let sheets = Sheet::ALL
            .into_iter()
            // The main window's background only gives colours.
            .filter(|&sheet| sheet != Sheet::Main)
            .filter_map(|sheet| {
                let bitmap = skin.sheet(sheet)?;
                let texture = ctx.load_texture(
                    format!("skin-{}", sheet.file_name()),
                    enlarged(bitmap),
                    TextureOptions::LINEAR,
                );
                Some((sheet, (texture, [bitmap.width, bitmap.height])))
            })
            .collect();
        Some(Self {
            sheets,
            digits: skin.digits()?,
        })
    }

    /// Paints a sprite with its corner at `at`, `unit` points to a skin
    /// pixel, and gives the rect it covers.
    pub fn paint(&self, painter: &Painter, sprite: Sprite, at: Pos2, unit: f32) -> Rect {
        let rect = on_pixels(painter, sprite, at, unit);
        self.paint_into(painter, sprite, rect, false);
        rect
    }

    /// Paints a sprite flipped left to right.
    pub fn paint_mirrored(&self, painter: &Painter, sprite: Sprite, at: Pos2, unit: f32) {
        let rect = on_pixels(painter, sprite, at, unit);
        self.paint_into(painter, sprite, rect, true);
    }

    pub fn has(&self, sheet: Sheet) -> bool {
        self.sheets.contains_key(&sheet)
    }

    /// Paints a sprite stretched over `rect`. Only what the sheet holds of
    /// it is drawn, in its part of the rect.
    pub fn paint_into(&self, painter: &Painter, sprite: Sprite, rect: Rect, mirrored: bool) {
        let Some((texture, [width, height])) = self.sheets.get(&sprite.sheet) else {
            return;
        };
        let Some(clipped) = sprite.clipped_to(*width, *height) else {
            return;
        };
        // Half a texel of the enlarged sheet in from the edges, so the
        // smoothing never reaches the neighbouring sprite.
        let inset = 0.5 / ENLARGE as f32;
        let (w, h) = (*width as f32, *height as f32);
        let (mut left, mut right) = (
            (clipped.x as f32 + inset) / w,
            ((clipped.x + clipped.width) as f32 - inset) / w,
        );
        if mirrored {
            std::mem::swap(&mut left, &mut right);
        }
        let uv = Rect::from_min_max(
            pos2(left, (clipped.y as f32 + inset) / h),
            pos2(right, ((clipped.y + clipped.height) as f32 - inset) / h),
        );
        let part = vec2(
            clipped.width as f32 / sprite.width as f32,
            clipped.height as f32 / sprite.height as f32,
        );
        let rect = Rect::from_min_size(rect.min, rect.size() * part);
        painter.image(texture.id(), rect, uv, Color32::WHITE);
    }

    /// Paints a sprite `width` points wide: its ends as they are, its
    /// middle stretched, so a gradient stays smooth.
    pub fn paint_bar(&self, painter: &Painter, sprite: Sprite, at: Pos2, width: f32, unit: f32) {
        let at = at.round_to_pixels(painter.pixels_per_point());
        let cap = (sprite.width / 8).max(1);
        let piece =
            |x: u32, w: u32| Sprite::new(sprite.sheet, sprite.x + x, sprite.y, w, sprite.height);
        let height = sprite.height as f32 * unit;
        let right = (at.x + width - cap as f32 * unit).round_to_pixels(painter.pixels_per_point());
        let middle = Rect::from_min_max(
            pos2(at.x + cap as f32 * unit, at.y),
            pos2(right, at.y + height),
        )
        .round_to_pixels(painter.pixels_per_point());
        self.paint_into(painter, piece(cap, sprite.width - 2 * cap), middle, false);
        self.paint(painter, piece(0, cap), at, unit);
        self.paint(
            painter,
            piece(sprite.width - cap, cap),
            pos2(right, at.y),
            unit,
        );
    }

    /// A line of the skin's bitmap font, cut off at `width` points.
    pub fn text(&self, painter: &Painter, text: &str, at: Pos2, width: f32, unit: f32) {
        let clip = Rect::from_min_size(at, vec2(width, 6.0 * unit));
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(clip));
        for (index, character) in text.chars().enumerate() {
            let x = at.x + (5 * index) as f32 * unit;
            if x >= at.x + width {
                break;
            }
            self.paint(&painter, font::glyph(character), pos2(x, at.y), unit);
        }
    }
}

/// Where a sprite at `at` goes, both its edges on screen pixels, so
/// sprites side by side meet without a gap.
fn on_pixels(painter: &Painter, sprite: Sprite, at: Pos2, unit: f32) -> Rect {
    let size = vec2(sprite.width as f32, sprite.height as f32) * unit;
    let pixels_per_point = painter.pixels_per_point();
    Rect::from_min_max(
        at.round_to_pixels(pixels_per_point),
        (at + size).round_to_pixels(pixels_per_point),
    )
}

/// Points per skin pixel: `SCALE`, or less to stay within `most`.
pub fn unit(most: f32) -> f32 {
    SCALE.min(most)
}

/// A bitmap `ENLARGE` times the size, each pixel a square block.
fn enlarged(bitmap: &crate::skin::Bitmap) -> ColorImage {
    let (width, height) = (bitmap.width as usize, bitmap.height as usize);
    let mut rgba = Vec::with_capacity(width * height * 4 * ENLARGE * ENLARGE);
    for row in bitmap.rgba.chunks_exact(width * 4) {
        let line: Vec<u8> = row
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| std::iter::repeat_n(pixel, ENLARGE).flatten().copied())
            .collect();
        for _ in 0..ENLARGE {
            rgba.extend_from_slice(&line);
        }
    }
    ColorImage::from_rgba_unmultiplied([width * ENLARGE, height * ENLARGE], &rgba)
}
