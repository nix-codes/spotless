//! An '80s DJ turntable seen from above, 336 units wide and black like the
//! Walkman: the player's buttons stand up out of its top edge as its keys,
//! a 7" single turns on the platter, one record per song with its title on
//! the label, and the arm moves in across it as the song plays. When the
//! song changes, the arm goes back to its rest, the record slides off, the
//! next one slides on and drops onto the spindle, and the arm comes back.

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use egui::{Align2, Color32};

use super::canvas::{Canvas, Face, blend, rgb, rgba};

/// The platter's centre and radius, and the record's, its label's and its
/// big hole's.
const PLATTER: (f32, f32) = (98.0, 102.0);
const PLATTER_R: f32 = 80.0;
const RECORD_R: f32 = 56.0;
const LABEL_R: f32 = 27.0;
const HOLE_R: f32 = 6.5;
/// The arm's pivot and length, pivot to needle.
const PIVOT: (f32, f32) = (262.0, 44.0);
const ARM: f32 = 158.0;
/// Where the arm rests, pointing down and to the left, in radians.
pub const REST: f32 = 115.0 * PI / 180.0;
/// How far a record slides to be off the platter.
pub const SLIDE: f32 = 230.0;
/// A label's colour for each song in turn.
const LABELS: [u32; 7] = [
    0xc8382a, 0xe8b422, 0x2a5ec8, 0xe6e0cc, 0x2f9a58, 0x8a3ab8, 0xe0682a,
];
const PRINT: Color32 = rgb(0x8a8e96);

/// A record: the song's title and which label colour it has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub title: String,
    pub label: usize,
}

/// A record where it's drawn: off to the left by `slide` units, and
/// `lifted` above the platter (0–1) while it drops.
pub struct Placed {
    pub record: Record,
    pub slide: f32,
    pub lifted: f32,
}

/// What a frame of the turntable shows.
#[derive(Default)]
pub struct Turntable {
    pub record: Option<Placed>,
    /// How far the record and the platter have turned, in radians.
    pub spin: f32,
    pub platter: f32,
    /// The arm's angle, and how far it's lifted (0–1).
    pub arm: f32,
    pub lift: f32,
}

/// The arm's angle with its needle `progress` of the way through the song:
/// from the record's edge to its run-out.
pub fn arm_for(progress: f32) -> f32 {
    let r = RECORD_R - 2.0 - progress.clamp(0.0, 1.0) * (RECORD_R - 2.0 - (LABEL_R + 4.0));
    let (dx, dy) = (PLATTER.0 - PIVOT.0, PLATTER.1 - PIVOT.1);
    let d = dx.hypot(dy);
    let cos = ((ARM * ARM + d * d - r * r) / (2.0 * ARM * d)).min(1.0);
    dy.atan2(dx) - cos.acos()
}

