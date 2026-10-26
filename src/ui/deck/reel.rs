//! A '70s three-head reel-to-reel deck standing up, 336 units wide: a
//! brushed aluminium face, see-through aluminium reels on top, the tape
//! round the tension arms and across the heads, a counter, two VU meters,
//! and the player's buttons along its bottom edge as its keys.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Align2, Color32};

use super::Tape;
use super::canvas::{Canvas, Face, blend, rgb, rgba};

pub const WIDTH: f32 = 336.0;
/// The face above the keys.
pub const PLATE: f32 = 214.0;
/// A reel's hub, the tape round it when it holds the whole song, and the
/// reel's flanges.
const HUB: f32 = 20.0;
const FULL: f32 = 58.0;
const FLANGE: f32 = 62.0;
const TAPE: Color32 = rgb(0x4a2e1a);

/// The tape's radius on a reel holding `share` of it.
pub fn pack(share: f32) -> f32 {
    (HUB * HUB + share.clamp(0.0, 1.0) * (FULL * FULL - HUB * HUB)).sqrt()
}

/// The deck, the keys' strip `keys` units tall included.
pub fn draw(c: &Canvas, keys: f32, tape: &Tape) {
    let (w, h) = (WIDTH, PLATE + keys);
    c.shaded(
        0.0,
        0.0,
        w,
        h,
        6.0,
        &[
            (0.0, rgb(0xe2e5e9)),
            (0.6, rgb(0xc3c7cd)),
            (1.0, rgb(0xa9aeb5)),
        ],
    );
    // Brushed sideways.
    let mut y = 3.0;
    while y < h - 3.0 {
        let alpha = 0.06 + 0.05 * (y * 4.1).sin();
        c.line(&[(1.0, y), (w - 1.0, y)], 0.5, rgba(0xffffff, alpha));
        y += 2.0;
    }
    c.outline(0.0, 0.0, w, h, 6.0, 1.0, rgb(0x6a6f77));

    let (lx, rx, cy) = (80.0, w - 80.0, 70.0);
    let (rl, rr) = (pack(1.0 - tape.progress), pack(tape.progress));
    let ty = 158.0;
    let (arm_l, arm_r) = ((20.0, ty - 8.0), (w - 20.0, ty - 8.0));
    // Tension arms from their pivots near the corners.
    c.line(&[(30.0, ty + 22.0), arm_l], 3.0, rgb(0x5a5f67));
    c.line(&[(w - 30.0, ty + 22.0), arm_r], 3.0, rgb(0x5a5f67));
    // The tape: off the outside of each reel, round the arms, across the
    // heads.
    let off_l = tangent((arm_l.0 - 5.0, arm_l.1), (lx, cy), rl, -1.0);
    let off_r = tangent((arm_r.0 + 5.0, arm_r.1), (rx, cy), rr, 1.0);
    let mut path = vec![off_l];
    path.extend(arc_points(arm_l, 5.0, PI, FRAC_PI_2));
    path.extend(arc_points(arm_r, 5.0, FRAC_PI_2, 0.0));
    path.push(off_r);
    c.line(&path, 2.0, TAPE);
    for (x, y) in [arm_l, arm_r] {
        c.circle(x, y, 4.5, rgb(0xeef0f3));
        c.ring(x, y, 4.5, 1.0, rgb(0x6a6f77));
    }
    // The head block: three chrome heads touching the tape.
    let hx = w / 2.0;
    c.rect(hx - 56.0, ty - 2.0, 112.0, 26.0, 3.0, rgb(0x17181c));
    for (i, name) in ["ERASE", "REC", "PLAY"].into_iter().enumerate() {
        let gx = hx - 34.0 + i as f32 * 34.0;
        let chrome = [
            (0.0, rgb(0x7e848c)),
            (0.5, rgb(0xf2f4f6)),
            (1.0, rgb(0x7e848c)),
        ];
        c.shaded_by(gx - 7.0, ty - 6.0, 14.0, 8.0, 3.0, |u, _| blend(&chrome, u));
        c.text(
            name,
            gx,
            ty + 14.0,
            Face::Mono,
            5.5,
            rgb(0xb8bcc4),
            Align2::CENTER_CENTER,
        );
    }
    // Capstan and pinch roller.
    let kx = hx + 74.0;
    c.circle(kx, ty - 4.0, 2.5, rgb(0xf2f4f6));
    c.circle(kx, ty + 6.0, 7.0, rgb(0x121214));
    c.ring(kx, ty + 6.0, 3.0, 1.0, rgb(0x3a3a40));
    // The speed, between the reels.
    c.text(
        "7½",
        hx,
        cy - 4.0,
        Face::Sans,
        9.0,
        rgb(0x3a3e46),
        Align2::CENTER_CENTER,
    );
    c.text(
        "IPS",
        hx,
        cy + 7.0,
        Face::Mono,
        6.0,
        rgb(0x3a3e46),
        Align2::CENTER_CENTER,
    );
    // Along the bottom: the counter, the VU meters, the name.
    let by = PLATE - 18.0;
    c.rect(12.0, by - 8.0, 46.0, 16.0, 0.0, rgb(0x0a0a0c));
    c.outline(12.0, by - 8.0, 46.0, 16.0, 0.0, 1.0, rgb(0x55595f));
    let digits = format!("{:04}", tape.counter as u32 % 10_000);
    for (i, digit) in digits.chars().enumerate() {
        let x = 15.0 + i as f32 * 10.0;
        c.rect(x, by - 6.0, 8.0, 12.0, 0.0, rgb(0x1c1c20));
        c.text(
            &digit.to_string(),
            x + 4.0,
            by,
            Face::Mono,
            10.0,
            rgb(0xf0f0f2),
            Align2::CENTER_CENTER,
        );
    }
    vu_meter(c, hx - 27.0, by - 2.0, tape.needles[0], "L");
    vu_meter(c, hx + 27.0, by - 2.0, tape.needles[1], "R");
    c.spaced(
        "SPOTLESS",
        w - 12.0,
        by - 5.0,
        Face::Display,
        9.0,
        2.0,
        rgb(0x2a2d33),
        Align2::RIGHT_CENTER,
    );
    c.text(
        "X-90 · 3 HEAD",
        w - 12.0,
        by + 7.0,
        Face::Mono,
        6.0,
        rgb(0x4a4e56),
        Align2::RIGHT_CENTER,
    );
    // The reels, over where the tape leaves them.
    reel(c, lx, cy, rl, tape.angles[0], Some(&tape.list));
    reel(c, rx, cy, rr, tape.angles[1], None);
    // The keys' strip.
    c.rect(2.0, PLATE, w - 4.0, keys - 2.0, 4.0, rgb(0x1c1d22));
}

