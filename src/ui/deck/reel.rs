//! A reel-to-reel deck seen from the front, 336 units wide, after an
//! Otari: two wooden reels with gold hubs, nearly touching, standing up out
//! of the case's top edge; the tape drops off each round a guide and runs
//! level through the heads, past the capstan; a counter, and the player's
//! buttons along the bottom as its transport keys.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_3, PI, TAU};

use egui::{Align2, Color32};

use super::Tape;
use super::canvas::{Canvas, Face, blend, rgb, rgba};

pub const WIDTH: f32 = 336.0;
/// Everything above the keys: as tall as the Walkman's body, so the deck
/// keeps its size whichever device plays.
pub const PLATE: f32 = 172.0;
/// A reel's hub, the tape round it when it holds the whole song, and the
/// reel.
const HUB: f32 = 22.0;
const FULL: f32 = 64.0;
const REEL: f32 = 70.0;
/// The holes in the reels, from the hub out, and half their width in
/// radians.
const HOLE_IN: f32 = 30.0;
const HOLE_OUT: f32 = 60.0;
const HOLE_HALF: f32 = 0.4;
/// The masking tape with the song on it, and its writing.
const TAG_WIDTH: f32 = 108.0;
const TAG_SIZE: f32 = 10.0;
/// Where the case's top edge is: the reels stand out over it.
const CASE_TOP: f32 = 40.0;
/// The case's wooden sides.
const SIDE: f32 = 8.0;
/// The line the tape runs along through the heads.
const TAPE_LINE: f32 = 150.0;
const TAPE: Color32 = rgb(0x7a5034);
const WOOD: [(f32, Color32); 3] = [
    (0.0, rgb(0x4a2414)),
    (0.5, rgb(0x6e3a20)),
    (1.0, rgb(0x4a2414)),
];

/// The tape's radius on a reel holding `share` of it.
pub fn pack(share: f32) -> f32 {
    (HUB * HUB + share.clamp(0.0, 1.0) * (FULL * FULL - HUB * HUB)).sqrt()
}

