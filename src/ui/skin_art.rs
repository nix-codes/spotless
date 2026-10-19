//! A skin's bitmaps as textures, and painting their sprites.
//!
//! Sprites are pixel art: they are drawn unsmoothed, each skin pixel a
//! whole number of screen pixels (`unit`), from a corner on the pixel grid.

use std::collections::HashMap;

use egui::emath::GuiRounding;
use egui::{Color32, ColorImage, Painter, Pos2, Rect, TextureHandle, TextureOptions, pos2, vec2};

use crate::skin::{Sheet, Skin, Sprite, font};

/// Points per skin pixel, at most: twice Winamp's size, as its "double
/// size" mode, at an interface size of 100%.
const SCALE: f32 = 2.0;

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
            .filter_map(|sheet| {
                let bitmap = skin.sheet(sheet)?;
                let size = [bitmap.width as usize, bitmap.height as usize];
                let image = ColorImage::from_rgba_unmultiplied(size, &bitmap.rgba);
                let texture = ctx.load_texture(
                    format!("skin-{}", sheet.file_name()),
                    image,
                    TextureOptions::NEAREST,
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
        let at = at.round_to_pixels(painter.pixels_per_point());
        let rect = Rect::from_min_size(at, vec2(sprite.width as f32, sprite.height as f32) * unit);
        self.paint_into(painter, sprite, rect);
        rect
    }

    /// Paints a sprite stretched over `rect`. Only what the sheet holds of
    /// it is drawn, in its part of the rect.
    fn paint_into(&self, painter: &Painter, sprite: Sprite, rect: Rect) {
        let Some((texture, [width, height])) = self.sheets.get(&sprite.sheet) else {
            return;
        };
        let Some(clipped) = sprite.clipped_to(*width, *height) else {
            return;
        };
        let (w, h) = (*width as f32, *height as f32);
        let uv = Rect::from_min_max(
            pos2(clipped.x as f32 / w, clipped.y as f32 / h),
            pos2(
                (clipped.x + clipped.width) as f32 / w,
                (clipped.y + clipped.height) as f32 / h,
            ),
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
        );
        self.paint_into(painter, piece(cap, sprite.width - 2 * cap), middle);
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

/// Points per skin pixel: a whole number of screen pixels, as near to
/// `SCALE` points as stays within `most` points.
pub fn unit(ctx: &egui::Context, most: f32) -> f32 {
    let pixels_per_point = ctx.pixels_per_point();
    let fits = (most * pixels_per_point).floor().max(1.0);
    let pixels = (SCALE * pixels_per_point).round().clamp(1.0, fits);
    pixels / pixels_per_point
}
