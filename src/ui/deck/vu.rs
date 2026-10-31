//! The deck's VU meters, one for each channel, which clicking the
//! spectrum puts in its place: a needle on a real VU meter's scale, 20 to
//! +3, red from 0 up, swinging as a real one does, and a PEAK light. The
//! face is black glass in the display's colour, cream or blue.

use std::f32::consts::{FRAC_PI_2, TAU};

use egui::{Align2, Color32, Painter, Rect, pos2};

use super::canvas::{Canvas, Face, blend, rgb, rgba};
use crate::store::{VuCalibration, VuFace};

/// AUTO: the music's loudness (RMS, dBFS) averaged over about this many
/// seconds reads `AUTO_BELOW` under 0 VU, its 0 VU kept within `AUTO_RANGE`.
const AUTO_SECONDS: f32 = 6.0;
const AUTO_BELOW: f32 = 4.0;
const AUTO_RANGE: (f32, f32) = (-24.0, -4.0);
/// AUTO's average before any music, which puts 0 VU at −14 dBFS.
pub const AUTO_START: f32 = -18.0;
/// The peak (dBFS) that lights PEAK.
pub const PEAK: f32 = -1.0;
/// Where the needle rests, against its stop below 20.
pub const REST: f32 = -0.04;
/// How the needle swings: its spring (radians a second) and damping,
/// which settle it in about 300 ms with a little overshoot.
const SPRING: f32 = 13.5;
const DAMPING: f32 = 0.8;

/// The scale's marks, in VU, and those with a number.
const MARKS: [f32; 11] = [
    -20.0, -10.0, -7.0, -5.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0,
];
const NUMBERED: [f32; 10] = [-20.0, -10.0, -7.0, -5.0, -3.0, -1.0, 0.0, 1.0, 2.0, 3.0];
/// A meter's height in its units, which scale to the room it gets.
const HEIGHT: f32 = 127.0;
/// Between the two meters, in points.
const GAP: f32 = 8.0;

/// Where `vu` falls along the scale, 0 at 20 to 1 at +3: spaced by
/// voltage, as the needle's coil spaces it.
pub fn place(vu: f32) -> f32 {
    let volts = |db: f32| 10f32.powf(db / 20.0);
    (volts(vu) - volts(-20.0)) / (volts(3.0) - volts(-20.0))
}

/// Where the needle heads for `rms` (dBFS) when `reference` reads 0 VU;
/// at rest in silence.
pub fn target(rms: f32, reference: f32) -> f32 {
    if rms <= crate::playback::analyser::SILENT {
        return REST;
    }
    place((rms - reference).clamp(-30.0, 4.0)).max(REST)
}

/// Moves AUTO's `average` loudness on by `dt` seconds of `rms`; silence
/// leaves it as it was.
pub fn follow(average: &mut f32, rms: [f32; 2], dt: f32) {
    let loudest = rms[0].max(rms[1]);
    if loudest > crate::playback::analyser::SILENT {
        *average += (loudest - *average) * (1.0 - (-dt / AUTO_SECONDS).exp());
    }
}

/// The loudness (dBFS) that reads 0 VU at `calibration`, AUTO's from its
/// `average`.
pub fn reference(calibration: VuCalibration, average: f32) -> f32 {
    calibration
        .reference()
        .unwrap_or_else(|| (average + AUTO_BELOW).clamp(AUTO_RANGE.0, AUTO_RANGE.1))
}

/// Moves a needle at `at`, going at `speed`, on towards `target` by `dt`
/// seconds.
pub fn swing(at: &mut f32, speed: &mut f32, target: f32, dt: f32) {
    let steps = 4;
    let h = dt / steps as f32;
    for _ in 0..steps {
        let pull = SPRING * SPRING * (target - *at) - 2.0 * DAMPING * SPRING * *speed;
        *speed += pull * h;
        *at += *speed * h;
    }
}

struct Look {
    top: Color32,
    bottom: Color32,
    glow: Color32,
    ink: Color32,
    faint: Color32,
    red: Color32,
    needle: Color32,
    legend: Color32,
    face: Face,
}