/// Where a line from `from` touches the circle at `centre`, on its left
/// (`side` -1) or right (1).
fn tangent(from: (f32, f32), centre: (f32, f32), r: f32, side: f32) -> (f32, f32) {
    let (dx, dy) = (from.0 - centre.0, from.1 - centre.1);
    let base = dy.atan2(dx);
    let off = (r / dx.hypot(dy)).min(1.0).acos();
    let at = |a: f32| (centre.0 + a.cos() * r, centre.1 + a.sin() * r);
    let (a, b) = (at(base + off), at(base - off));
    if (a.0 - b.0) * side > 0.0 { a } else { b }
}

fn arc_points(centre: (f32, f32), r: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
    (0..=6)
        .map(|i| {
            let a = from + (to - from) * i as f32 / 6.0;
            (centre.0 + a.cos() * r, centre.1 + a.sin() * r)
        })
        .collect()
}

/// An aluminium reel seen through: the back flange, the tape wound to
/// radius `pack`, then the front flange with three wide windows, and the
/// hub, all turned to `angle`; `tag` written on masking tape on it.
fn reel(c: &Canvas, x: f32, y: f32, pack: f32, angle: f32, tag: Option<&str>) {
    c.circle(x, y, FLANGE, rgb(0x6c727c));
    for k in 0..3 {
        let a = angle + k as f32 * TAU / 3.0 + PI / 3.0;
        c.line(
            &[
                (x + a.cos() * HUB, y + a.sin() * HUB),
                (x + a.cos() * FLANGE, y + a.sin() * FLANGE),
            ],
            1.0,
            rgb(0x4a4f57),
        );
    }
    let tape = [
        (0.0, rgb(0x7a5034)),
        (0.7, rgb(0x4a2d1a)),
        (1.0, rgb(0x2c1a0f)),
    ];
    for i in (0..=8).rev() {
        let t = i as f32 / 8.0;
        c.circle(x, y, HUB + (pack - HUB) * t, blend(&tape, t));
    }
    let mut ring = HUB + 3.0;
    while ring < pack - 1.0 {
        c.ring(x, y, ring, 0.5, rgba(0xffffff, 0.07));
        ring += 3.0;
    }
    c.ring(x, y, pack, 1.0, rgb(0xa07a5a));
    // The front flange, lit from the top left whichever way it's turned.
    let metal = [
        (0.0, rgb(0xf4f6f9)),
        (0.45, rgb(0xb4bac3)),
        (0.55, rgb(0xdfe3e8)),
        (1.0, rgb(0x80868f)),
    ];
    let (cos, sin) = (angle.cos(), angle.sin());
    let shade = |px: f32, py: f32| {
        let (u, v) = (px - x, py - y);
        let (u, v) = (u * cos + v * sin, -u * sin + v * cos);
        blend(&metal, ((u + v) / (2.0 * FLANGE) + 1.0) / 2.0)
    };
    c.sector(x, y, FLANGE - 6.0, FLANGE, 0.0, TAU, shade);
    c.sector(x, y, 0.0, HUB + 4.0, 0.0, TAU, shade);
    for k in 0..3 {
        let window_end = angle + k as f32 * TAU / 3.0 + 0.2 + TAU / 3.0 - 0.4;
        c.sector(
            x,
            y,
            HUB + 4.0,
            FLANGE - 6.0,
            window_end,
            window_end + 0.4,
            shade,
        );
    }
    c.ring(x, y, FLANGE, 1.0, rgba(0x3a3e46, 0.5));
    if let Some(tag) = tag {
        let a = angle - FRAC_PI_2 + 0.15;
        let (tx, ty) = (x + a.cos() * (FLANGE - 6.0), y + a.sin() * (FLANGE - 6.0));
        let along = a + FRAC_PI_2;
        let (ax, ay) = (along.cos(), along.sin());
        let (nx, ny) = (a.cos(), a.sin());
        let corner = |s: f32, t: f32| (tx + ax * s + nx * t, ty + ay * s + ny * t);
        c.polygon(
            &[
                corner(-26.0, -5.0),
                corner(26.0, -5.0),
                corner(26.0, 5.0),
                corner(-26.0, 5.0),
            ],
            rgb(0xe8dfc4),
        );
        c.text_centred_at_angle(tag, tx, ty, Face::Marker, 7.0, rgb(0x1c2a6e), along, 48.0);
    }
    // The hub and its three keys.
    let hub = [(0.0, rgb(0xffffff)), (1.0, rgb(0x8c929b))];
    for i in (0..=6).rev() {
        let t = i as f32 / 6.0;
        c.circle(
            x - 3.0 * (1.0 - t),
            y - 3.0 * (1.0 - t),
            (HUB - 2.0) * t.max(0.15),
            blend(&hub, t),
        );
    }
    c.circle(x, y, 7.0, rgb(0x2a2d33));
    for k in 0..3 {
        let a = angle + k as f32 * TAU / 3.0 - FRAC_PI_2;
        super::spoke(c, x, y, a, 4.0, 8.0, 2.0, rgb(0xc8ccd2));
    }
}

