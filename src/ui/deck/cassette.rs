//! A cassette, 262 × 154 units: its shell, a label written in marker with
//! the playlist and the song, and a window onto the two reels winding the
//! tape from left to right as the song plays.

use egui::{Align2, Color32};

use super::Tape;
use super::canvas::{Canvas, Face, blend, rgb, rgba};

pub const WIDTH: f32 = 262.0;
pub const HEIGHT: f32 = 154.0;
/// A reel's hub, and the tape round it when it holds the whole side.
const HUB: f32 = 11.0;
const FULL: f32 = 30.0;
const INK: Color32 = rgb(0x1c2a6e);

/// The tape's radius on a reel holding `share` of it: the tape's area is
/// what's shared out.
pub fn pack(share: f32) -> f32 {
    (HUB * HUB + share.clamp(0.0, 1.0) * (FULL * FULL - HUB * HUB)).sqrt()
}

pub fn draw(c: &Canvas, tape: &Tape) {
    let (w, h) = (WIDTH, HEIGHT);
    c.shaded(
        0.0,
        0.0,
        w,
        h,
        8.0,
        &[(0.0, rgb(0x3b3b46)), (1.0, rgb(0x24242c))],
    );
    c.outline(0.0, 0.0, w, h, 8.0, 1.0, rgb(0x5a5a6c));
    for (x, y) in [
        (9.0, 9.0),
        (w - 9.0, 9.0),
        (9.0, h - 9.0),
        (w - 9.0, h - 9.0),
        (w / 2.0, h - 12.0),
    ] {
        c.circle(x, y, 3.2, rgb(0x1a1a20));
        c.line(&[(x - 2.0, y), (x + 2.0, y)], 1.0, rgb(0x6a6a7c));
    }
    // The label.
    let (lx, ly, lw, lh) = (18.0, 14.0, w - 36.0, h * 0.64);
    c.rect(lx, ly, lw, lh, 4.0, rgb(0xdcd6c4));
    c.rect(lx, ly + 24.0, lw, 4.0, 0.0, rgb(0x1f8a44));
    c.text(
        &tape.side.to_string(),
        lx + 10.0,
        ly + 13.0,
        Face::Sans,
        15.0,
        rgb(0x22222a),
        Align2::CENTER_CENTER,
    );
    let label = c.clipped(lx, ly, lw - 60.0, 26.0);
    label.text(
        &tape.list,
        lx + 22.0,
        ly + 14.0,
        Face::Marker,
        15.0,
        INK,
        Align2::LEFT_CENTER,
    );
    c.text(
        "TYPE II · 90",
        lx + lw - 8.0,
        ly + 13.0,
        Face::Mono,
        7.0,
        rgb(0x5a564c),
        Align2::RIGHT_CENTER,
    );
    c.clipped(lx, ly + 30.0, lw, 18.0).text(
        &tape.song,
        lx + 10.0,
        ly + 39.0,
        Face::Marker,
        11.0,
        INK,
        Align2::LEFT_CENTER,
    );
    // The window, with the reels.
    let (cx, cy, dx) = (w / 2.0, ly + 70.0, w * 0.2125);
    let (wx, wy, ww, wh) = (
        cx - dx - FULL - 4.0,
        cy - 21.0,
        2.0 * (dx + FULL + 4.0),
        42.0,
    );
    c.rect(wx, wy, ww, wh, 6.0, rgb(0x121218));
    let window = c.clipped(wx, wy, ww, wh);
    reel(
        &window,
        cx - dx,
        cy,
        pack(1.0 - tape.progress),
        tape.angles[0],
    );
    reel(&window, cx + dx, cy, pack(tape.progress), tape.angles[1]);
    window.rect(wx, wy, ww, wh / 2.4, 0.0, rgba(0xffffff, 0.06));
    c.outline(wx, wy, ww, wh, 6.0, 1.0, rgb(0x8a8474));
    // The bridge at the bottom, the tape across it and the head openings.
    let by = h - 30.0;
    c.polygon(
        &[(w * 0.2, h), (w * 0.25, by), (w * 0.75, by), (w * 0.8, h)],
        rgb(0x1c1c22),
    );
    c.rect(w * 0.24, h - 5.0, w * 0.52, 2.0, 0.0, rgb(0x4a2e1a));
    for fx in [0.3, 0.44, 0.56, 0.7] {
        c.rect(w * fx - 5.0, h - 14.0, 10.0, 8.0, 0.0, rgb(0x0a0a0e));
    }
}

/// A reel: the tape wound to radius `r`, and the white hub with its six
/// teeth turned to `angle`.
fn reel(c: &Canvas, x: f32, y: f32, r: f32, angle: f32) {
    let stops = [(0.0, rgb(0x3a2414)), (1.0, rgb(0x6b4428))];
    let steps = 8;
    for i in (0..=steps).rev() {
        let t = i as f32 / steps as f32;
        c.circle(x, y, HUB + (r - HUB) * t, blend(&stops, t));
    }
    let mut ring = HUB + 4.0;
    while ring < r {
        c.ring(x, y, ring, 1.0, rgba(0x000000, 0.25));
        ring += 5.0;
    }
    c.circle(x, y, HUB, rgb(0xe8e6df));
    c.circle(x, y, HUB - 4.0, rgb(0x16161c));
    for tooth in 0..6 {
        let a = angle + tooth as f32 * std::f32::consts::FRAC_PI_3;
        super::spoke(c, x, y, a, HUB - 7.0, HUB - 2.0, 1.6, rgb(0xe8e6df));
    }
}