fn look(face: VuFace, display: Color32) -> Look {
    match face {
        VuFace::Display => Look {
            top: rgb(0x14181b),
            bottom: rgb(0x06080a),
            glow: display.gamma_multiply(0.07),
            ink: display,
            faint: display.gamma_multiply(0.45),
            red: rgb(0xff3b2b),
            needle: rgb(0xeef6f0),
            legend: display.gamma_multiply(0.3),
            face: Face::Mono,
        },
        VuFace::Cream => Look {
            top: rgb(0xf6dfaa),
            bottom: rgb(0xdcb06a),
            glow: rgba(0xfff6dc, 0.8),
            ink: rgb(0x1d1a14),
            faint: rgb(0x6d5a3a),
            red: rgb(0xc8261b),
            needle: rgb(0x121212),
            legend: rgba(0x1d1a14, 0.85),
            face: Face::Sans,
        },
        VuFace::Blue => Look {
            top: rgb(0x2f6fd0),
            bottom: rgb(0x0f3a99),
            glow: rgba(0x64a6ff, 0.75),
            ink: Color32::WHITE,
            faint: rgb(0xbcd3ff),
            red: rgb(0xff5a3c),
            needle: rgb(0xffb02e),
            legend: rgba(0xffffff, 0.88),
            face: Face::Sans,
        },
    }
}

/// Draws the left and right meters side by side in `rect`, their needles
/// `at` along the scale, their PEAK lights `lit`, and the loudness that
/// reads 0 VU, `reference` (dBFS).
pub fn draw(
    painter: &Painter,
    rect: Rect,
    face: VuFace,
    display: Color32,
    at: [f32; 2],
    lit: [bool; 2],
    reference: f32,
) {
    let look = look(face, display);
    let width = (rect.width() - GAP) / 2.0;
    for side in 0..2 {
        let left = rect.left() + side as f32 * (width + GAP);
        let k = rect.height() / HEIGHT;
        let canvas = Canvas {
            painter: painter.clone(),
            origin: pos2(left, rect.top()),
            k,
        };
        meter(
            &canvas,
            width / k,
            &look,
            side,
            at[side],
            lit[side],
            reference,
        );
    }
}

