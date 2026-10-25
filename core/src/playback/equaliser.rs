//! The equaliser: a preamp and ten bands at Winamp's frequencies, each
//! from -12 to +12 dB, applied by the desktop's audio output (`sink.rs`)
//! as the audio plays.
//!
//! The settings are kept here for the output to read, as the chosen
//! output is (`outputs.rs`). Each band is a peaking filter (Robert
//! Bristow-Johnson's), one per band and channel; the output reads the
//! settings again for each piece of audio it plays, so a slider is heard
//! moving at once.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// The bands' centres, in Hz, as Winamp's.
pub const FREQUENCIES: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
/// How far a band or the preamp goes either way, in dB.
pub const RANGE: f32 = 12.0;
/// The bands' width: about an octave.
const Q: f64 = 1.41;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Equaliser {
    pub on: bool,
    /// dB, before the bands.
    pub preamp: f32,
    /// dB, by [`FREQUENCIES`].
    pub bands: [f32; 10],
}

impl Equaliser {
    /// Off, and flat.
    pub const OFF: Self = Self {
        on: false,
        preamp: 0.0,
        bands: [0.0; 10],
    };

    /// Within the range, as read from a file.
    pub fn clamped(self) -> Self {
        let clamp = |db: f32| {
            if db.is_finite() {
                db.clamp(-RANGE, RANGE)
            } else {
                0.0
            }
        };
        Self {
            on: self.on,
            preamp: clamp(self.preamp),
            bands: self.bands.map(clamp),
        }
    }
}

impl Default for Equaliser {
    fn default() -> Self {
        Self::OFF
    }
}

/// Bands saved under a name, by the user.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub bands: [f32; 10],
}

/// Winamp's presets, as XMMS kept them on its ±20 dB scale, brought to
/// Winamp's ±12.
const fn winamp(xmms: [f32; 10]) -> [f32; 10] {
    let mut bands = [0.0; 10];
    let mut n = 0;
    while n < 10 {
        bands[n] = (xmms[n] * 0.6 * 10.0).round() / 10.0;
        n += 1;
    }
    bands
}

/// Winamp's own presets.
pub const PRESETS: [(&str, [f32; 10]); 18] = [
    (
        "Classical",
        winamp([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -7.2, -7.2, -7.2, -9.6]),
    ),
    (
        "Club",
        winamp([0.0, 0.0, 8.0, 5.6, 5.6, 5.6, 3.2, 0.0, 0.0, 0.0]),
    ),
    (
        "Dance",
        winamp([9.6, 7.2, 2.4, 0.0, 0.0, -5.6, -7.2, -7.2, 0.0, 0.0]),
    ),
    ("Flat", [0.0; 10]),
    (
        "Full Bass",
        winamp([9.6, 9.6, 9.6, 5.6, 1.6, -4.0, -8.0, -10.4, -11.2, -11.2]),
    ),
    (
        "Full Bass & Treble",
        winamp([7.2, 5.6, 0.0, -7.2, -4.8, 1.6, 8.0, 11.2, 12.0, 12.0]),
    ),
    (
        "Full Treble",
        winamp([-9.6, -9.6, -9.6, -4.0, 2.4, 11.2, 16.0, 16.0, 16.0, 16.8]),
    ),
    (
        "Laptop Speakers",
        winamp([4.8, 11.2, 5.6, -3.2, -2.4, 1.6, 4.8, 9.6, 12.8, 14.4]),
    ),
    (
        "Large Hall",
        winamp([10.4, 10.4, 5.6, 5.6, 0.0, -4.8, -4.8, -4.8, 0.0, 0.0]),
    ),
    (
        "Live",
        winamp([-4.8, 0.0, 4.0, 5.6, 5.6, 5.6, 4.0, 2.4, 2.4, 2.4]),
    ),
    (
        "Party",
        winamp([7.2, 7.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 7.2, 7.2]),
    ),
    (
        "Pop",
        winamp([-1.6, 4.8, 7.2, 8.0, 5.6, 0.0, -2.4, -2.4, -1.6, -1.6]),
    ),
    (
        "Reggae",
        winamp([0.0, 0.0, 0.0, -5.6, 0.0, 6.4, 6.4, 0.0, 0.0, 0.0]),
    ),
    (
        "Rock",
        winamp([8.0, 4.8, -5.6, -8.0, -3.2, 4.0, 8.8, 11.2, 11.2, 11.2]),
    ),
    (
        "Ska",
        winamp([-2.4, -4.8, -4.0, 0.0, 4.0, 5.6, 8.8, 9.6, 11.2, 9.6]),
    ),
    (
        "Soft",
        winamp([4.8, 1.6, 0.0, -2.4, 0.0, 4.0, 8.0, 9.6, 11.2, 12.0]),
    ),
    (
        "Soft Rock",
        winamp([4.0, 4.0, 2.4, 0.0, -4.0, -5.6, -3.2, 0.0, 2.4, 8.8]),
    ),
    (
        "Techno",
        winamp([8.0, 5.6, 0.0, -5.6, -4.8, 0.0, 8.0, 9.6, 9.6, 8.8]),
    ),
];