/// The deck, the keys' strip `keys` units tall included.
pub fn draw(c: &Canvas, keys: f32, tape: &Tape) {
    let (w, h) = (WIDTH, PLATE + keys);
    // The case: wooden all round, a dark grey panel, a wooden strip over
    // the keys and the keys' recess.
    c.shaded_by(0.0, CASE_TOP, w, h - CASE_TOP, 4.0, |u, _| blend(&WOOD, u));
    for i in 0..3 {
        let x = 1.5 + i as f32 * 2.5;
        for x in [x, w - x] {
            grain(c, h - CASE_TOP, |t| {
                (x + 0.6 * (t * 0.09 + x).sin(), CASE_TOP + t)
            });
        }
    }
    c.shaded(
        SIDE,
        CASE_TOP,
        w - 2.0 * SIDE,
        PLATE - CASE_TOP - 6.0,
        0.0,
        &[(0.0, rgb(0x505155)), (1.0, rgb(0x3c3d41))],
    );
    for i in 0..2 {
        let y = PLATE - 4.5 + i as f32 * 3.0;
        grain(c, w - 2.0 * SIDE, |t| {
            (SIDE + t, y + 0.5 * (t * 0.07 + y).sin())
        });
    }
    c.rect(SIDE, PLATE, w - 2.0 * SIDE, keys - 4.0, 0.0, rgb(0x26272a));
    // Vents between the reels' tops.
    for i in 0..6 {
        c.rect(
            w / 2.0 - 9.0,
            CASE_TOP + 4.0 + i as f32 * 4.0,
            18.0,
            2.0,
            0.0,
            rgb(0x2a2b2e),
        );
    }

    let (lx, rx, cy) = (96.0, w - 96.0, REEL);
    let (rl, rr) = (pack(1.0 - tape.progress), pack(tape.progress));
    // The tape: off each reel's outer side, round a guide, level through
    // the heads.
    let guide_l = (40.0, TAPE_LINE - 4.5);
    let guide_r = (w - 40.0, TAPE_LINE - 4.5);
    let off_l = tangent((guide_l.0 - 4.5, guide_l.1), (lx, cy), rl, -1.0);
    let off_r = tangent((guide_r.0 + 4.5, guide_r.1), (rx, cy), rr, 1.0);
    let mut path = vec![off_l];
    path.extend(arc_points(guide_l, 4.5, PI, FRAC_PI_2));
    path.extend(arc_points(guide_r, 4.5, FRAC_PI_2, 0.0));
    path.push(off_r);
    c.line(&path, 2.0, TAPE);
    let chrome = [(0.0, rgb(0xf4f6f8)), (1.0, rgb(0x8a9098))];
    for (x, y) in [guide_l, guide_r] {
        c.sector(x, y, 0.0, 4.0, 0.0, TAU, |px, py| {
            blend(&chrome, ((px - x + 1.5).hypot(py - y + 1.5) / 4.5).min(1.0))
        });
        c.circle(x, y, 1.2, rgb(0x3a3a3a));
    }
    // The head block between the reels' lower edges, its heads touching
    // the tape, and its name plate.
    let hx = w / 2.0;
    c.shaded(
        hx - 27.0,
        124.0,
        54.0,
        36.0,
        5.0,
        &[(0.0, rgb(0x6a6c70)), (1.0, rgb(0x45474b))],
    );
    c.outline(hx - 27.0, 124.0, 54.0, 36.0, 5.0, 1.0, rgb(0x2a2b2e));
    let shine = [
        (0.0, rgb(0x7e848c)),
        (0.5, rgb(0xf2f4f6)),
        (1.0, rgb(0x7e848c)),
    ];
    for dx in [-15.0, 0.0, 15.0] {
        c.shaded_by(hx + dx - 5.0, TAPE_LINE - 6.0, 10.0, 8.0, 3.0, |u, _| {
            blend(&shine, u)
        });
    }
    c.rect(hx - 20.0, 152.0, 40.0, 6.0, 1.5, rgb(0x1e1f22));
    c.text(
        "SPOTLESS",
        hx,
        155.0,
        Face::Display,
        4.5,
        rgb(0xc8ccd2),
        Align2::CENTER_CENTER,
    );
    // Capstan and pinch roller, right of the heads.
    let kx = hx + 44.0;
    c.circle(kx - 9.0, TAPE_LINE - 2.0, 1.8, rgb(0xf2f4f6));
    c.circle(kx, TAPE_LINE + 1.0, 7.0, rgb(0x121214));
    c.ring(kx, TAPE_LINE + 1.0, 3.0, 1.0, rgb(0x3a3a40));
    // The counter, left of the heads; the model, right.
    let (cx, cy_counter) = (60.0, 159.0);
    c.rect(cx - 23.0, cy_counter - 7.0, 46.0, 14.0, 0.0, rgb(0x0a0a0c));
    c.outline(
        cx - 23.0,
        cy_counter - 7.0,
        46.0,
        14.0,
        0.0,
        1.0,
        rgb(0x2a2b2e),
    );
    let digits = format!("{:04}", tape.counter as u32 % 10_000);
    for (i, digit) in digits.chars().enumerate() {
        let x = cx - 20.0 + i as f32 * 10.0;
        c.rect(x, cy_counter - 5.0, 8.0, 10.0, 0.0, rgb(0x1c1c20));
        c.text(
            &digit.to_string(),
            x + 4.0,
            cy_counter,
            Face::Mono,
            9.0,
            rgb(0xf0f0f2),
            Align2::CENTER_CENTER,
        );
    }
    c.text(
        "X-90 · 7½ IPS",
        w - 14.0,
        cy_counter,
        Face::Mono,
        6.5,
        rgb(0xb8bcc4),
        Align2::RIGHT_CENTER,
    );
    // The reels last, over the tape's start and the case's top edge.
    reel(c, lx, cy, rl, tape.angles[0], Some(&tape.title));
    reel(c, rx, cy, rr, tape.angles[1], None);
}

/// A wavy line of wood grain, `length` units long, its points at `at(t)`.
fn grain(c: &Canvas, length: f32, at: impl Fn(f32) -> (f32, f32)) {
    let points: Vec<_> = (0..=(length / 4.0) as usize)
        .map(|i| at(i as f32 * 4.0))
        .collect();
    c.line(&points, 0.8, rgba(0x000000, 0.14));
}

