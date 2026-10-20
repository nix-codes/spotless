//! Where a piece's slices go: from the parts of its image to the parts of
//! the rect it fills.
//!
//! A frame is cut in nine: corners kept at their size, edges stretched or
//! tiled along their length, a centre stretched, tiled or left out. A bar
//! is a frame cut only along its length. Corners that don't fit shrink
//! together rather than overlap.

use egui::{Rect, pos2};

/// How a slice fills the length it is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    Stretch,
    Tile,
}

/// One part of an image drawn into one part of the screen. `src` is in
/// image pixels, `dst` in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    pub src: Rect,
    pub dst: Rect,
}

/// Tiles shorter than this on screen are stretched instead, and no span
/// is cut into more tiles than `MAX_TILES`.
const MIN_TILE: f32 = 0.5;
const MAX_TILES: usize = 4096;

/// A frame's patches. `src` is the piece's area in its image, `slice` its
/// fixed `[left, top, right, bottom]` in image pixels, `scale` points per
/// image pixel. A `center` of None leaves the middle undrawn.
pub fn frame(
    src: Rect,
    slice: [u32; 4],
    edges: Fill,
    center: Option<Fill>,
    dst: Rect,
    scale: f32,
) -> Vec<Patch> {
    let [left, top, right, bottom] = slice.map(|n| n as f32);
    let columns = cuts(
        src.left(),
        src.right(),
        left,
        right,
        dst.left(),
        dst.right(),
        scale,
    );
    let rows = cuts(
        src.top(),
        src.bottom(),
        top,
        bottom,
        dst.top(),
        dst.bottom(),
        scale,
    );
    let mut patches = Vec::new();
    for (row, &(src_y, dst_y)) in rows.iter().enumerate() {
        for (column, &(src_x, dst_x)) in columns.iter().enumerate() {
            let (fill_x, fill_y) = match (column, row) {
                (1, 1) => match center {
                    Some(fill) => (fill, fill),
                    None => continue,
                },
                (1, _) => (edges, Fill::Stretch),
                (_, 1) => (Fill::Stretch, edges),
                _ => (Fill::Stretch, Fill::Stretch),
            };
            for (sx, dx) in span(src_x, dst_x, fill_x, scale) {
                for &(sy, dy) in &span(src_y, dst_y, fill_y, scale) {
                    patches.push(Patch {
                        src: Rect::from_min_max(pos2(sx.0, sy.0), pos2(sx.1, sy.1)),
                        dst: Rect::from_min_max(pos2(dx.0, dy.0), pos2(dx.1, dy.1)),
                    });
                }
            }
        }
    }
    patches
}

/// A bar's patches: fixed `ends` along its length, the middle filled.
pub fn bar(
    src: Rect,
    ends: [u32; 2],
    vertical: bool,
    middle: Fill,
    dst: Rect,
    scale: f32,
) -> Vec<Patch> {
    let [start, end] = ends;
    let slice = if vertical {
        [0, start, 0, end]
    } else {
        [start, 0, end, 0]
    };
    frame(src, slice, Fill::Stretch, Some(middle), dst, scale)
}

/// A rect inside `dst`, `padding` image pixels in from each side.
pub fn inset(dst: Rect, padding: [u32; 4], scale: f32) -> Rect {
    let [left, top, right, bottom] = padding.map(|n| n as f32 * scale);
    let min = pos2(
        (dst.left() + left).min(dst.center().x),
        (dst.top() + top).min(dst.center().y),
    );
    let max = pos2(
        (dst.right() - right).max(min.x),
        (dst.bottom() - bottom).max(min.y),
    );
    Rect::from_min_max(min, max)
}

type Range = (f32, f32);

/// One axis cut in three: the (source, destination) ranges of the start,
/// the middle and the end. Ends too long for the destination shrink by the
/// same factor; the middle then has no length.
fn cuts(
    src_from: f32,
    src_to: f32,
    start: f32,
    end: f32,
    dst_from: f32,
    dst_to: f32,
    scale: f32,
) -> [(Range, Range); 3] {
    let length = (dst_to - dst_from).max(0.0);
    let wanted = (start + end) * scale;
    let shrink = if wanted > length && wanted > 0.0 {
        length / wanted
    } else {
        1.0
    };
    let dst_start = dst_from + start * scale * shrink;
    let dst_end = dst_to - end * scale * shrink;
    [
        ((src_from, src_from + start), (dst_from, dst_start)),
        ((src_from + start, src_to - end), (dst_start, dst_end)),
        ((src_to - end, src_to), (dst_end, dst_to)),
    ]
}