/// The body, `height` units tall from just under the keys' middle.
pub fn draw(c: &Canvas, height: f32, deck: &Turntable) {
    let (w, h) = (super::walkman::WIDTH, height);
    // The plinth: black metal, a fine speckle on it.
    c.shaded(
        0.0,
        0.0,
        w,
        h,
        10.0,
        &[(0.0, rgb(0x3a3b40)), (1.0, rgb(0x232428))],
    );
    for i in 0..600 {
        let (x, y) = ((i as f32 * 97.3) % w, (i as f32 * 53.7) % h);
        if y > 4.0 && y < h - 4.0 && x > 4.0 && x < w - 4.0 {
            let speck = if i % 2 == 0 {
                rgba(0xffffff, 0.04)
            } else {
                rgba(0x000000, 0.05)
            };
            c.rect(x, y, 1.0, 1.0, 0.0, speck);
        }
    }
    c.outline(0.0, 0.0, w, h, 10.0, 1.0, rgb(0x141416));
    // The slot the keys stand in.
    c.rect(2.0, -2.0, w - 4.0, 16.0, 4.0, rgb(0x18191c));

    // The platter: a polished rim with strobe dots, a black slip mat.
    let (px, py) = PLATTER;
    c.circle(px, py + 2.0, PLATTER_R + 1.0, rgba(0x000000, 0.4));
    c.sector(px, py, 0.0, PLATTER_R, 0.0, TAU, |x, y| {
        let t = ((y - py).atan2(x - px) / TAU).rem_euclid(1.0);
        let shine = (t * 2.0 * TAU).cos() * 0.5 + 0.5;
        blend(&[(0.0, rgb(0x8a9098)), (1.0, rgb(0xf2f4f6))], shine)
    });
    for row in 0..2 {
        let (n, r) = (90 + row * 6, PLATTER_R - 1.6 - row as f32 * 2.3);
        for i in 0..n {
            let a = deck.platter + i as f32 / n as f32 * TAU;
            c.circle(px + a.cos() * r, py + a.sin() * r, 0.75, rgb(0x30343a));
        }
    }
    c.circle(px, py, PLATTER_R - 6.0, rgb(0x151517));
    for turn in [0.0, PI] {
        let a = deck.platter + turn;
        let r = RECORD_R + 12.0;
        c.text_centred_at_angle(
            "S P O T L E S S",
            px + (a - PI / 2.0).cos() * r,
            py + (a - PI / 2.0).sin() * r,
            Face::Display,
            7.0,
            rgb(0x2e2f34),
            a,
            f32::MAX,
        );
    }
    c.circle(px, py, 1.8, rgb(0xc8ccd2));
    // The target light, glowing on the platter's edge.
    let (lx, ly) = (16.0, 26.0);
    c.sector(lx, ly, 0.0, 26.0, 0.0, TAU, |x, y| {
        let d = (x - lx).hypot(y - ly) / 26.0;
        rgba(0xffe9b0, 0.6 * (1.0 - d).max(0.0))
    });
    c.circle(lx, ly, 4.0, rgb(0xc8ccd2));
    c.circle(lx, ly, 2.0, rgb(0xfff6d8));

    // The record, sliding and dropping as it's changed.
    if let Some(placed) = &deck.record {
        let room = c.clipped(0.0, 0.0, w, h);
        let scale = 1.0 + 0.06 * placed.lifted;
        record(
            &room,
            px - placed.slide,
            py,
            scale,
            &placed.record,
            deck.spin,
        );
    }

    // The pitch fader, at its middle.
    let (fx, top, bottom) = (322.0, 40.0, 110.0);
    c.rect(fx - 2.0, top, 4.0, bottom - top, 0.0, rgb(0x121214));
    for i in 0..=8 {
        let y = top + (bottom - top) * i as f32 / 8.0;
        c.rect(fx - 9.0, y, 4.0, 0.8, 0.0, rgb(0x6a6e76));
    }
    for (mark, y) in [("+", top - 6.0), ("−", bottom + 7.0)] {
        c.text(
            mark,
            fx - 7.0,
            y,
            Face::Mono,
            7.0,
            PRINT,
            Align2::CENTER_CENTER,
        );
    }
    let knob = (top + bottom) / 2.0;
    c.rect(fx - 6.0, knob - 5.0, 12.0, 10.0, 0.0, rgb(0x2a2b2f));
    c.rect(fx - 6.0, knob - 0.6, 12.0, 1.2, 0.0, rgb(0xd8dce2));
    c.text(
        "PITCH",
        fx,
        bottom + 18.0,
        Face::Mono,
        6.0,
        PRINT,
        Align2::CENTER_CENTER,
    );
    // 33 and 45, 45 lit.
    for (i, speed) in ["33", "45"].into_iter().enumerate() {
        let (bx, by) = (252.0 + i as f32 * 26.0, 128.0);
        let lit = speed == "45";
        c.rect(bx, by, 22.0, 10.0, 2.0, rgb(0x18191c));
        c.text(
            speed,
            bx + 11.0,
            by + 5.0,
            Face::Mono,
            6.5,
            if lit { rgb(0xffb52b) } else { PRINT },
            Align2::CENTER_CENTER,
        );
        if lit {
            c.circle(bx + 11.0, by - 4.0, 1.4, rgb(0xff9a2e));
        }
    }
    c.text(
        "SPOTLESS",
        w - 12.0,
        h - 22.0,
        Face::Display,
        10.0,
        PRINT,
        Align2::RIGHT_CENTER,
    );
    c.text(
        "TT-90 · DIRECT DRIVE",
        w - 12.0,
        h - 10.0,
        Face::Mono,
        5.5,
        rgb(0x6e727a),
        Align2::RIGHT_CENTER,
    );

    arm(c, deck.arm, deck.lift);
}

