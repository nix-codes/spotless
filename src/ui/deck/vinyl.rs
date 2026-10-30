//! An '80s DJ turntable seen from above, 336 units wide and black like the
//! Walkman: the player's buttons stand up out of its top edge as its keys,
//! a 7" single turns on the platter, one record per song with its artists
//! and title on the label, and the arm moves in across it as the song
//! plays. When the song changes, the arm goes back to its rest, the record
//! slides off, the next one slides on and drops onto the spindle, and the
//! arm comes back.
//! At 33 a 12" LP holds the whole playlist instead, a band of grooves for
//! each song: the arm moves in across a side, and the record is turned
//! over for side B.

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use egui::{Align2, Color32, Rect, Shape, Stroke, vec2};

use super::canvas::{Canvas, Face, blend, rgb, rgba};

/// The platter's centre and radius, and the record's, its label's and its
/// big hole's.
const PLATTER: (f32, f32) = (98.0, 102.0);
const PLATTER_R: f32 = 80.0;
const RECORD_R: f32 = 64.0;
const LABEL_R: f32 = 40.0;
const HOLE_R: f32 = 6.5;
/// An LP's radius and its label's, and where its grooves start and how
/// far in the longest side reaches.
const LP_R: f32 = 74.0;
const LP_LABEL_R: f32 = 24.0;
const LP_OUT: f32 = 72.0;
const LP_RUN: f32 = 30.0;
/// The artists' size on the label and how wide their line can be, over
/// the hole; the title's, and how wide each of its lines can be where
/// they cross the label, under the hole.
const ARTIST_SIZE: f32 = 7.0;
const ARTIST_WIDTH: f32 = 50.0;
const TITLE_SIZE: f32 = 9.5;
const TITLE_WIDTHS: [f32; 2] = [62.0, 50.0];
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

/// A record: the song's artists and title, and which label colour it has;
/// an LP has the playlist's name as its title.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub artist: String,
    pub title: String,
    pub label: usize,
    pub lp: Option<Lp>,
}

/// The side of an LP showing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lp {
    pub side: char,
    pub songs: usize,
    pub seconds: u64,
    /// Where each song's grooves start, and the last one's end: from the
    /// edge (0) in to as far as the longer side reaches (1).
    pub marks: Vec<f32>,
}

impl Record {
    /// The same record on the platter: an LP's side stays the same one
    /// when songs are added to the playlist.
    pub fn same(&self, other: &Self) -> bool {
        match (&self.lp, &other.lp) {
            (Some(a), Some(b)) => {
                a.side == b.side && self.title == other.title && self.label == other.label
            }
            _ => self == other,
        }
    }
}

/// A record where it's drawn: off to the left by `slide` units,
/// `lifted` above the platter (0–1) while it drops, and `over` while it's
/// turned over, as its width shows it: 1 flat, through 0 on its edge.
pub struct Placed {
    pub record: Record,
    pub slide: f32,
    pub lifted: f32,
    pub over: f32,
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
    /// 33 is pressed: the playlist plays on an LP.
    pub lp: bool,
}

/// The arm's angle with its needle `progress` of the way through the song:
/// from the record's edge to its run-out.
pub fn arm_for(progress: f32) -> f32 {
    arm_at(RECORD_R - 2.0 - progress.clamp(0.0, 1.0) * (RECORD_R - 2.0 - (LABEL_R + 4.0)))
}

/// The arm's angle with its needle `at` of the way in on an LP, as its
/// marks are.
pub fn arm_on_lp(at: f32) -> f32 {
    arm_at(lp_radius(at))
}

fn lp_radius(at: f32) -> f32 {
    LP_OUT - at.clamp(0.0, 1.0) * (LP_OUT - LP_RUN)
}

/// The arm's angle with its needle `r` from the platter's centre.
fn arm_at(r: f32) -> f32 {
    let (dx, dy) = (PLATTER.0 - PIVOT.0, PLATTER.1 - PIVOT.1);
    let d = dx.hypot(dy);
    let cos = ((ARM * ARM + d * d - r * r) / (2.0 * ARM * d)).min(1.0);
    dy.atan2(dx) - cos.acos()
}

