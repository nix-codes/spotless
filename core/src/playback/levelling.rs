//! Volume levelling and the volume, applied to the audio before it goes
//! out.
//!
//! Levelling is ReplayGain's: each song is turned up or down by its own
//! gain, and a limiter shaves the peaks that would go past the threshold
//! (librespot's dynamic method, with its defaults). The song's own gain is
//! used, never its album's. The volume slider follows a cubic curve over
//! 60 dB, which spreads the useful range across it.

use std::time::Duration;

use super::{CHANNELS, SAMPLE_RATE};

/// Where the limiter starts to act.
const THRESHOLD_DB: f64 = -2.0;
/// How softly it starts to.
const KNEE_DB: f64 = 5.0;
const ATTACK: Duration = Duration::from_millis(5);
const RELEASE: Duration = Duration::from_millis(100);
/// The quietest the volume slider goes above silence.
const VOLUME_RANGE_DB: f64 = 60.0;

/// A song's ReplayGain values. Spotify keeps them in its own header at the
/// start of a file; other files have them as tags.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gains {
    pub track_gain_db: f64,
    pub track_peak: f64,
    pub album_gain_db: f64,
    pub album_peak: f64,
}

impl Default for Gains {
    fn default() -> Self {
        Self {
            track_gain_db: 0.0,
            track_peak: 1.0,
            album_gain_db: 0.0,
            album_peak: 1.0,
        }
    }
}

impl Gains {
    /// Where they are in a Spotify file: 16 bytes in, past the start of its
    /// header.
    pub const SPOTIFY_OFFSET: u64 = 144;
    pub const SPOTIFY_LEN: usize = 16;

    /// Read from Spotify's header: four little-endian floats.
    pub fn from_spotify(bytes: [u8; Self::SPOTIFY_LEN]) -> Self {
        let at = |i: usize| {
            f64::from(f32::from_le_bytes([
                bytes[i],
                bytes[i + 1],
                bytes[i + 2],
                bytes[i + 3],
            ]))
        };
        Self {
            track_gain_db: at(0),
            track_peak: at(4),
            album_gain_db: at(8),
            album_peak: at(12),
        }
    }
}

/// The limiter, with what it has measured so far. One for each player: it
/// carries on from one song to the next.
#[derive(Debug)]
pub struct Levelling {
    on: bool,
    /// The playing song's gain, as a ratio.
    factor: f64,
    integrators: [f64; 2],
    peaks: [f64; 2],
    /// The channel the next sample is on.
    channel: usize,
    attack: f64,
    release: f64,
}

impl Levelling {
    pub fn new(on: bool) -> Self {
        Self {
            on,
            factor: 1.0,
            integrators: [0.0; 2],
            peaks: [0.0; 2],
            channel: 0,
            attack: coefficient(ATTACK),
            release: coefficient(RELEASE),
        }
    }

    pub fn on(&self) -> bool {
        self.on
    }

    /// Turns it on or off from the next piece of audio.
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    /// For the song starting now.
    pub fn set_gains(&mut self, gains: Gains) {
        self.factor = db_to_ratio(gains.track_gain_db);
        log::debug!(
            "levelling: track gain {:.2} dB, peak {:.3}",
            gains.track_gain_db,
            gains.track_peak
        );
    }

    /// Levels `samples` (interleaved stereo) and turns them down by
    /// `volume`, the attenuation [`attenuation`] gives.
    pub fn apply(&mut self, samples: &mut [f64], volume: f64) {
        if !self.on {
            if volume < 1.0 {
                samples.iter_mut().for_each(|s| *s *= volume);
            }
            return;
        }
        let knee_factor = 1.0 / (8.0 * KNEE_DB);
        for sample in samples.iter_mut() {
            // A feedforward limiter in the log domain (Giannoulis, Massberg
            // and Reiss, 2012), as librespot has it.
            *sample *= self.factor;
            // Silence is -inf dB, which would hold the limiter stuck.
            *sample += f64::MIN_POSITIVE;
            let bias_db = ratio_to_db(sample.abs()) - THRESHOLD_DB;
            let knee_boundary_db = bias_db * 2.0;
            let limiter_db = if knee_boundary_db < -KNEE_DB {
                0.0
            } else if knee_boundary_db.abs() <= KNEE_DB {
                let term = knee_boundary_db + KNEE_DB;
                term * term * knee_factor
            } else {
                bias_db
            };
            let channel = self.channel;
            self.channel ^= 1;
            let integrator = &mut self.integrators[channel];
            *integrator = f64::max(
                limiter_db,
                self.release * *integrator + (1.0 - self.release) * limiter_db,
            );
            let peak = &mut self.peaks[channel];
            *peak = self.attack * *peak + (1.0 - self.attack) * *integrator;
            // Both channels turned down together, keeping the stereo image.
            let max_peak = f64::max(self.peaks[0], self.peaks[1]);
            *sample *= db_to_ratio(-max_peak) * volume;
        }
    }
}

