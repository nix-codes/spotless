//! A portable CD player seen from above, 336 units wide and black like the
//! Walkman: the player's buttons stand up out of its top edge as its keys,
//! the disc turns under a round window in its lid, and an LCD beside it
//! shows the song and the time.
//!
//! The disc shows its shiny side, which a real player's window wouldn't:
//! that's where the music shows. It holds the whole playlist, each song a
//! ring from the centre out, and the laser's red dot moves out as it plays.
//! At 8cm, set by a switch beside the lights, a mini CD holds the song
//! instead, at its real size beside a full disc, and slides out of the
//! window for the next one's.

use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};

use egui::{Align2, Color32, Rect};

use super::Tape;
use super::canvas::{Canvas, Face, blend, rgb, rgba};
use crate::playback::engine::Status;

/// The disc's parts, as shares of its radius: the hole, the clear middle,
/// and where the music starts and the mirror ends.
const HOLE: f32 = 0.125;
const CLEAR: f32 = 0.33;
const INNER: f32 = 0.417;
const OUTER: f32 = 0.967;
/// A mini CD's edge and where its music ends: the same hole and middle on
/// an 8 cm disc.
const MINI: f32 = 40.0 / 60.0;
const MINI_OUTER: f32 = 38.5 / 60.0;
/// The playlist fills the disc but for this much spare.
const SPARE: f32 = 1.06;
/// How far a mini CD slides to be out of sight under the lid.
pub const SLIDE: f32 = 125.0;
const BLUE: Color32 = rgb(0x3d7bff);
const PRINT: Color32 = rgb(0x8a8e96);
const SPECTRUM: [u32; 7] = [
    0xff3d3d, 0xff9a2e, 0xffe83a, 0x5dff6a, 0x3ad8ff, 0x4a6bff, 0xb44aff,
];

/// What the disc and the LCD show.
#[derive(Clone, Default)]
pub struct Disc {
    /// Where each song ends, as a share of the playlist's time.
    pub ends: Vec<f32>,
    /// How much of the playlist has played.
    pub at: f32,
    /// The song's number in the playlist, from 1; 0 for none.
    pub number: usize,
    pub seconds: u32,
    pub status: Status,
    /// How far the disc has turned, in radians.
    pub angle: f32,
    /// A mini CD holding the song, `at` its share of it.
    pub mini: bool,
    /// How far it's slid to the right while it's changed, and whether
    /// it's being changed: no laser, and READ on the LCD.
    pub slide: f32,
    pub changing: bool,
}

impl Disc {
    /// Where the laser reads, as a share of a full disc's radius.
    pub fn reads_at(&self) -> f32 {
        radius(self.mini, self.at)
    }
}

/// Where `at` of what's on the disc lies, as a share of a full disc's
/// radius: the disc reads from the centre out at a steady speed, so by
/// area.
fn radius(mini: bool, at: f32) -> f32 {
    let outer = if mini { MINI_OUTER } else { OUTER };
    let share = at.clamp(0.0, 1.0) / SPARE;
    (INNER * INNER + share * (outer * outer - INNER * INNER)).sqrt()
}