static SETTINGS: Mutex<Equaliser> = Mutex::new(Equaliser::OFF);

/// What the audio output applies from now on.
pub fn set(equaliser: Equaliser) {
    *SETTINGS.lock().unwrap_or_else(|p| p.into_inner()) = equaliser;
}

fn current() -> Equaliser {
    *SETTINGS.lock().unwrap_or_else(|p| p.into_inner())
}

/// A band's filter: its coefficients, normalised.
#[derive(Clone, Copy, Debug, Default)]
struct Coefficients {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

impl Coefficients {
    fn peaking(frequency: f32, db: f32, rate: f64) -> Self {
        let a = 10f64.powf(f64::from(db) / 40.0);
        let w0 = std::f64::consts::TAU * f64::from(frequency) / rate;
        let alpha = w0.sin() / (2.0 * Q);
        let cos = w0.cos();
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

/// A filter's last two inputs and outputs, for one channel.
#[derive(Clone, Copy, Debug, Default)]
struct History {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

/// The equaliser for one stream of interleaved audio.
pub struct Processor {
    rate: f64,
    channels: usize,
    /// The settings the filters are made for; None while off.
    applied: Option<Equaliser>,
    /// The bands that change anything, with their filter's history per
    /// channel.
    bands: Vec<(Coefficients, Vec<History>)>,
    gain: f64,
}

impl Processor {
    pub fn new(rate: u32, channels: usize) -> Self {
        Self {
            rate: f64::from(rate),
            channels: channels.max(1),
            applied: None,
            bands: Vec::new(),
            gain: 1.0,
        }
    }

    /// Applies the settings in use now to `samples`, interleaved and
    /// starting on the first channel.
    pub fn process(&mut self, samples: &mut [f32]) {
        self.process_with(current(), samples);
    }

    fn process_with(&mut self, equaliser: Equaliser, samples: &mut [f32]) {
        if !equaliser.on {
            self.applied = None;
            return;
        }
        if self.applied != Some(equaliser) {
            self.retune(equaliser);
        }
        for frame in samples.chunks_mut(self.channels) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let mut x = f64::from(*sample) * self.gain;
                for (c, histories) in &mut self.bands {
                    let h = &mut histories[channel];
                    let y = c.b0 * x + c.b1 * h.x1 + c.b2 * h.x2 - c.a1 * h.y1 - c.a2 * h.y2;
                    *h = History {
                        x1: x,
                        x2: h.x1,
                        y1: y,
                        y2: h.y1,
                    };
                    x = y;
                }
                *sample = x.clamp(-1.0, 1.0) as f32;
            }
        }
    }

    /// New filters for new settings. A band that was already on keeps
    /// its history, so a slider moves without a click.
    fn retune(&mut self, equaliser: Equaliser) {
        let old = self.applied.map(|applied| applied.bands);
        let mut kept = std::mem::take(&mut self.bands).into_iter();
        let mut bands = Vec::new();
        for (n, (&frequency, &db)) in FREQUENCIES.iter().zip(&equaliser.bands).enumerate() {
            let was_on = old.is_some_and(|old| old[n].abs() >= 0.05);
            let history = if was_on {
                kept.next().map(|(_, history)| history)
            } else {
                None
            };
            if db.abs() < 0.05 {
                continue;
            }
            let history = history.unwrap_or_else(|| vec![History::default(); self.channels]);
            bands.push((Coefficients::peaking(frequency, db, self.rate), history));
        }
        self.bands = bands;
        self.gain = 10f64.powf(f64::from(equaliser.preamp) / 20.0);
        self.applied = Some(equaliser);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    /// A sine at `frequency`, stereo, half a second long.
    fn sine(frequency: f32) -> Vec<f32> {
        (0..RATE as usize / 2)
            .flat_map(|n| {
                let s = 0.25 * (std::f32::consts::TAU * frequency * n as f32 / RATE as f32).sin();
                [s, s]
            })
            .collect()
    }

    /// The level of the last half of `samples`, in dB against the input.
    fn gain(samples: &[f32], input: &[f32]) -> f32 {
        let rms = |s: &[f32]| (s.iter().map(|x| x * x).sum::<f32>() / s.len() as f32).sqrt();
        let half = samples.len() / 2;
        20.0 * (rms(&samples[half..]) / rms(&input[half..])).log10()
    }

    fn equalised(equaliser: Equaliser, frequency: f32) -> f32 {
        let input = sine(frequency);
        let mut samples = input.clone();
        Processor::new(RATE, 2).process_with(equaliser, &mut samples);
        gain(&samples, &input)
    }

    #[test]
    fn off_changes_nothing() {
        let mut bands = [0.0; 10];
        bands[0] = 12.0;
        let equaliser = Equaliser {
            on: false,
            preamp: 6.0,
            bands,
        };
        let input = sine(60.0);
        let mut samples = input.clone();
        Processor::new(RATE, 2).process_with(equaliser, &mut samples);
        assert_eq!(samples, input);
    }

    #[test]
    fn a_band_raises_and_lowers_its_frequency_only() {
        let mut bands = [0.0; 10];
        bands[3] = 6.0;
        let up = Equaliser {
            on: true,
            preamp: 0.0,
            bands,
        };
        assert!((equalised(up, 600.0) - 6.0).abs() < 0.3);
        assert!(equalised(up, 6000.0).abs() < 0.3);
        bands[3] = -6.0;
        let down = Equaliser { bands, ..up };
        assert!((equalised(down, 600.0) + 6.0).abs() < 0.3);
    }

    #[test]
    fn the_preamp_scales_everything() {
        let equaliser = Equaliser {
            on: true,
            preamp: -6.0,
            bands: [0.0; 10],
        };
        assert!((equalised(equaliser, 1000.0) + 6.0).abs() < 0.1);
    }

    #[test]
    fn loud_results_are_kept_within_full_scale() {
        let equaliser = Equaliser {
            on: true,
            preamp: 12.0,
            bands: [12.0; 10],
        };
        let mut samples = sine(1000.0);
        Processor::new(RATE, 2).process_with(equaliser, &mut samples);
        assert!(samples.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn presets_are_within_the_range() {
        for (name, bands) in PRESETS {
            assert!(bands.iter().all(|db| db.abs() <= RANGE), "{name}");
        }
        assert_eq!(PRESETS[13].0, "Rock");
        assert_eq!(PRESETS[13].1[0], 4.8);
    }

    #[test]
    fn settings_read_from_a_file_are_clamped() {
        let read: Equaliser = serde_json::from_str(
            r#"{"on": true, "preamp": 40, "bands": [-30, 1, 0, 0, 0, 0, 0, 0, 0, 0]}"#,
        )
        .unwrap();
        let read = read.clamped();
        assert_eq!(read.preamp, 12.0);
        assert_eq!(read.bands[..2], [-12.0, 1.0]);
        let missing: Equaliser = serde_json::from_str("{}").unwrap();
        assert_eq!(missing, Equaliser::OFF);
    }
}
