//! A skin's images as textures, and painting its pieces and text.
//!
//! Pixel art is drawn `unit` points to an image pixel, seldom a whole
//! number of screen pixels, so each image is first enlarged `ENLARGE`
//! times unsmoothed and then scaled down smoothly: every pixel comes out
//! the same size, only its edges a little soft. Painted art is only scaled
//! smoothly.

use std::collections::HashMap;
use std::sync::Arc;

use egui::emath::GuiRounding;
use egui::{
    Align2, Color32, ColorImage, FontId, Id, Painter, Pos2, Rect, Shape, TextureHandle,
    TextureOptions, Vec2, pos2, vec2,
};

use crate::skin::Bitmap;
use crate::skin::font::fold;
use crate::skin::format::{Filter, Font, Role, Skin, State};

/// How much pixel art is enlarged before it is scaled down.
const ENLARGE: usize = 4;

/// The pieces the skinned player strip needs; without any of them, the
/// strip is Classic's in the skin's colours.
const PLAYER: [&str; 16] = [
    "previous",
    "play",
    "pause",
    "stop",
    "next",
    "eject",
    "shuffle",
    "repeat",
    "pl",
    "status.playing",
    "status.paused",
    "status.stopped",
    "seek.track",
    "seek.thumb",
    "volume.track",
    "volume.thumb",
];

/// The fonts it needs, as bitmaps.
const PLAYER_FONTS: [Role; 2] = [Role::Digits, Role::Display];

pub struct SkinPaint {
    skin: Skin,
    textures: HashMap<String, (TextureHandle, [u32; 2], usize)>,
}

impl SkinPaint {
    pub fn new(ctx: &egui::Context, skin: Skin) -> Self {
        let enlarge = match skin.filter {
            Filter::Pixel => ENLARGE,
            Filter::Smooth => 1,
        };
        let textures = skin
            .images
            .iter()
            .map(|(path, bitmap)| {
                let texture = ctx.load_texture(
                    format!("skin-{path}"),
                    enlarged(bitmap, enlarge),
                    TextureOptions::LINEAR,
                );
                let size = [bitmap.width, bitmap.height];
                (path.clone(), (texture, size, enlarge))
            })
            .collect();
        Self { skin, textures }
    }

    /// Points per image pixel at an interface size of 100%.
    pub fn scale(&self) -> f32 {
        self.skin.scale
    }

    pub fn has(&self, name: &str) -> bool {
        self.skin.piece(name).is_some()
    }