/// A 7" single centred on `x, y`, `scale` times its size while it's
/// lifted: black vinyl, its grooves catching a light that doesn't turn,
/// and the label with the song, which does.
fn record(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, spin: f32) {
    let r = RECORD_R * scale;
    let lift = (scale - 1.0) * 40.0;
    c.circle(x + 1.0, y + 2.0 + lift, r + 1.0, rgba(0x000000, 0.45));
    c.circle(x, y, r, rgb(0x0b0b0d));
    let mut groove = r - 2.5;
    let mut i = 0;
    while groove > (LABEL_R + 4.0) * scale {
        let colour = if i % 3 == 0 {
            rgb(0x26262c)
        } else {
            rgb(0x1a1a1e)
        };
        c.ring(x, y, groove, 0.5, colour);
        groove -= 1.1;
        i += 1;
    }
    let sheen = [
        (0.0, rgba(0xffffff, 0.0)),
        (0.06, rgba(0xffffff, 0.19)),
        (0.12, rgba(0xffffff, 0.0)),
        (0.5, rgba(0xffffff, 0.0)),
        (0.56, rgba(0xffffff, 0.13)),
        (0.62, rgba(0xffffff, 0.0)),
        (1.0, rgba(0xffffff, 0.0)),
    ];
    c.sector(
        x,
        y,
        (LABEL_R + 1.0) * scale,
        r - 1.0,
        0.0,
        TAU,
        |px, py| {
            blend(
                &sheen,
                (((py - y).atan2(px - x) + FRAC_PI_4) / TAU).rem_euclid(1.0),
            )
        },
    );
    c.ring(x, y, r - 0.5, 0.8, rgb(0x3a3a42));
    label(c, x, y, scale, record, spin);
}

/// The label, turned to `spin`: its colour, the printing, the title under
/// the hole on up to two lines, and the yellow 45 adapter in the hole.
fn label(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, spin: f32) {
    let colour = LABELS[record.label % LABELS.len()];
    c.circle(x, y, LABEL_R * scale, rgb(colour));
    let ink = if matches!(colour, 0xe8b422 | 0xe6e0cc) {
        rgb(0x1a1a1a)
    } else {
        rgb(0xf4f0e6)
    };
    // A point on the label, `across` and `down` from its centre as it
    // turns.
    let at = |across: f32, down: f32| {
        let (sin, cos) = spin.sin_cos();
        let (u, v) = (across * scale, down * scale);
        (x + u * cos - v * sin, y + u * sin + v * cos)
    };
    let print = |text: &str, across: f32, down: f32, face: Face, size: f32| {
        let (px, py) = at(across, down);
        c.text_centred_at_angle(text, px, py, face, size * scale, ink, spin, f32::MAX);
    };
    print("SPOTLESS RECORDS", 0.0, -19.0, Face::Sans, 4.5);
    print("45 RPM", -17.0, -1.0, Face::Mono, 4.0);
    print("A", 17.0, -1.0, Face::Sans, 7.0);
    let lines = wrap(c, &record.title, 40.0, 7.0);
    let first = 10.0 - (lines.len() as f32 - 1.0);
    for (i, line) in lines.iter().enumerate() {
        print(line, 0.0, first + i as f32 * 8.0, Face::Sans, 7.0);
    }
    // The big hole, the adapter's three spokes and its ring, the spindle.
    let yellow = rgb(0xf0c020);
    c.circle(x, y, HOLE_R * scale, rgb(0x111111));
    c.circle(x, y, 3.2 * scale, yellow);
    for k in 0..3 {
        let a = spin + k as f32 * TAU / 3.0 - PI / 2.0;
        super::spoke(
            c,
            x,
            y,
            a,
            2.0 * scale,
            (HOLE_R - 0.5) * scale,
            3.2 * scale,
            yellow,
        );
    }
    c.ring(x, y, (HOLE_R - 0.6) * scale, scale, yellow);
    c.circle(x, y, 1.6 * scale, rgb(0xc8ccd2));
}

