//! What the music playing sounds like right now: its spectrum in bands and
//! each channel's peak, for the desktop's deck to draw.
//!
//! The desktop's audio output (`sink.rs`) feeds each piece of audio here as
//! it starts playing, after the equaliser, with the volume taken back out
//! so the meters don't shrink with the volume slider. Reading measures
//! the last moments of it; audio stops being fed when nobody has read for
//! a while, so a hidden deck costs nothing.

use std::f32::consts::TAU;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// The spectrum's bands, a third of an octave or so each.
pub const BANDS: usize = 24;
/// What silence reads, in dB.
pub const SILENT: f32 = -120.0;
/// The lowest and highest frequencies the bands cover, in Hz.
const LOWEST: f32 = 40.0;
const HIGHEST: f32 = 18_000.0;
/// Frames measured for the spectrum: about 46 ms at 44.1 kHz.
const FRAMES: usize = 2048;
/// Frames measured for the peaks: about 23 ms.
const PEAK_FRAMES: usize = 1024;
/// Audio fed longer ago than this has finished playing.
const FRESH: Duration = Duration::from_millis(120);
/// Audio is fed only while the deck has read this recently.
const WATCHED: Duration = Duration::from_secs(1);

/// The bands in dB, a full-scale sine filling one band reading 0, and the
/// left and right peaks in dBFS.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub bands: [f32; BANDS],
    pub peaks: [f32; 2],
}

impl Reading {
    pub const SILENCE: Self = Self {
        bands: [SILENT; BANDS],
        peaks: [SILENT; 2],
    };
}

/// The last `FRAMES` frames played, left and right, oldest first from
/// `next`.
struct Recent {
    left: Vec<f32>,
    right: Vec<f32>,
    next: usize,
    fed: Option<Instant>,
    read: Option<Instant>,
}

static RECENT: Mutex<Recent> = Mutex::new(Recent {
    left: Vec::new(),
    right: Vec::new(),
    next: 0,
    fed: None,
    read: None,
});

/// Keeps a piece of interleaved stereo audio as it starts playing. `gain`
/// undoes the volume it was played at. Skipped while nobody reads, and
/// rather than wait for a reader, so audio never waits.
pub fn feed(samples: &[f32], gain: f32) {
    let Ok(mut recent) = RECENT.try_lock() else {
        return;
    };
    if !recent.read.is_some_and(|at| at.elapsed() < WATCHED) {
        return;
    }
    if recent.left.len() != FRAMES {
        recent.left = vec![0.0; FRAMES];
        recent.right = vec![0.0; FRAMES];
        recent.next = 0;
    }
    for frame in samples.as_chunks::<2>().0 {
        let at = recent.next;
        recent.left[at] = frame[0] * gain;
        recent.right[at] = frame[1] * gain;
        recent.next = (at + 1) % FRAMES;
    }
    recent.fed = Some(Instant::now());
}

/// The music as it sounds now; silence when nothing has played lately.
pub fn read() -> Reading {
    let (left, right) = {
        let mut recent = RECENT.lock().unwrap_or_else(|p| p.into_inner());
        recent.read = Some(Instant::now());
        if !recent.fed.is_some_and(|at| at.elapsed() < FRESH) {
            return Reading::SILENCE;
        }
        let mut left = Vec::with_capacity(FRAMES);
        let mut right = Vec::with_capacity(FRAMES);
        let next = recent.next;
        left.extend_from_slice(&recent.left[next..]);
        left.extend_from_slice(&recent.left[..next]);
        right.extend_from_slice(&recent.right[next..]);
        right.extend_from_slice(&recent.right[..next]);
        (left, right)
    };
    measure(&left, &right, librespot_playback::SAMPLE_RATE as f32)
}

/// The middle of a band, in Hz.
pub fn centre(band: usize) -> f32 {
    edge(band as f32 + 0.5)
}