/// The body, `height` units tall from just under the keys' middle;
/// returns where its 33 and 45 can be clicked.
pub fn draw(c: &Canvas, height: f32, deck: &Turntable) -> [Rect; 2] {
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
    // The mat's print, hidden under an LP.
    let covered = deck
        .record
        .as_ref()
        .is_some_and(|placed| placed.record.lp.is_some() && placed.slide == 0.0);
    for turn in [0.0, PI] {
        if covered {
            break;
        }
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
        if placed.over < 0.999 {
            turning(&room, px, py, scale, &placed.record, placed.over);
        } else {
            record(
                &room,
                px - placed.slide,
                py,
                scale,
                &placed.record,
                deck.spin,
            );
        }
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
    // 33 and 45, the one it plays at lit.
    let mut buttons = [Rect::NOTHING; 2];
    for (i, speed) in ["33", "45"].into_iter().enumerate() {
        let (bx, by) = (252.0 + i as f32 * 26.0, 128.0);
        let lit = (i == 0) == deck.lp;
        buttons[i] = c.r(bx - 2.0, by - 8.0, 26.0, 20.0);
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
    buttons
}

/// A 7" single centred on `x, y`, `scale` times its size while it's
/// lifted: black vinyl, its grooves catching a light that doesn't turn,
/// and the label with the song, which does.
fn record(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, spin: f32) {
    let (radius, label_r) = match record.lp {
        Some(_) => (LP_R, LP_LABEL_R),
        None => (RECORD_R, LABEL_R),
    };
    let r = radius * scale;
    let lift = (scale - 1.0) * 40.0;
    c.circle(x + 1.0, y + 2.0 + lift, r + 1.0, rgba(0x000000, 0.45));
    c.circle(x, y, r, rgb(0x0b0b0d));
    // The grooves: one band on a single; on an LP one a song, smooth
    // gaps between them, and the run-out inside the last.
    let bands: Vec<(f32, f32)> = match &record.lp {
        Some(lp) => lp
            .marks
            .windows(2)
            .enumerate()
            .map(|(i, mark)| {
                let inset = if i == 0 { 0.0 } else { 0.7 };
                (
                    lp_radius(mark[0]) * scale - inset,
                    lp_radius(mark[1]) * scale + 0.7,
                )
            })
            .collect(),
        None => vec![(r - 2.5, (LABEL_R + 4.0) * scale)],
    };
    for (outer, inner) in bands {
        let mut groove = outer;
        let mut i = 0;
        while groove > inner {
            let colour = if i % 3 == 0 {
                rgb(0x26262c)
            } else {
                rgb(0x1a1a1e)
            };
            c.ring(x, y, groove, 0.5, colour);
            groove -= 1.1;
            i += 1;
        }
    }
    if let Some(lp) = &record.lp {
        let last = lp.marks.len().saturating_sub(1);
        for mark in lp.marks.iter().take(last).skip(1) {
            let gap = lp_radius(*mark) * scale;
            c.ring(x, y, gap, 1.6, rgb(0x030304));
            c.ring(x, y, gap + 0.6, 0.5, rgba(0xffffff, 0.16));
        }
        let end = lp.marks.last().map_or(LP_RUN, |end| lp_radius(*end)) * scale;
        c.circle(x, y, end - 0.7, rgb(0x0d0d10));
        c.ring(x, y, (LP_LABEL_R + 2.0) * scale, 0.4, rgb(0x26262c));
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
        (label_r + 1.0) * scale,
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
    match &record.lp {
        Some(lp) => lp_label(c, x, y, scale, record, lp, spin),
        None => label(c, x, y, scale, record, spin),
    }
}

/// A record being turned over, `over` as wide as it is: black vinyl, a
/// few grooves and the plain label, seen edge-on as it turns.
fn turning(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, over: f32) {
    let (radius, label_r) = match record.lp {
        Some(_) => (LP_R, LP_LABEL_R),
        None => (RECORD_R, LABEL_R),
    };
    let width = over.abs().max(0.02);
    let ellipse = |r: f32, dx: f32, dy: f32, colour: Color32| {
        c.painter.add(Shape::ellipse_filled(
            c.p(x + dx, y + dy),
            vec2(r * width, r) * c.k,
            colour,
        ));
    };
    let r = radius * scale;
    ellipse(r + 1.0, 3.0, 5.0, rgba(0x000000, 0.45));
    ellipse(r, 0.0, 0.0, rgb(0x0b0b0d));
    for ring in [0.9, 0.75, 0.6] {
        c.painter.add(Shape::ellipse_stroke(
            c.p(x, y),
            vec2(r * ring * width, r * ring) * c.k,
            Stroke::new(0.5 * c.k, rgb(0x26262c)),
        ));
    }
    ellipse(
        label_r * scale,
        0.0,
        0.0,
        rgb(LABELS[record.label % LABELS.len()]),
    );
}

/// An LP's label, turned to `spin`: the playlist's name over the hole,
/// 33⅓ and the side beside it, and how many songs and how long under it.
fn lp_label(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, lp: &Lp, spin: f32) {
    let colour = LABELS[record.label % LABELS.len()];
    c.circle(x, y, LP_LABEL_R * scale, rgb(colour));
    let ink = ink(colour);
    let print = |text: &str, across: f32, down: f32, face: Face, size: f32| {
        let (sin, cos) = spin.sin_cos();
        let (u, v) = (across * scale, down * scale);
        let (px, py) = (x + u * cos - v * sin, y + u * sin + v * cos);
        c.text_centred_at_angle(text, px, py, face, size * scale, ink, spin, f32::MAX);
    };
    print("SPOTLESS", 0.0, -16.0, Face::Display, 4.0);
    let name = wrap(c, &record.title, [34.0, 30.0], 6.0);
    let first = if name.len() > 1 { -10.0 } else { -8.0 };
    for (i, line) in name.iter().enumerate() {
        print(line, 0.0, first + i as f32 * 6.5, Face::Sans, 6.0);
    }
    print("33⅓", -13.0, 2.0, Face::Mono, 4.0);
    print(&lp.side.to_string(), 13.0, 2.0, Face::Sans, 7.0);
    let length = match lp.seconds {
        s if s >= 3600 => format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60),
        s => format!("{}:{:02}", s / 60, s % 60),
    };
    print(
        &format!("{} SONGS · {length}", lp.songs),
        0.0,
        11.0,
        Face::Mono,
        3.4,
    );
    print(&format!("SIDE {}", lp.side), 0.0, 16.0, Face::Mono, 3.4);
    c.circle(x, y, 1.9 * scale, rgb(0x111111));
    c.circle(x, y, 1.4 * scale, rgb(0xc8ccd2));
}

/// Dark ink on the light labels, light on the others.
fn ink(colour: u32) -> Color32 {
    if matches!(colour, 0xe8b422 | 0xe6e0cc) {
        rgb(0x1a1a1a)
    } else {
        rgb(0xf4f0e6)
    }
}

/// The label, turned to `spin`: its colour, the artists over the hole, the
/// printing, the title under the hole on up to two lines, and the yellow
/// 45 adapter in the hole.
fn label(c: &Canvas, x: f32, y: f32, scale: f32, record: &Record, spin: f32) {
    let colour = LABELS[record.label % LABELS.len()];
    c.circle(x, y, LABEL_R * scale, rgb(colour));
    let ink = ink(colour);
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
    let artist = c.cut(&record.artist, Face::Sans, ARTIST_SIZE, ARTIST_WIDTH);
    print(&artist, 0.0, -26.0, Face::Sans, ARTIST_SIZE);
    print("45 RPM", -24.0, -1.0, Face::Mono, 5.0);
    print("A", 24.0, -1.0, Face::Sans, 9.0);
    let lines = wrap(c, &record.title, TITLE_WIDTHS, TITLE_SIZE);
    let first = if lines.len() > 1 { 13.0 } else { 16.0 };
    for (i, line) in lines.iter().enumerate() {
        print(line, 0.0, first + i as f32 * 10.0, Face::Sans, TITLE_SIZE);
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

/// `text` in two lines at most, each no wider than its `widths`: the
/// second has the rest, cut short with dots if it won't fit.
fn wrap(c: &Canvas, text: &str, widths: [f32; 2], size: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        let second = lines.len() == 2;
        match lines.last_mut() {
            Some(last)
                if second || c.width(&format!("{last} {word}"), Face::Sans, size) <= widths[0] =>
            {
                last.push(' ');
                last.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
        .iter()
        .zip(widths)
        .map(|(line, width)| c.cut(line, Face::Sans, size, width))
        .collect()
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