/// `text` in lines at most `width` units wide, two at most, the last cut
/// short with dots if the rest won't fit.
fn wrap(c: &Canvas, text: &str, width: f32, size: f32) -> Vec<String> {
    let fits = |line: &str| c.width(line, Face::Sans, size) <= width;
    let mut lines: Vec<String> = Vec::new();
    let mut cut = false;
    for word in text.split_whitespace() {
        let joined = lines.last().map(|last| format!("{last} {word}"));
        match joined {
            Some(joined) if fits(&joined) => *lines.last_mut().expect("a line") = joined,
            _ if lines.len() < 2 => lines.push(word.to_string()),
            _ => {
                cut = true;
                break;
            }
        }
    }
    let count = lines.len();
    for (i, line) in lines.iter_mut().enumerate() {
        if (cut && i + 1 == count) || !fits(line) {
            while !line.is_empty() && !fits(&format!("{}...", line.trim_end())) {
                line.pop();
            }
            *line = format!("{}...", line.trim_end());
        }
    }
    lines
}

/// The tonearm at `angle`, `lift` of the way up: its shadow, which falls
/// further while it's up, the S-shaped arm, the counterweight behind the
/// pivot, and the headshell.
fn arm(c: &Canvas, angle: f32, lift: f32) {
    let (px, py) = PIVOT;
    let rest = (px + REST.cos() * ARM, py + REST.sin() * ARM);
    c.circle(rest.0, rest.1, 4.0, rgb(0x18191c));
    c.sector(px, py, 0.0, 15.0, 0.0, TAU, |x, y| {
        let d = ((x - px + 4.0).hypot(y - py + 4.0) / 19.0).min(1.0);
        blend(&[(0.0, rgb(0x5a5e66)), (1.0, rgb(0x1c1d21))], d)
    });
    let (sin, cos) = angle.sin_cos();
    let needle = (px + cos * ARM, py + sin * ARM);
    let bend = (
        px + (angle + 0.1).cos() * ARM * 0.55,
        py + (angle + 0.1).sin() * ARM * 0.55,
    );
    let end = (needle.0 - cos * 10.0, needle.1 - sin * 10.0);
    let curve = |dx: f32, dy: f32| -> Vec<(f32, f32)> {
        (0..=16)
            .map(|i| {
                let t = i as f32 / 16.0;
                let u = 1.0 - t;
                (
                    u * u * px + 2.0 * u * t * bend.0 + t * t * end.0 + dx,
                    u * u * py + 2.0 * u * t * bend.1 + t * t * end.1 + dy,
                )
            })
            .collect()
    };
    let (sx, sy) = (3.0 + lift * 5.0, 4.0 + lift * 7.0);
    c.line(&curve(sx, sy), 4.0, rgba(0x000000, 0.3));
    c.line(&curve(0.0, 0.0), 2.6, rgb(0xdfe2e6));
    // The counterweight, ridged, behind the pivot.
    let (wx, wy) = (px - cos * 18.0, py - sin * 18.0);
    let corner = |s: f32, t: f32| (wx + cos * s - sin * t, wy + sin * s + cos * t);
    c.polygon(
        &[
            corner(-6.0, -6.0),
            corner(6.0, -6.0),
            corner(6.0, 6.0),
            corner(-6.0, 6.0),
        ],
        rgb(0x2a2b2f),
    );
    for s in [-4.0, -2.0, 0.0, 2.0, 4.0] {
        c.line(&[corner(s, -6.0), corner(s, 6.0)], 0.6, rgb(0x0a0a0c));
    }
    c.circle(px, py, 5.0, rgb(0xc8ccd2));
    // The headshell, turned off the arm's line as real ones are, its
    // finger lift and the stylus's body.
    let shell = angle + 0.35;
    let (hc, hs) = (shell.cos(), shell.sin());
    let (hx, hy) = (needle.0 - cos * 6.0, needle.1 - sin * 6.0);
    let piece = |x0: f32, y0: f32, x1: f32, y1: f32, dx: f32, dy: f32| {
        [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
            .map(|(s, t)| (hx + hc * s - hs * t + dx, hy + hs * s + hc * t + dy))
    };
    c.polygon(
        &piece(-7.0, -4.0, 9.0, 4.0, lift * 5.0, lift * 7.0),
        rgba(0x000000, 0.3),
    );
    c.polygon(&piece(-7.0, -4.0, 9.0, 4.0, 0.0, 0.0), rgb(0x1a1a1c));
    c.polygon(&piece(-2.0, -8.0, 0.0, -4.0, 0.0, 0.0), rgb(0xc8ccd2));
    c.polygon(&piece(2.0, -2.0, 8.0, 2.0, 0.0, 0.0), rgb(0xe8e8ea));
}