/// Where a frequency falls across the bands, from 0 at their low end to
/// `BANDS` at their high end.
pub fn place(hz: f32) -> f32 {
    (hz / LOWEST).ln() / (HIGHEST / LOWEST).ln() * BANDS as f32
}

fn edge(band: f32) -> f32 {
    LOWEST * (HIGHEST / LOWEST).powf(band / BANDS as f32)
}

fn decibels(power: f32) -> f32 {
    if power > 0.0 {
        (10.0 * power.log10()).max(SILENT)
    } else {
        SILENT
    }
}

/// Measures `FRAMES` frames of left and right at `rate` Hz.
fn measure(left: &[f32], right: &[f32], rate: f32) -> Reading {
    let peak = |channel: &[f32]| {
        let peak = channel[channel.len() - PEAK_FRAMES..]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
        decibels(peak * peak)
    };
    let window = window();
    let mut re: Vec<f32> = left
        .iter()
        .zip(right)
        .zip(window)
        .map(|((l, r), w)| (l + r) / 2.0 * w)
        .collect();
    let mut im = vec![0.0; FRAMES];
    fft(&mut re, &mut im);
    // A full-scale sine's bin, through the Hann window, is FRAMES / 4,
    // and the window spreads its power over 1.5 bins' worth.
    let full = (FRAMES as f32 / 4.0).powi(2) * 1.5;
    let hz_per_bin = rate / FRAMES as f32;
    let mut bands = [SILENT; BANDS];
    for (band, value) in bands.iter_mut().enumerate() {
        let low = (edge(band as f32) / hz_per_bin).round() as usize;
        let high = ((edge(band as f32 + 1.0) / hz_per_bin).round() as usize).max(low + 1);
        let power: f32 = (low..high.min(FRAMES / 2))
            .map(|bin| re[bin] * re[bin] + im[bin] * im[bin])
            .sum();
        *value = decibels(power / full);
    }
    Reading {
        bands,
        peaks: [peak(left), peak(right)],
    }
}

fn window() -> &'static [f32] {
    static WINDOW: OnceLock<Vec<f32>> = OnceLock::new();
    WINDOW.get_or_init(|| {
        (0..FRAMES)
            .map(|n| 0.5 - 0.5 * (TAU * n as f32 / FRAMES as f32).cos())
            .collect()
    })
}

/// An in-place radix-2 FFT; the length must be a power of two.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -TAU / len as f32;
        let (step_re, step_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(len) {
            let (mut w_re, mut w_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                (w_re, w_im) = (
                    w_re * step_re - w_im * step_im,
                    w_re * step_im + w_im * step_re,
                );
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amplitude: f32) -> Vec<f32> {
        (0..FRAMES)
            .map(|n| amplitude * (TAU * hz * n as f32 / 44_100.0).sin())
            .collect()
    }

    #[test]
    fn a_tone_lights_its_own_band() {
        let tone = sine(1000.0, 0.5);
        let reading = measure(&tone, &tone, 44_100.0);
        let band = (0..BANDS)
            .max_by(|a, b| reading.bands[*a].total_cmp(&reading.bands[*b]))
            .unwrap();
        assert!((centre(band) / 1000.0).log2().abs() < 0.25, "{band}");
        // Half scale is -6 dB.
        assert!((reading.bands[band] + 6.0).abs() < 0.5, "{reading:?}");
        assert!(reading.bands[0] < -60.0, "{reading:?}");
        assert!((reading.peaks[0] + 6.0).abs() < 0.1);
    }

    #[test]
    fn silence_reads_silent() {
        let quiet = vec![0.0; FRAMES];
        assert_eq!(measure(&quiet, &quiet, 44_100.0), Reading::SILENCE);
    }

    #[test]
    fn audio_is_kept_only_while_read() {
        feed(&[0.5; 64], 1.0);
        assert_eq!(read(), Reading::SILENCE);
        feed(&[0.5; 64], 2.0);
        assert!((read().peaks[0]).abs() < 0.1);
    }
}