/// One meter, `w` units wide.
fn meter(c: &Canvas, w: f32, look: &Look, side: usize, at: f32, lit: bool, reference: f32) {
    c.rect(0.0, 0.0, w, HEIGHT, 3.0, rgb(0x25282e));
    let (fx, fy, fw, fh) = (4.0, 4.0, w - 8.0, HEIGHT - 8.0);
    let face = c.clipped(fx, fy, fw, fh);
    face.shaded(fx, fy, fw, fh, 2.0, &[(0.0, look.top), (1.0, look.bottom)]);
    // The lamp behind the face, brightest under the scale.
    let (gx, gy, gr) = (fx + fw / 2.0, fy + fh * 0.5, fw * 0.55);
    let fade = [(0.0, look.glow), (1.0, Color32::TRANSPARENT)];
    for ring in 0..6 {
        let (r0, r1) = (ring as f32 / 6.0 * gr, (ring + 1) as f32 / 6.0 * gr);
        face.sector(gx, gy, r0, r1, 0.0, TAU, |x, y| {
            blend(&fade, ((x - gx).hypot(y - gy) / gr).min(1.0))
        });
    }

    // The needle's pivot is below the face; the scale is an arc round it,
    // its middle at `crown` and its ends at `ends`, half `reach` apart,
    // its numbers clear of the face's sides however wide the face is.
    let cx = fx + fw / 2.0;
    let (crown, ends) = (fy + fh * 0.32, fy + fh * 0.74);
    let sag = ends - crown;
    let radius = |reach: f32| (reach * reach + sag * sag) / (2.0 * sag);
    let mut reach = (fw * 0.4).min(fh * 1.25);
    let room = fw / 2.0 - 14.0;
    let r = radius(reach);
    if reach * (r + 16.0) / r > room {
        reach *= room / (reach * (r + 16.0) / r);
    }
    let r = radius(reach);
    let py = crown + r;
    let span = (reach / r).min(1.0).asin();
    let angle = |p: f32| -span + p * 2.0 * span - FRAC_PI_2;
    let point = |p: f32, radius: f32| {
        let a = angle(p);
        (cx + a.cos() * radius, py + a.sin() * radius)
    };
    let arc = |from: f32, to: f32, radius: f32| -> Vec<(f32, f32)> {
        (0..=24)
            .map(|i| point(from + (to - from) * i as f32 / 24.0, radius))
            .collect()
    };
    let zero = place(0.0);
    face.line(&arc(0.0, zero, r), 1.2, look.ink);
    face.line(&arc(zero, 1.0, r), 1.2, look.red);
    face.line(&arc(zero, 1.0, r + 3.0), 4.5, look.red);
    let size = 11.0;
    for vu in MARKS {
        let p = place(vu);
        let numbered = NUMBERED.contains(&vu);
        let colour = if vu > 0.0 { look.red } else { look.ink };
        let width = if vu == 0.0 { 1.8 } else { 1.2 };
        let long = if numbered { 7.0 } else { 5.0 };
        face.line(&[point(p, r), point(p, r + long)], width, colour);
        if numbered {
            let (tx, ty) = point(p, r + 16.0);
            let label = if vu > 0.0 {
                format!("+{vu}")
            } else {
                format!("{}", vu.abs())
            };
            face.text(
                &label,
                tx,
                ty,
                look.face,
                size,
                colour,
                Align2::CENTER_CENTER,
            );
        }
    }
    // The percent scale's ticks, under the arc.
    for tenth in 1..=10 {
        let p = place(20.0 * (0.1 * tenth as f32).log10());
        if (0.0..=1.0).contains(&p) {
            face.line(&[point(p, r - 2.0), point(p, r - 5.0)], 1.0, look.faint);
        }
    }
    face.text(
        "VU",
        cx,
        fy + fh * 0.72,
        look.face,
        20.0,
        look.legend,
        Align2::CENTER_CENTER,
    );
    let name = if side == 0 { "LEVEL L" } else { "LEVEL R" };
    face.text(
        name,
        cx,
        fy + fh - 9.0,
        Face::Sans,
        8.0,
        look.faint,
        Align2::CENTER_CENTER,
    );
    face.text(
        &format!("0 VU {:.0} dBFS", reference),
        fx + fw - 8.0,
        fy + fh - 9.0,
        Face::Sans,
        8.0,
        look.faint,
        Align2::RIGHT_CENTER,
    );
    face.text(
        "PEAK",
        fx + fw - 18.0,
        fy + 11.0,
        Face::Sans,
        8.0,
        look.faint,
        Align2::RIGHT_CENTER,
    );
    let (lx, ly) = (fx + fw - 10.0, fy + 11.0);
    if lit {
        face.circle(lx, ly, 6.0, rgba(0xff3b2b, 0.3));
        face.circle(lx, ly, 3.5, rgb(0xff4a33));
    } else {
        face.circle(lx, ly, 3.5, rgba(0x50281e, 0.6));
    }

    // The needle and its shadow on the face.
    let p = at.clamp(REST, 1.04);
    let tip = point(p, r + 6.0);
    let foot = point(p, r - fh * 0.58);
    face.line(
        &[(foot.0 + 2.0, foot.1 + 3.0), (tip.0 + 2.0, tip.1 + 3.0)],
        1.4,
        rgba(0x000000, 0.18),
    );
    face.line(&[foot, tip], 1.6, look.needle);

    // The glass over it all.
    face.shaded(
        fx,
        fy,
        fw,
        fh,
        2.0,
        &[
            (0.0, rgba(0xffffff, 0.1)),
            (0.35, rgba(0xffffff, 0.02)),
            (1.0, rgba(0x000000, 0.18)),
        ],
    );
    c.outline(fx, fy, fw, fh, 2.0, 1.0, rgb(0x0a0b0d));
}

#[cfg(test)]
mod tests {
    use super::{AUTO_START, REST, place, reference, target};
    use crate::store::VuCalibration;

    #[test]
    fn the_scale_is_spaced_as_a_vu_meter() {
        assert!(place(-20.0).abs() < 1e-6);
        assert!((place(3.0) - 1.0).abs() < 1e-6);
        // 0 VU sits about two thirds of the way along.
        assert!((place(0.0) - 0.685).abs() < 0.005);
        assert!(place(-10.0) < 0.2);
    }

    #[test]
    fn silence_rests_the_needle() {
        assert_eq!(target(crate::playback::analyser::SILENT, -14.0), REST);
        assert!((target(-14.0, -14.0) - place(0.0)).abs() < 1e-6);
        assert_eq!(target(-80.0, -14.0), REST);
    }

    #[test]
    fn auto_follows_the_music() {
        assert_eq!(reference(VuCalibration::Auto, AUTO_START), -14.0);
        let mut average = AUTO_START;
        // A loud master, a minute of it: 0 VU as high as AUTO goes.
        for _ in 0..3600 {
            super::follow(&mut average, [-7.0, -8.0], 1.0 / 60.0);
        }
        assert!((reference(VuCalibration::Auto, average) + 4.0).abs() < 0.1);
        // Silence keeps it.
        super::follow(&mut average, [-120.0; 2], 10.0);
        assert!((average + 7.0).abs() < 0.1);
        assert_eq!(reference(VuCalibration::Minus18, average), -18.0);
    }
}