/// A slice laid along its destination: once, stretched, or tiled at its
/// own size with the last tile cut short.
fn span(src: Range, dst: Range, fill: Fill, scale: f32) -> Vec<(Range, Range)> {
    let (src_length, dst_length) = (src.1 - src.0, dst.1 - dst.0);
    if src_length <= 0.0 || dst_length <= 0.0 {
        return Vec::new();
    }
    let tile = src_length * scale;
    if fill == Fill::Stretch || tile < MIN_TILE || dst_length / tile > MAX_TILES as f32 {
        return vec![(src, dst)];
    }
    let mut tiles = Vec::new();
    let mut at = dst.0;
    while at < dst.1 - f32::EPSILON {
        let length = tile.min(dst.1 - at);
        let part = src_length * length / tile;
        tiles.push(((src.0, src.0 + part), (at, at + length)));
        at += tile;
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), egui::vec2(w, h))
    }

    #[test]
    fn a_stretched_frame_keeps_its_corners_and_stretches_the_rest() {
        let src = rect(10.0, 20.0, 12.0, 9.0);
        let dst = rect(0.0, 0.0, 100.0, 40.0);
        let patches = frame(
            src,
            [3, 2, 4, 1],
            Fill::Stretch,
            Some(Fill::Stretch),
            dst,
            2.0,
        );
        assert_eq!(patches.len(), 9);
        // Top left corner: 3x2 image pixels at 2 points each.
        assert_eq!(patches[0].src, rect(10.0, 20.0, 3.0, 2.0));
        assert_eq!(patches[0].dst, rect(0.0, 0.0, 6.0, 4.0));
        // The centre takes what the corners leave.
        assert_eq!(patches[4].src, rect(13.0, 22.0, 5.0, 6.0));
        assert_eq!(patches[4].dst, rect(6.0, 4.0, 86.0, 34.0));
        // Bottom right corner.
        assert_eq!(patches[8].dst, rect(92.0, 38.0, 8.0, 2.0));
    }

    #[test]
    fn tiled_edges_repeat_at_their_size_and_cut_the_last_tile_short() {
        let src = rect(0.0, 0.0, 6.0, 6.0);
        let dst = rect(0.0, 0.0, 15.0, 6.0);
        let patches = frame(src, [2, 2, 2, 2], Fill::Tile, None, dst, 1.0);
        let top: Vec<_> = patches
            .iter()
            .filter(|p| p.dst.top() == 0.0 && p.dst.left() >= 2.0 && p.dst.right() <= 13.0)
            .collect();
        // 11 points of edge from a 2-pixel slice: five whole tiles and half
        // of one.
        assert_eq!(top.len(), 6);
        assert_eq!(top[5].dst, rect(12.0, 0.0, 1.0, 2.0));
        assert_eq!(top[5].src, rect(2.0, 0.0, 1.0, 2.0));
        // No centre.
        assert!(!patches.iter().any(|p| p.dst.contains(pos2(7.0, 3.0))));
    }

    #[test]
    fn corners_too_big_for_the_rect_shrink_together() {
        let src = rect(0.0, 0.0, 20.0, 20.0);
        let dst = rect(0.0, 0.0, 10.0, 40.0);
        let patches = frame(
            src,
            [8, 8, 12, 8],
            Fill::Stretch,
            Some(Fill::Stretch),
            dst,
            1.0,
        );
        // 20 points of corners in 10: each is halved, nothing in between.
        assert_eq!(patches[0].dst, rect(0.0, 0.0, 4.0, 8.0));
        assert_eq!(patches[1].dst, rect(4.0, 0.0, 6.0, 8.0));
        assert!(patches.iter().all(|p| p.dst.right() <= 10.0));
    }

    #[test]
    fn a_bar_cuts_only_along_its_length() {
        let src = rect(0.0, 60.0, 40.0, 10.0);
        let dst = rect(0.0, 0.0, 300.0, 15.0);
        let patches = bar(src, [4, 4], false, Fill::Stretch, dst, 1.5);
        assert_eq!(patches.len(), 3);
        assert_eq!(patches[0].dst, rect(0.0, 0.0, 6.0, 15.0));
        assert_eq!(patches[1].src, rect(4.0, 60.0, 32.0, 10.0));
        assert_eq!(patches[2].dst, rect(294.0, 0.0, 6.0, 15.0));

        let tall = rect(0.0, 0.0, 8.0, 100.0);
        let vertical = bar(
            rect(0.0, 0.0, 8.0, 18.0),
            [3, 3],
            true,
            Fill::Tile,
            tall,
            1.0,
        );
        assert_eq!(vertical[0].dst, rect(0.0, 0.0, 8.0, 3.0));
        assert_eq!(vertical.last().unwrap().dst, rect(0.0, 97.0, 8.0, 3.0));
        // 94 points of middle from a 12-pixel slice: eight tiles.
        assert_eq!(vertical.len(), 2 + 8);
    }

    #[test]
    fn tiny_or_endless_tiles_are_stretched() {
        let one = span((0.0, 1.0), (0.0, 50_000.0), Fill::Tile, 1.0);
        assert_eq!(one.len(), 1);
        let small = span((0.0, 1.0), (0.0, 10.0), Fill::Tile, 0.1);
        assert_eq!(small.len(), 1);
    }

    #[test]
    fn padding_never_turns_a_rect_inside_out() {
        let dst = rect(0.0, 0.0, 10.0, 10.0);
        assert_eq!(inset(dst, [1, 2, 3, 4], 1.0), rect(1.0, 2.0, 6.0, 4.0));
        let squeezed = inset(dst, [20, 20, 20, 20], 1.0);
        assert!(squeezed.width() >= 0.0 && squeezed.height() >= 0.0);
    }
}