    /// What the skinned player strip needs that the skin lacks.
    pub fn missing_player(&self) -> Vec<&'static str> {
        let pieces = PLAYER.into_iter().filter(|name| !self.has(name));
        let fonts = PLAYER_FONTS
            .into_iter()
            .filter(|role| !matches!(self.skin.fonts.get(role), Some(Font::Bitmap { .. })))
            .map(|role| match role {
                Role::Digits => "a digits font",
                _ => "a display font",
            });
        pieces.chain(fonts).collect()
    }

    pub fn has_player(&self) -> bool {
        self.missing_player().is_empty()
    }

    /// Plain space either side of a title bar's name, in image pixels.
    pub fn title_gap(&self) -> f32 {
        self.skin.piece("title").map_or(0, |piece| piece.title_gap) as f32
    }

    /// A piece's own size, `unit` points to an image pixel.
    pub fn size(&self, name: &str, unit: f32) -> Option<Vec2> {
        Some(self.skin.piece(name)?.size(unit))
    }

    /// Where a frame's content goes, drawn over `rect`.
    pub fn content(&self, name: &str, rect: Rect, unit: f32) -> Option<Rect> {
        Some(self.skin.piece(name)?.content(rect, unit))
    }

    /// Paints a piece over `rect`; false when the skin hasn't it.
    pub fn paint(
        &self,
        painter: &Painter,
        name: &str,
        state: State,
        rect: Rect,
        unit: f32,
    ) -> bool {
        match self.shapes(name, state, rect, unit, painter.pixels_per_point()) {
            Some(shapes) => {
                painter.extend(shapes);
                true
            }
            None => false,
        }
    }

    /// A piece over `rect` as shapes to paint, for a place that must put
    /// it under what is drawn first.
    pub fn shapes(
        &self,
        name: &str,
        state: State,
        rect: Rect,
        unit: f32,
        pixels_per_point: f32,
    ) -> Option<Vec<Shape>> {
        let piece = self.skin.piece(name)?;
        let (image, patches, dim) = piece.patches(state, rect, unit);
        let tint = if dim {
            Color32::WHITE.gamma_multiply(0.5)
        } else {
            Color32::WHITE
        };
        let shapes = patches
            .into_iter()
            .filter_map(|patch| {
                let dst = patch.dst.round_to_pixels(pixels_per_point);
                self.image(image, patch.src, dst, tint)
            })
            .collect();
        Some(shapes)
    }

    /// Paints a piece at its own size with its corner at `at`, and gives
    /// the rect it covers.
    pub fn paint_at(
        &self,
        painter: &Painter,
        name: &str,
        state: State,
        at: Pos2,
        unit: f32,
    ) -> Option<Rect> {
        let size = self.size(name, unit)?;
        let rect = Rect::from_min_size(at, size).round_to_pixels(painter.pixels_per_point());
        self.paint(painter, name, state, rect, unit);
        Some(rect)
    }

    /// A text role's cell: its advance and height, in image pixels.
    pub fn cell(&self, role: Role) -> Vec2 {
        match self.skin.fonts.get(&role) {
            Some(Font::Bitmap {
                cell: [width, height],
                spacing,
                ..
            }) => vec2((width + spacing) as f32, *height as f32),
            _ => vec2(5.0, 6.0),
        }
    }

    /// A line of a role's bitmap font, cut off at `width` points. A
    /// character the font hasn't is drawn with Spotless's own, in `color`.
    #[allow(clippy::too_many_arguments)]
    pub fn text(
        &self,
        painter: &Painter,
        role: Role,
        text: &str,
        at: Pos2,
        width: f32,
        unit: f32,
        color: Color32,
    ) {
        let cell = self.cell(role) * unit;
        let clip = Rect::from_min_size(at, vec2(width, cell.y));
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(clip));
        let font = self.skin.fonts.get(&role);
        for (index, character) in text.chars().enumerate() {
            let x = at.x + index as f32 * cell.x;
            if x >= at.x + width {
                break;
            }
            let glyph = font.and_then(|font| {
                font.glyph(character)
                    .or_else(|| font.glyph(fold(character)))
            });
            match (font, glyph) {
                (Some(Font::Bitmap { image, tint, .. }), Some(area)) => {
                    let dst = Rect::from_min_size(pos2(x, at.y), area.size() * unit)
                        .round_to_pixels(painter.pixels_per_point());
                    let tint = if *tint { color } else { Color32::WHITE };
                    if let Some(shape) = self.image(image, area.rect(), dst, tint) {
                        painter.add(shape);
                    }
                }
                _ if character != ' ' => {
                    painter.text(
                        pos2(x + cell.x / 2.0, at.y + cell.y / 2.0),
                        Align2::CENTER_CENTER,
                        character,
                        FontId::monospace(cell.y * 1.2),
                        color,
                    );
                }
                _ => {}
            }
        }
    }

    /// `src` of an image (in its pixels) drawn over `dst`.
    fn image(&self, image: &str, src: Rect, dst: Rect, tint: Color32) -> Option<Shape> {
        let (texture, [width, height], enlarge) = self.textures.get(image)?;
        // Half a texel of the enlarged image in from the edges, so the
        // smoothing never reaches the neighbouring piece.
        let inset = 0.5 / *enlarge as f32;
        let (w, h) = (*width as f32, *height as f32);
        let uv = Rect::from_min_max(
            pos2((src.left() + inset) / w, (src.top() + inset) / h),
            pos2((src.right() - inset) / w, (src.bottom() - inset) / h),
        );
        Some(Shape::image(texture.id(), dst, uv, tint))
    }
}

/// Where the skin in use is kept for the widgets, which see only colours.
fn key() -> Id {
    Id::new("skin-paint")
}

/// Makes `paint` the skin the widgets draw their chrome with.
pub fn set_current(ctx: &egui::Context, paint: Option<Arc<SkinPaint>>) {
    ctx.data_mut(|data| match paint {
        Some(paint) => {
            data.insert_temp(key(), paint);
        }
        None => data.remove::<Arc<SkinPaint>>(key()),
    });
}

pub fn current(ctx: &egui::Context) -> Option<Arc<SkinPaint>> {
    ctx.data(|data| data.get_temp(key()))
}

/// Paints a piece of the skin in use over `rect` at the skin's scale:
/// false when there is no skin, or it hasn't the piece.
pub fn chrome(painter: &Painter, name: &str, state: State, rect: Rect) -> bool {
    current(painter.ctx())
        .is_some_and(|paint| paint.paint(painter, name, state, rect, paint.scale()))
}

/// Whether the skin in use has a piece.
pub fn has_chrome(ctx: &egui::Context, name: &str) -> bool {
    current(ctx).is_some_and(|paint| paint.has(name))
}

/// The state of a control: `on` for a toggle that is on.
pub fn state(response: &egui::Response, on: bool, enabled: bool) -> State {
    match (response.is_pointer_button_down_on(), response.hovered(), on) {
        _ if !enabled => State::Disabled,
        (true, _, true) => State::OnPressed,
        (true, _, false) => State::Pressed,
        (false, true, true) => State::OnHover,
        (false, true, false) => State::Hover,
        (false, false, true) => State::On,
        (false, false, false) => State::Normal,
    }
}

/// An image `enlarge` times the size, each pixel a square block.
fn enlarged(bitmap: &Bitmap, enlarge: usize) -> ColorImage {
    let (width, height) = (bitmap.width as usize, bitmap.height as usize);
    let mut rgba = Vec::with_capacity(width * height * 4 * enlarge * enlarge);
    for row in bitmap.rgba.chunks_exact(width * 4) {
        let line: Vec<u8> = row
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|pixel| std::iter::repeat_n(pixel, enlarge).flatten().copied())
            .collect();
        for _ in 0..enlarge {
            rgba.extend_from_slice(&line);
        }
    }
    ColorImage::from_rgba_unmultiplied([width * enlarge, height * enlarge], &rgba)
}