/// The volume slider's position (0 to `u16::MAX`) as what the audio is
/// multiplied by.
pub fn attenuation(volume: u16) -> f64 {
    match volume {
        0 => 0.0,
        u16::MAX => 1.0,
        _ => {
            let min = 10f64.powf(-VOLUME_RANGE_DB / 60.0);
            let linear = f64::from(volume) / f64::from(u16::MAX);
            (linear * (1.0 - min) + min).powi(3)
        }
    }
}

fn db_to_ratio(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

fn ratio_to_db(ratio: f64) -> f64 {
    ratio.log10() * 20.0
}

/// A smoothing coefficient that takes `duration` to act, sample by sample.
fn coefficient(duration: Duration) -> f64 {
    let samples_per_second = f64::from(SAMPLE_RATE) * f64::from(CHANNELS);
    (-1.0 / (duration.as_secs_f64() * samples_per_second)).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_gains_are_four_little_endian_floats() {
        let mut bytes = [0; Gains::SPOTIFY_LEN];
        for (i, value) in [-6.5f32, 0.9, -7.0, 0.95].into_iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let gains = Gains::from_spotify(bytes);
        assert_eq!(gains.track_gain_db, -6.5);
        assert!((gains.track_peak - 0.9).abs() < 1e-6);
        assert_eq!(gains.album_gain_db, -7.0);
    }

    #[test]
    fn off_only_turns_the_volume_down() {
        let mut levelling = Levelling::new(false);
        levelling.set_gains(Gains {
            track_gain_db: -10.0,
            ..Gains::default()
        });
        let mut samples = [0.5, -0.5];
        levelling.apply(&mut samples, 0.5);
        assert_eq!(samples, [0.25, -0.25]);
    }

    #[test]
    fn a_quiet_song_is_turned_down_by_its_gain() {
        let mut levelling = Levelling::new(true);
        levelling.set_gains(Gains {
            track_gain_db: -6.0,
            ..Gains::default()
        });
        // Well under the threshold, the limiter leaves it be.
        let mut samples = vec![0.1; 2000];
        levelling.apply(&mut samples, 1.0);
        let expected = 0.1 * db_to_ratio(-6.0);
        assert!((samples[1999] - expected).abs() < 1e-6, "{}", samples[1999]);
    }

    #[test]
    fn the_limiter_keeps_a_loud_song_under_full_scale() {
        let mut levelling = Levelling::new(true);
        levelling.set_gains(Gains {
            track_gain_db: 6.0,
            ..Gains::default()
        });
        let mut samples = vec![0.9; 44_100];
        levelling.apply(&mut samples, 1.0);
        // Once the limiter has caught up, close to the threshold.
        let settled = samples[44_000];
        assert!(settled < 1.0, "{settled}");
        assert!(settled > db_to_ratio(THRESHOLD_DB - 1.0), "{settled}");
    }

    #[test]
    fn the_volume_curve_is_cubic_over_sixty_decibels() {
        assert_eq!(attenuation(0), 0.0);
        assert_eq!(attenuation(u16::MAX), 1.0);
        let half = attenuation(u16::MAX / 2);
        // (0.5 * 0.9 + 0.1)^3
        assert!((half - 0.55f64.powi(3)).abs() < 1e-4, "{half}");
        // Just above silence, about 60 dB down.
        assert!((ratio_to_db(attenuation(1)) + 60.0).abs() < 0.1);
    }
}