/// `text` in marker, cut short with dots to fit `room` units.
fn cut(c: &Canvas, text: &str, room: f32) -> String {
    let width = |text: &str| c.width(text, Face::Marker, TAG_SIZE);
    if width(text) <= room {
        return text.to_string();
    }
    let mut kept: String = text.to_string();
    while !kept.is_empty() && width(&format!("{}...", kept.trim_end())) > room {
        kept.pop();
    }
    format!("{}...", kept.trim_end())
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

/// Whether a point `r` from a reel's centre, at `a` radians in the reel's
/// own turn, is wood: not in a hole, not past the edge.
fn on_wood(r: f32, a: f32) -> bool {
    if !(HUB..=REEL).contains(&r) {
        return false;
    }
    if !(HOLE_IN..=HOLE_OUT).contains(&r) {
        return true;
    }
    (0..3).all(|k| {
        let centre = FRAC_PI_2 + k as f32 * TAU / 3.0;
        let off = (a - centre + PI).rem_euclid(TAU) - PI;
        off.abs() > HOLE_HALF
    })
}

/// A wooden reel turned to `angle`: what shows through its three holes
/// (the back flange, dark, and the tape wound to radius `pack`), then the
/// front flange, its grain turning with it, the gold hub, and `tag`
/// written on masking tape on it.
fn reel(c: &Canvas, x: f32, y: f32, pack: f32, angle: f32, tag: Option<&str>) {
    let turned = |r: f32, a: f32| (x + (a + angle).cos() * r, y + (a + angle).sin() * r);
    c.circle(x, y + 3.0, REEL + 1.5, rgba(0x000000, 0.35));
    c.circle(x, y, REEL, rgb(0x0e0907));
    let tape = [
        (0.0, rgb(0x8a6040)),
        (0.6, rgb(0x6a4630)),
        (1.0, rgb(0x4a2e1c)),
    ];
    for i in (0..=8).rev() {
        let t = i as f32 / 8.0;
        c.circle(x, y, HUB + (pack - HUB) * t, blend(&tape, t));
    }
    let mut ring = HUB + 2.0;
    while ring < pack - 1.0 {
        c.ring(x, y, ring, 0.5, rgba(0x000000, 0.13));
        ring += 2.2;
    }
    c.ring(x, y, pack, 1.2, rgb(0xc09070));

    // The front flange: wood shaded across in its own turn, so it turns.
    let (cos, sin) = (angle.cos(), angle.sin());
    let wood = |px: f32, py: f32| {
        let (u, v) = (px - x, py - y);
        let along = u * cos + v * sin + (-u * sin + v * cos) * 0.4;
        blend(&WOOD, (along / (2.0 * REEL) + 0.5).clamp(0.0, 1.0))
    };
    c.sector(x, y, HUB - 1.0, HOLE_IN, 0.0, TAU, wood);
    c.sector(x, y, HOLE_OUT, REEL, 0.0, TAU, wood);
    for k in 0..3 {
        let from = angle + FRAC_PI_2 + k as f32 * TAU / 3.0 + HOLE_HALF;
        c.sector(
            x,
            y,
            HOLE_IN - 0.5,
            HOLE_OUT + 0.5,
            from,
            from + TAU / 3.0 - 2.0 * HOLE_HALF,
            wood,
        );
    }
    // The grain: wavy lines across the reel, broken where there's no wood.
    let mut row = -REEL;
    while row <= REEL {
        let mut run: Vec<(f32, f32)> = Vec::new();
        let mut across = -REEL;
        while across <= REEL + 3.0 {
            let v = row
                + 2.2 * (across * 0.045 + row * 0.6 + x * 0.37).sin()
                + 1.2 * (across * 0.12 + row * 1.7).sin();
            let (r, a) = (across.hypot(v), v.atan2(across));
            if on_wood(r, a) {
                run.push(turned(r, a));
            } else {
                if run.len() > 1 {
                    c.line(&run, 0.8, rgba(0x000000, 0.11));
                }
                run.clear();
            }
            across += 3.0;
        }
        if run.len() > 1 {
            c.line(&run, 0.8, rgba(0x000000, 0.11));
        }
        row += 2.6;
    }
    // The holes' bevelled edges.
    for k in 0..3 {
        let centre = FRAC_PI_2 + k as f32 * TAU / 3.0;
        let mut edge = Vec::new();
        for i in 0..=8 {
            let a = centre - HOLE_HALF + 2.0 * HOLE_HALF * i as f32 / 8.0;
            edge.push(turned(HOLE_IN, a));
        }
        for i in 0..=12 {
            let a = centre + HOLE_HALF - 2.0 * HOLE_HALF * i as f32 / 12.0;
            edge.push(turned(HOLE_OUT, a));
        }
        c.polygon_outline(&edge, 2.0, rgb(0x1c0e06));
    }
    // A small print between the upper holes, and the screws round the hub.
    let (px, py) = turned(44.0, -FRAC_PI_2);
    c.text_centred_at_angle(
        "SPOTLESS",
        px,
        py,
        Face::Display,
        7.0,
        rgba(0xf0e6d8, 0.82),
        angle,
        44.0,
    );
    for k in 0..3 {
        let a = FRAC_PI_2 + FRAC_PI_3 + k as f32 * TAU / 3.0;
        let (sx, sy) = turned(29.0, a);
        c.circle(sx, sy, 2.0, rgb(0x1a1a1a));
        let (dx, dy) = ((a + angle).cos() * 1.4, (a + angle).sin() * 1.4);
        c.line(
            &[(sx - dy, sy + dx), (sx + dy, sy - dx)],
            0.6,
            rgb(0x6a6a6a),
        );
    }
    // The song on a strip of masking tape under the hub, over part of the
    // lower hole, cut short if it's too long for it.
    if let Some(tag) = tag {
        let down = angle + FRAC_PI_2;
        let (tx, ty) = (x + down.cos() * 33.0, y + down.sin() * 33.0);
        let along = angle;
        let (ax, ay) = (along.cos(), along.sin());
        let (nx, ny) = (down.cos(), down.sin());
        let corner = |s: f32, t: f32| (tx + ax * s + nx * t, ty + ay * s + ny * t);
        let (half_w, half_h) = (TAG_WIDTH / 2.0, 7.5);
        c.polygon(
            &[
                corner(-half_w, -half_h),
                corner(half_w, -half_h),
                corner(half_w, half_h),
                corner(-half_w, half_h),
            ],
            rgb(0xe8dfc4),
        );
        c.polygon_outline(
            &[
                corner(-half_w, -half_h),
                corner(half_w, -half_h),
                corner(half_w, half_h),
                corner(-half_w, half_h),
            ],
            0.6,
            rgba(0x8a7a58, 0.6),
        );
        let text = cut(c, tag, TAG_WIDTH - 8.0);
        c.text_centred_at_angle(
            &text,
            tx,
            ty,
            Face::Marker,
            TAG_SIZE,
            rgb(0x1c2a6e),
            along,
            f32::MAX,
        );
    }
    // The gold hub, a dark ring, the cream cap and the spindle's keys.
    let gold = [
        (0.0, rgb(0xf4d878)),
        (0.6, rgb(0xc99a2e)),
        (1.0, rgb(0x7a5810)),
    ];
    c.sector(x, y, 0.0, HUB, 0.0, TAU, |px, py| {
        blend(
            &gold,
            ((px - x + 6.0).hypot(py - y + 6.0) / (HUB + 6.0)).min(1.0),
        )
    });
    c.circle(x, y, 16.0, rgb(0x2a2418));
    let cream = [(0.0, rgb(0xf4ecd0)), (1.0, rgb(0xc8bc94))];
    for i in (0..=5).rev() {
        let t = i as f32 / 5.0;
        c.circle(
            x - 3.0 * (1.0 - t),
            y - 3.0 * (1.0 - t),
            13.0 * t.max(0.2),
            blend(&cream, t),
        );
    }
    c.circle(x, y, 3.2, rgb(0x3a3a3a));
    for k in 0..3 {
        let a = angle + k as f32 * TAU / 3.0 - FRAC_PI_2;
        super::spoke(c, x, y, a, 4.0, 8.0, 2.0, rgb(0x8a8a8a));
    }
    // The rim, and a sheen from the top left that doesn't turn.
    c.ring(x, y, REEL, 1.2, rgb(0x1c0e06));
    for i in 0..5 {
        let (r0, r1) = (i as f32 * REEL / 5.0, (i + 1) as f32 * REEL / 5.0);
        c.sector(x, y, r0, r1, 0.0, TAU, |px, py| {
            let t = ((px - x) + (py - y)) / (2.8 * REEL) + 0.5;
            rgba(0xffffff, (0.11 * (1.0 - t)).max(0.0))
        });
    }
}