/// The body, `height` units tall from just under the keys' middle;
/// returns where its 8cm / 12cm switch can be clicked.
pub fn draw(c: &Canvas, height: f32, tape: &Tape) -> Rect {
    let (w, h) = (super::walkman::WIDTH, height);
    c.shaded(
        0.0,
        0.0,
        w,
        h,
        18.0,
        &[(0.0, rgb(0x45484f)), (1.0, rgb(0x2a2c31))],
    );
    c.outline(0.0, 0.0, w, h, 18.0, 1.0, rgb(0x1a1b1f));
    // The slot the keys stand in, and the lid's seam.
    c.rect(2.0, -2.0, w - 4.0, 16.0, 4.0, rgb(0x18191c));
    c.outline(6.0, 18.0, w - 12.0, h - 24.0, 14.0, 1.5, rgb(0x1c1d21));

    // The window: a raised bezel round the dark tray, the disc, the
    // spindle's clamp, then smoke and a glare on the glass.
    let (wr, r) = (74.0, 70.0);
    let (cx, cy) = (94.0, 18.0 + (h - 24.0) / 2.0);
    c.sector(cx, cy, 0.0, wr + 4.0, 0.0, TAU, |x, y| {
        let t = ((x - cx) + (y - cy)) / (4.0 * wr) + 0.5;
        blend(&[(0.0, rgb(0x5a5e66)), (1.0, rgb(0x16171a))], t)
    });
    c.circle(cx, cy, wr, rgb(0x0c0d10));
    // The tray's ring for a mini CD.
    c.ring(cx, cy, (MINI * r) + 1.4, 1.2, rgb(0x1c1e23));
    c.ring(cx, cy, (MINI * r) + 0.5, 0.5, rgb(0x2a2d33));
    let body = [(0.0, rgb(0x45484f)), (1.0, rgb(0x2a2c31))];
    if tape.disc.slide == 0.0 {
        disc(c, cx, cy, r, &tape.disc);
    } else {
        // Sliding under the lid: drawn in the band it slides along, the
        // lid round the window laid back over it, and the bezel.
        let band = c.clipped(0.0, cy - wr, w, 2.0 * wr);
        disc(&band, cx + tape.disc.slide, cy, r, &tape.disc);
        band.sector(cx, cy, wr + 4.0, 3.0 * wr, 0.0, TAU, |_, y| {
            blend(&body, (y / h).clamp(0.0, 1.0))
        });
        band.outline(0.0, 0.0, w, h, 18.0, 1.0, rgb(0x1a1b1f));
        band.outline(6.0, 18.0, w - 12.0, h - 24.0, 14.0, 1.5, rgb(0x1c1d21));
        band.sector(cx, cy, wr, wr + 4.0, 0.0, TAU, |x, y| {
            let t = ((x - cx) + (y - cy)) / (4.0 * wr) + 0.5;
            blend(&[(0.0, rgb(0x5a5e66)), (1.0, rgb(0x16171a))], t)
        });
    }
    let clamp = r * 0.11;
    c.sector(cx, cy, 0.0, clamp, 0.0, TAU, |x, y| {
        let d = ((x - cx + 2.0).powi(2) + (y - cy + 2.0).powi(2)).sqrt() / clamp;
        blend(&[(0.0, rgb(0x5a5e66)), (1.0, rgb(0x1c1d21))], d)
    });
    for k in 0..3 {
        let a = tape.disc.angle + k as f32 * TAU / 3.0;
        let d = r * 0.075;
        c.circle(cx + a.cos() * d, cy + a.sin() * d, 1.4, rgb(0xc8ccd2));
    }
    c.circle(cx, cy, wr, rgba(0x0a0e1a, 0.22));
    c.sector(cx, cy, 0.0, wr, 0.0, TAU, |x, y| {
        let t = ((x - cx) + (y - cy)) / (2.6 * wr) + 0.5;
        if t < 0.42 {
            rgba(0xffffff, 0.13 - 0.25 * t)
        } else {
            Color32::TRANSPARENT
        }
    });
    c.ring(cx, cy, wr, 1.5, rgb(0x08090b));

    // Beside it: the name, the model, the LCD and the lights.
    let (px, pw) = (184.0, w - 16.0 - 184.0);
    c.outlined(
        "SPOTLESS",
        px,
        34.0,
        Face::Display,
        19.0,
        2.0,
        rgb(0x9da1a9),
        rgb(0x1c1d21),
    );
    c.line(&[(px, 50.0), (px + pw - 34.0, 50.0)], 2.0, BLUE);
    c.text(
        "D-90",
        px + pw,
        50.0,
        Face::Mono,
        10.0,
        rgb(0x9da1a9),
        Align2::RIGHT_CENTER,
    );
    c.text(
        "PORTABLE CD PLAYER",
        px,
        61.0,
        Face::Mono,
        7.0,
        PRINT,
        Align2::LEFT_CENTER,
    );
    lcd(c, px, 72.0, pw, 50.0, &tape.disc);
    // ESP lights while it plays; BASS stays off.
    for (x, on, colour, label) in [
        (px + 4.0, tape.playing, 0xff9a2e, "ESP"),
        (px + 54.0, false, 0xff5a3a, "BASS"),
    ] {
        if on {
            c.circle(x, 140.0, 6.0, rgba(colour, 0.25));
            c.circle(x, 140.0, 2.5, rgb(colour));
        } else {
            c.circle(x, 140.0, 2.5, rgb(0x3a2a24));
        }
        c.text(
            label,
            x + 8.0,
            140.0,
            Face::Mono,
            7.0,
            PRINT,
            Align2::LEFT_CENTER,
        );
    }
    c.text(
        "1-BIT DAC · 10 SEC ANTI-SKIP",
        px + pw,
        h - 20.0,
        Face::Mono,
        6.5,
        rgb(0x6e727a),
        Align2::RIGHT_CENTER,
    );
    super::walkman::slide_switch(c, px + pw - 20.0, 140.0, tape.cd_size, ["8cm", "12cm"])
}