/// A VU meter, 48 × 26 units round `x, y`, its needle at `level` (0–1).
fn vu_meter(c: &Canvas, x: f32, y: f32, level: f32, label: &str) {
    let ink = rgb(0x2a1c08);
    let red = rgb(0xc0281c);
    c.rect(x - 24.0, y - 13.0, 48.0, 26.0, 2.0, rgb(0x1a1a1d));
    c.shaded(
        x - 22.0,
        y - 11.0,
        44.0,
        22.0,
        0.0,
        &[(0.0, rgb(0xffd88a)), (1.0, rgb(0xe2a040))],
    );
    let (px, py, r) = (x, y + 19.0, 24.0);
    let (from, to) = (-FRAC_PI_2 - 0.75, -FRAC_PI_2 + 0.75);
    let zero = from + (to - from) * 0.72;
    c.arc(px, py, r, from, zero, 1.2, ink);
    c.arc(px, py, r, zero, to, 1.2, red);
    for i in 0..=6 {
        let a = from + (to - from) * i as f32 / 6.0;
        c.line(
            &[
                (px + a.cos() * r, py + a.sin() * r),
                (px + a.cos() * (r + 3.0), py + a.sin() * (r + 3.0)),
            ],
            1.0,
            if i >= 5 { red } else { ink },
        );
    }
    c.text(
        "VU",
        x - 18.0,
        y + 6.0,
        Face::Sans,
        5.0,
        ink,
        Align2::LEFT_CENTER,
    );
    c.text(
        label,
        x + 18.0,
        y + 6.0,
        Face::Sans,
        5.0,
        ink,
        Align2::RIGHT_CENTER,
    );
    let a = from + (to - from) * level.clamp(0.0, 1.05);
    c.clipped(x - 22.0, y - 11.0, 44.0, 22.0).line(
        &[
            (px, py),
            (px + a.cos() * (r + 4.0), py + a.sin() * (r + 4.0)),
        ],
        1.0,
        rgb(0x111111),
    );
    c.rect(x - 22.0, y - 11.0, 44.0, 11.0, 0.0, rgba(0xffffff, 0.09));
}