/// The disc's shiny side, `r` units across from `cx, cy`.
fn disc(c: &Canvas, cx: f32, cy: f32, r: f32, disc: &Disc) {
    let angle = |x: f32, y: f32| (y - cy).atan2(x - cx);
    // The mirror: broad light and dark sectors, as a mirror shows the room.
    let mirror = [
        (0.0, rgb(0xeef1f5)),
        (0.12, rgb(0x9aa1ab)),
        (0.25, rgb(0x5d636d)),
        (0.38, rgb(0xa3aab3)),
        (0.5, rgb(0xf2f4f7)),
        (0.62, rgb(0xa0a7b0)),
        (0.75, rgb(0x5a606a)),
        (0.88, rgb(0xa6acb5)),
        (1.0, rgb(0xeef1f5)),
    ];
    let edge = if disc.mini { MINI * r } else { r };
    let radius = |at: f32| radius(disc.mini, at);
    c.sector(cx, cy, CLEAR * r, edge * 0.985, 0.0, TAU, |x, y| {
        blend(&mirror, ((angle(x, y) + FRAC_PI_4) / TAU).rem_euclid(1.0))
    });
    // A rainbow where there's music: thin rings whose colours sit at a
    // slightly different angle at each radius, so the streaks bend as on
    // a real disc. It stays put while the disc turns.
    let (inner, end) = (INNER * r, radius(1.0) * r);
    let mut colours = vec![(0.0, Color32::TRANSPARENT)];
    for (k, hex) in SPECTRUM.into_iter().enumerate() {
        colours.push((0.02 + k as f32 * 0.016, rgba(hex, 0.69)));
    }
    colours.push((0.14, Color32::TRANSPARENT));
    let rings = 24;
    for i in 0..rings {
        let r0 = inner + (end - inner) * i as f32 / rings as f32;
        let r1 = inner + (end - inner) * (i + 1) as f32 / rings as f32 + 0.6;
        let start = -FRAC_PI_4 - 0.35 + 0.7 * i as f32 / rings as f32;
        for half in [0.0, 0.5] {
            let from = start + half * TAU;
            c.sector(cx, cy, r0, r1, from, from + 0.14 * TAU, |x, y| {
                blend(&colours, (angle(x, y) - from).rem_euclid(TAU) / TAU)
            });
        }
    }
    // Music reads a shade darker than the plain mirror past it, and a
    // faint line parts the songs.
    c.sector(cx, cy, inner, end, 0.0, TAU, |_, _| rgba(0x1a1c24, 0.08));
    for (i, &share) in disc.ends.iter().enumerate() {
        let last = i + 1 == disc.ends.len();
        let colour = if last {
            rgba(0x30343c, 0.5)
        } else {
            rgba(0xffffff, 0.25)
        };
        c.ring(cx, cy, radius(share) * r, 0.7, colour);
    }
    // The see-through middle: a stacking ring and the hole.
    c.sector(cx, cy, HOLE * r, CLEAR * r, 0.0, TAU, |_, _| {
        rgba(0xc8d4e0, 0.19)
    });
    c.ring(cx, cy, 0.24 * r, 1.2, rgba(0xe8eef4, 0.5));
    c.ring(cx, cy, CLEAR * r, 1.0, rgba(0xffffff, 0.19));
    c.ring(cx, cy, HOLE * r, 1.0, rgba(0xffffff, 0.19));
    // The matrix code pressed round the middle: what shows it turning.
    // A mini CD's carries its song's number.
    let code = if disc.mini {
        format!("SPOTLESS-CD3 {:02} ▲ S{:03} ", disc.number, disc.number)
    } else {
        "SPOTLESS-CD 01 ▲ L042 ".to_string()
    };
    let code: Vec<char> = code.chars().collect();
    let at = (CLEAR + 0.045) * r;
    for (i, letter) in code.iter().enumerate() {
        let a = disc.angle + i as f32 / code.len() as f32 * TAU * 0.65 - FRAC_PI_2;
        c.text_centred_at_angle(
            &letter.to_string(),
            cx + a.cos() * at,
            cy + a.sin() * at,
            Face::Mono,
            5.0,
            rgb(0x4a505a),
            a + FRAC_PI_2,
            5.0,
        );
    }
    c.ring(cx, cy, edge, 1.0, rgba(0xffffff, 0.31));
    // The laser reads from this side: a red dot where the playlist has
    // got to.
    if disc.number > 0 && !disc.changing {
        let (x, y) = (cx + radius(disc.at) * r, cy);
        let strength = if disc.status == Status::Playing {
            1.0
        } else {
            0.45
        };
        c.circle(x, y, 7.0, rgba(0xff2a2a, 0.18 * strength));
        c.circle(x, y, 4.0, rgba(0xff2a2a, 0.45 * strength));
        c.circle(x, y, 1.8, rgba(0xff6a5a, strength));
    }
}

/// The grey-green LCD: the song's number and its time in big digits, the
/// unlit segments faintly there, and play, pause or stop, ESP and the
/// battery in small marks.
fn lcd(c: &Canvas, x: f32, y: f32, w: f32, h: f32, disc: &Disc) {
    let (on, off) = (rgb(0x1d2418), rgba(0x1d2418, 0.04));
    c.rect(x - 2.0, y - 2.0, w + 4.0, h + 4.0, 4.0, rgb(0x121316));
    c.shaded(
        x,
        y,
        w,
        h,
        2.0,
        &[(0.0, rgb(0xb3c09c)), (1.0, rgb(0x93a17e))],
    );
    let stopped = disc.status == Status::Stopped;
    let seconds = if stopped { 0 } else { disc.seconds };
    // While a disc is changed, it's read: no song yet.
    let number = match disc.number {
        _ if disc.changing => "--".to_string(),
        0 => "--".to_string(),
        n => format!("{n:02}"),
    };
    let time = if disc.changing {
        "--:--".to_string()
    } else {
        format!("{:>2}:{:02}", seconds / 60, seconds % 60)
    };
    let digits = |text: &str, x: f32, colour: Color32, align: Align2| {
        c.text(text, x, y + 22.0, Face::Mono, 22.0, colour, align);
    };
    digits("88", x + 8.0, off, Align2::LEFT_CENTER);
    digits(&number, x + 8.0, on, Align2::LEFT_CENTER);
    digits("88:88", x + w - 6.0, off, Align2::RIGHT_CENTER);
    digits(&time, x + w - 6.0, on, Align2::RIGHT_CENTER);
    let small = y + 41.0;
    c.text(
        if disc.changing { "READ" } else { "TRACK" },
        x + 8.0,
        small,
        Face::Sans,
        6.0,
        on,
        Align2::LEFT_CENTER,
    );
    let mark = x + 50.0;
    match disc.status {
        Status::Playing | Status::Loading => c.polygon(
            &[
                (mark - 2.0, small - 3.0),
                (mark + 3.0, small),
                (mark - 2.0, small + 3.0),
            ],
            on,
        ),
        Status::Paused => {
            c.rect(mark - 3.0, small - 3.0, 2.0, 6.0, 0.0, on);
            c.rect(mark + 1.0, small - 3.0, 2.0, 6.0, 0.0, on);
        }
        Status::Stopped => c.rect(mark - 3.0, small - 3.0, 6.0, 6.0, 0.0, on),
    }
    let esp = if disc.status == Status::Playing {
        on
    } else {
        off
    };
    c.text(
        "ESP",
        x + 64.0,
        small,
        Face::Sans,
        6.0,
        esp,
        Align2::LEFT_CENTER,
    );
    if disc.mini && !disc.changing {
        c.text(
            "8cm",
            x + 84.0,
            small,
            Face::Sans,
            6.0,
            on,
            Align2::LEFT_CENTER,
        );
    }
    // The battery, nearly full.
    let (bx, by) = (x + w - 22.0, y + 37.0);
    c.outline(bx, by, 14.0, 8.0, 0.0, 1.0, on);
    c.rect(bx + 14.0, by + 2.0, 2.0, 4.0, 0.0, on);
    for i in 0..3 {
        c.rect(bx + 2.0 + i as f32 * 4.0, by + 2.0, 3.0, 4.0, 0.0, on);
    }
}
