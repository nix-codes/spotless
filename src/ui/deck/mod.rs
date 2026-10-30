//! The deck under the player (VIS): the player's buttons become the keys
//! of a Walkman, a reel-to-reel deck, a portable CD player or a turntable,
//! which plays the song on a cassette, open reels, a disc or a single,
//! with the music's spectrum and its left and right levels beside it.
//!
//! The Walkman's, the CD player's and the turntable's keys stay where the
//! buttons always are, standing out of their top edge; the reel deck has
//! its keys along its bottom, as real ones do. Clicking the device
//! switches to the next.
//! The tape winds from the left reel to the right one as the song plays,
//! and back fast when another starts; the reels turn only while it plays.
//! The Walkman's switch (MIX) puts the whole playlist on the cassette
//! instead: side A left to right, then auto reverse plays side B back; the
//! reel deck's lever at 3¾ IPS puts it on the reels, in one pass at half
//! the speed.
//! The disc holds the whole playlist and turns while it plays, faster the
//! nearer the centre the laser reads. A single holds one song, and is
//! changed for the next one's when the song changes; at 33 an LP holds
//! the whole playlist, and is turned over for side B.

mod canvas;
mod cassette;
mod cd;
mod reel;
mod vinyl;
mod walkman;

pub use canvas::install_fonts;

use egui::{Align2, Color32, FontId, Painter, Rect, Sense, Ui, pos2, vec2};

use super::{Inputs, View, player_strip, theme};
use crate::action::{Action, Actions};
use crate::model::{PlaylistId, TrackRef};
use crate::playback::analyser::{self, BANDS, Reading};
use crate::playback::engine::Status;
use crate::store::DeckMedia;
use canvas::{Canvas, rgb};

/// Points round the keys, beside them and above and below.
const SIDE: f32 = 4.0;
const MARGIN: f32 = 6.0;
/// Points between the device and the spectrum.
const GAP: f32 = 10.0;
/// The Walkman's body below the keys' band, in its units.
const WALKMAN_BODY: f32 = 172.0;
/// The levels' box under the spectrum, in points.
const LEVELS_HEIGHT: f32 = 38.0;
/// The bars span this many dB: a band at TOP fills its bar.
const BAR_FLOOR: f32 = -60.0;
const BAR_TOP: f32 = -6.0;
/// The levels span -40 to 0 dBFS, in LEDs of a dB each.
const LEVEL_FLOOR: f32 = -40.0;
const LEVEL_LEDS: usize = 40;
const SEGMENTS: usize = 16;
/// How fast the tape moves, in the cassette's units per second; the reels
/// turn by it, faster the less tape they hold.
const TAPE_SPEED: f32 = 48.0;
/// How fast it winds to a new place, in tapes per second.
const WIND_SPEED: f32 = 1.6;
/// How fast the disc turns, in radians per second where the laser reads at
/// its whole radius; nearer the centre, faster.
const DISC_SPEED: f32 = 2.4 * 0.417;
/// 45 rpm and 33⅓, in radians per second.
const SINGLE_SPEED: f32 = 45.0 / 60.0 * std::f32::consts::TAU;
const LP_SPEED: f32 = 100.0 / 3.0 / 60.0 * std::f32::consts::TAU;
/// When a record change's steps end, in seconds: the arm back at its rest,
/// the old record off, the new one on the spindle, the arm on the record.
const ARM_BACK: f32 = 0.35;
const RECORD_OFF: f32 = 0.75;
const RECORD_ON: f32 = 1.15;
const ARM_DOWN: f32 = 1.6;
/// When an LP's turning over ends its steps, in seconds: the arm back, the
/// record up, over, down on the platter, the arm on it.
const LIFTED: f32 = 0.6;
const TURNED: f32 = 1.3;
const DROPPED: f32 = 1.55;
const FLIP_DOWN: f32 = 2.0;
/// The standard cassettes, in minutes for both sides.
const CASSETTES: [u32; 4] = [46, 60, 90, 120];
const GREEN: Color32 = rgb(0x33e65f);
const YELLOW: Color32 = rgb(0xffd02b);
const RED: Color32 = rgb(0xff3b2b);

/// What a frame of the device shows.
pub struct Tape {
    /// How much of the song has played: the share on the right reel.
    pub progress: f32,
    /// The left and right reels' turn, in radians.
    pub angles: [f32; 2],
    /// A for odd songs in the playlist, B for even ones; on a mixtape,
    /// the side the song is on.
    pub side: char,
    /// The song's artists, as the cassette's label has them.
    pub artist: String,
    /// The song's title as written on a tape (`short_title`).
    pub title: String,
    pub playing: bool,
    pub counter: f32,
    pub disc: cd::Disc,
    pub turntable: vinyl::Turntable,
    /// The playlist on the cassette (MIX), or None for the song (SINGLE).
    pub mixtape: Option<Mixtape>,
    /// The Walkman's switch, from SINGLE (0) to MIX (1) as drawn.
    pub switch: f32,
    /// The playlist on the reels at 3¾ IPS, or None at 7½.
    pub reel_list: Option<String>,
    /// The reel deck's lever, from 3¾ (0) to 7½ (1) as drawn.
    pub lever: f32,
}

/// The playlist playing as a mixtape: its name, the cassette's length,
/// and where the song playing is on it.
pub struct Mixtape {
    pub name: String,
    /// Both sides, in minutes: C46, C60, C90 or C120.
    pub minutes: u32,
    pub side: char,
    /// The share of the tape on the right reel.
    pub share: f32,
}

/// What moves from frame to frame.
#[derive(Default)]
pub struct Motion {
    angles: [f32; 2],
    counter: f32,
    /// The song's progress as drawn, which winds to the real one.
    drawn: Option<f32>,
    winding: bool,
    bars: [f32; BANDS],
    tops: [f32; BANDS],
    holds: [f32; BANDS],
    levels: [f32; 2],
    level_tops: [f32; 2],
    level_holds: [f32; 2],
    disc_angle: f32,
    /// The single on the platter, and the one being changed for it.
    record: Option<vinyl::Record>,
    change: Option<Change>,
    spin: f32,
    platter: f32,
    arm: Option<f32>,
    lift: f32,
    /// The arm is up and on its way to another place on the record.
    cueing: bool,
    switch: f32,
    /// The reel deck's lever, from 7½ (0) to 3¾ (1).
    slow: f32,
}

/// A record being changed, or an LP turned over: how long since it
/// began, the record going, and where the arm was.
struct Change {
    t: f32,
    old: vinyl::Record,
    from: f32,
    flip: bool,
}

impl Change {
    /// When the record is down on the platter, and when the arm is on it.
    fn ends(&self) -> (f32, f32) {
        if self.flip {
            (DROPPED, FLIP_DOWN)
        } else {
            (RECORD_ON, ARM_DOWN)
        }
    }
}

impl Motion {
    /// Moves everything on by `dt` seconds; says whether anything still
    /// moves.
    fn step(&mut self, view: &View, reading: &Reading, dt: f32) -> bool {
        let player = view.player;
        let playing = player.status == Status::Playing;
        let target = match &player.track {
            Some(track) if track.duration_ms > 0 && player.status != Status::Stopped => {
                (player.position_now() as f32 / track.duration_ms as f32).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let media = view.settings.deck_media;
        let now = record(view);
        let same = match (&now, &self.record) {
            (Some(now), Some(on)) => now.same(on),
            (now, on) => now == on,
        };
        if !same
            && media == DeckMedia::Vinyl
            && let (Some(old), Some(new)) = (self.record.take(), &now)
        {
            // The other side of the same LP: it's turned over.
            let flip = old.lp.is_some()
                && new.lp.is_some()
                && (old.title == new.title && old.label == new.label);
            let from = self.arm.unwrap_or(vinyl::REST);
            self.change = Some(Change {
                t: 0.0,
                old,
                from,
                flip,
            });
        }
        self.record = now;
        let mut moving = false;
        match media {
            DeckMedia::Cd | DeckMedia::Vinyl => {
                // Neither winds: the laser or the arm goes to its place.
                self.drawn = Some(target);
                self.winding = false;
                if media == DeckMedia::Cd && playing {
                    self.disc_angle += DISC_SPEED / cd::radius(disc(view).at) * dt;
                }
                if media == DeckMedia::Vinyl {
                    let groove = match self.record.as_ref().and_then(|record| record.lp.as_ref()) {
                        Some(_) => album(view).map(|album| vinyl::arm_on_lp(album.needle())),
                        None => None,
                    };
                    let arm = groove.unwrap_or_else(|| vinyl::arm_for(target));
                    moving = self.play_records(view, arm, playing, dt);
                }
            }
            _ => {
                // Side B of a mixtape plays back the other way; reels at
                // 3¾ run at half the speed.
                let (target, pace) = match (mixtape(view), slow_reel(view)) {
                    (Some(mix), _) => (mix.share, if mix.side == 'B' { -1.0 } else { 1.0 }),
                    (_, Some(album)) => (album.through(), 0.5),
                    _ => (target, 1.0),
                };
                self.wind(view, target, pace, playing, dt);
            }
        }
        let switch = if view.settings.cassette_mix { 1.0 } else { 0.0 };
        self.switch += (switch - self.switch) * (dt * 16.0).min(1.0);
        let slow = if view.settings.reel_slow { 1.0 } else { 0.0 };
        self.slow += (slow - self.slow) * (dt * 14.0).min(1.0);
        moving |= (switch - self.switch).abs() > 0.001 || (slow - self.slow).abs() > 0.001;

        let share = |db: f32, floor: f32, top: f32| ((db - floor) / (top - floor)).clamp(0.0, 1.0);
        for band in 0..BANDS {
            let value = share(reading.bands[band], BAR_FLOOR, BAR_TOP);
            fall(&mut self.bars[band], value, 1.6 * dt);
            hold(
                &mut self.tops[band],
                &mut self.holds[band],
                self.bars[band],
                0.5,
                0.7 * dt,
                dt,
            );
        }
        for side in 0..2 {
            let value = share(reading.peaks[side], LEVEL_FLOOR, 0.0);
            fall(&mut self.levels[side], value, 1.4 * dt);
            let level = self.levels[side];
            hold(
                &mut self.level_tops[side],
                &mut self.level_holds[side],
                level,
                0.8,
                0.5 * dt,
                dt,
            );
        }
        playing
            || moving
            || self.winding
            || self
                .tops
                .iter()
                .chain(&self.level_tops)
                .any(|top| *top > 0.0)
    }

    /// Moves the record change on, the arm to `groove`, its angle over
    /// the song playing, and the record and platter round; says whether
    /// anything moves.
    fn play_records(&mut self, view: &View, groove: f32, playing: bool, dt: f32) -> bool {
        if let Some(change) = &mut self.change {
            change.t += dt;
            if change.t >= change.ends().1 {
                self.change = None;
                self.cueing = false;
            }
        }
        let resting = view.player.status == Status::Stopped || self.record.is_none();
        let target = if resting { vinyl::REST } else { groove };
        let arm = self.arm.get_or_insert(target);
        let before = *arm;
        let rest = vinyl::REST;
        match &self.change {
            Some(c) if c.t < ARM_BACK => *arm = c.from + (rest - c.from) * ease(c.t / ARM_BACK),
            Some(c) if c.t < c.ends().0 => *arm = rest,
            Some(c) => {
                let (on, down) = c.ends();
                *arm = rest + (target - rest) * ease((c.t - on) / (down - on))
            }
            None => {
                // Far off, as after a skip on an LP, the arm is lifted
                // there and lowered; near, it follows the groove.
                let gap = target - *arm;
                if gap.abs() > 0.03 {
                    self.cueing = true;
                }
                if !self.cueing {
                    *arm += gap * (dt * 8.0).min(1.0);
                } else if self.lift > 0.85 {
                    *arm += (1.4 * dt).min(gap.abs()) * gap.signum();
                }
                if (target - *arm).abs() < 0.002 {
                    self.cueing = false;
                }
            }
        }
        let arm_moved = (*arm - before).abs() > 0.0005;
        let dropping = matches!(&self.change, Some(c) if (ARM_BACK..c.ends().0).contains(&c.t));
        if playing {
            let lp = self
                .record
                .as_ref()
                .is_some_and(|record| record.lp.is_some());
            let speed = if lp { LP_SPEED } else { SINGLE_SPEED };
            self.platter += speed * dt;
            if !dropping {
                self.spin += speed * dt;
            }
        }
        let up = if self.cueing || matches!(&self.change, Some(c) if c.t < c.ends().1 - 0.1) {
            1.0
        } else {
            0.0
        };
        self.lift += (up - self.lift) * (dt * 10.0).min(1.0);
        self.change.is_some() || arm_moved || self.lift > 0.01
    }

    /// The turntable as drawn: where the record is in a change, and the arm.
    fn turntable(&self) -> vinyl::Turntable {
        let still = |record: &vinyl::Record| vinyl::Placed {
            record: record.clone(),
            slide: 0.0,
            lifted: 0.0,
            over: 1.0,
        };
        let record = match &self.change {
            Some(c) if c.t < ARM_BACK => Some(still(&c.old)),
            // An LP turned over: up, over, down.
            Some(c) if c.flip && c.t < LIFTED => Some(vinyl::Placed {
                lifted: ease((c.t - ARM_BACK) / (LIFTED - ARM_BACK)),
                ..still(&c.old)
            }),
            Some(c) if c.flip && c.t < TURNED => {
                let t = ease((c.t - LIFTED) / (TURNED - LIFTED));
                let record = if t < 0.5 {
                    Some(&c.old)
                } else {
                    self.record.as_ref()
                };
                record.map(|record| vinyl::Placed {
                    lifted: 1.0,
                    over: (t * std::f32::consts::PI).cos(),
                    ..still(record)
                })
            }
            Some(c) if c.flip => self.record.as_ref().map(|record| vinyl::Placed {
                lifted: 1.0 - ease((c.t - TURNED) / (DROPPED - TURNED)),
                ..still(record)
            }),
            Some(c) if c.t < RECORD_OFF => Some(vinyl::Placed {
                slide: ease((c.t - ARM_BACK) / (RECORD_OFF - ARM_BACK)) * vinyl::SLIDE,
                ..still(&c.old)
            }),
            Some(c) if c.t < RECORD_ON => {
                // Slides on above the platter, then drops onto it.
                let t = (c.t - RECORD_OFF) / (RECORD_ON - RECORD_OFF);
                self.record.as_ref().map(|record| vinyl::Placed {
                    slide: (1.0 - ease(t / 0.75)) * vinyl::SLIDE,
                    lifted: 1.0 - ease((t - 0.75) / 0.25),
                    ..still(record)
                })
            }
            _ => self.record.as_ref().map(still),
        };
        vinyl::Turntable {
            record,
            spin: self.spin,
            platter: self.platter,
            arm: self.arm.unwrap_or(vinyl::REST),
            lift: self.lift,
            lp: false,
        }
    }

    /// Winds the tape on to `target`, the share on the right reel, and
    /// plays it at `pace` (1 left to right, -1 back, 0.5 at half the
    /// speed).
    fn wind(&mut self, view: &View, target: f32, pace: f32, playing: bool, dt: f32) {
        let drawn = self.drawn.get_or_insert(target);
        let gap = target - *drawn;
        let pack = |share: f32| match view.settings.deck_media {
            DeckMedia::Reel => reel::pack(share) / 1.6,
            _ => cassette::pack(share),
        };
        let mut turn = |speed: f32, share: f32| {
            self.angles[0] += speed / pack(1.0 - share) * dt;
            self.angles[1] += speed / pack(share) * dt;
        };
        if gap.abs() > 0.02 || (self.winding && gap.abs() > 0.001) {
            self.winding = true;
            let step = (WIND_SPEED * dt).min(gap.abs()) * gap.signum();
            *drawn += step;
            turn(900.0 * gap.signum(), *drawn);
            self.counter += 40.0 * dt * gap.signum();
        } else {
            self.winding = false;
            *drawn = target;
            if playing {
                turn(TAPE_SPEED * pace, target);
                self.counter += 0.9 * dt * pace.abs();
            }
        }
        self.counter = self.counter.max(0.0);
    }

    fn tape(&self, view: &View) -> Tape {
        let (side, artist, title) = match player_strip::playing_entry(view)
            .and_then(|(list, entry)| Some((view.list(list)?, entry)))
            .and_then(|(list, entry)| Some((list, list.index_of(entry)?, list.get(entry)?)))
        {
            Some((_, index, entry)) => (
                if index % 2 == 0 { 'A' } else { 'B' },
                artists(&entry.track),
                short_title(&entry.track.title).to_string(),
            ),
            None => ('A', String::new(), String::new()),
        };
        let mixtape = mixtape(view);
        Tape {
            progress: self.drawn.unwrap_or(0.0),
            angles: self.angles,
            side: mixtape.as_ref().map_or(side, |mix| mix.side),
            artist,
            title,
            playing: view.player.status == Status::Playing,
            counter: self.counter,
            disc: cd::Disc {
                angle: self.disc_angle,
                ..disc(view)
            },
            turntable: vinyl::Turntable {
                lp: view.settings.vinyl_lp,
                ..self.turntable()
            },
            mixtape,
            switch: self.switch,
            reel_list: slow_reel(view).map(|album| album.name),
            lever: 1.0 - self.slow,
        }
    }
}

/// Eases 0–1 in and out; clamped either side.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The single of the song playing: its artists, its title and its place
/// in the playlist, which picks its label.
fn record(view: &View) -> Option<vinyl::Record> {
    if view.settings.vinyl_lp
        && let Some(album) = album(view)
    {
        return Some(album.lp());
    }
    let (list, entry) = player_strip::playing_entry(view)?;
    let list = view.list(list)?;
    let track = &list.get(entry)?.track;
    Some(vinyl::Record {
        artist: artists(track),
        title: short_title(&track.title).to_string(),
        label: list.index_of(entry)?,
        lp: None,
    })
}

/// A song's artists as a tape or a record has them.
fn artists(track: &TrackRef) -> String {
    track
        .artists
        .iter()
        .map(|artist| artist.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A song's title as written on a tape: without the version Spotify adds
/// after " - " ("Insight - 2019 Digital Master"), nor anything in
/// brackets before or after it ("(Don't Fear) The Reaper" is "The
/// Reaper"). A title all in brackets stays as it is.
fn short_title(title: &str) -> &str {
    let mut title = title.split(" - ").next().unwrap_or(title).trim();
    loop {
        let before = title;
        for (open, close) in [('(', ')'), ('[', ']')] {
            if let Some(rest) = title.strip_prefix(open)
                && let Some(at) = rest.find(close)
                && !rest[at + 1..].trim().is_empty()
            {
                title = rest[at + 1..].trim_start();
            }
            if let Some(rest) = title.strip_suffix(close)
                && let Some(at) = rest.rfind(open)
                && !rest[..at].trim().is_empty()
            {
                title = rest[..at].trim_end();
            }
        }
        if title == before {
            return title;
        }
    }
}

/// A playlist on a cassette: side A takes songs while they fit in half
/// the playlist, so no song is cut in two, and side B the rest.
#[derive(Debug, PartialEq)]
struct Sides {
    /// The first song on side B.
    split: usize,
    /// A side's length on the cassette, in ms.
    side: u64,
    /// Both sides, in minutes.
    minutes: u32,
    /// The longer side's songs, in ms: as far as an LP's grooves reach.
    longer: u64,
}

/// The sides of a playlist of songs `lengths` ms long, on the shortest
/// standard cassette that holds the longer one; past a C120, one just long
/// enough.
fn sides(lengths: &[u64]) -> Sides {
    let total: u64 = lengths.iter().sum();
    let (mut split, mut a) = (0, 0);
    while split + 1 < lengths.len() && a + lengths[split] <= total / 2 {
        a += lengths[split];
        split += 1;
    }
    if split == 0 && !lengths.is_empty() {
        (split, a) = (1, lengths[0]);
    }
    let longer = a.max(total - a);
    let minutes = CASSETTES
        .into_iter()
        .find(|minutes| u64::from(*minutes) * 30_000 >= longer)
        .unwrap_or(longer.div_ceil(60_000) as u32 * 2);
    Sides {
        split,
        side: u64::from(minutes) * 30_000,
        minutes,
        longer,
    }
}

/// Where song `index` is `position` ms in: its side, and the share of the
/// tape on the right reel. Side A runs left to right, side B back.
fn place(lengths: &[u64], sides: &Sides, index: usize, position: u64) -> (char, f32) {
    let first = if index < sides.split { 0 } else { sides.split };
    let into = lengths[first..index].iter().sum::<u64>() + position.min(lengths[index]);
    let share = (into as f32 / sides.side.max(1) as f32).clamp(0.0, 1.0);
    if index < sides.split {
        ('A', share)
    } else {
        ('B', 1.0 - share)
    }
}

/// The playlist playing laid out on two sides, as on a mixtape or an LP,
/// and where the song playing is.
struct Album {
    list: PlaylistId,
    name: String,
    lengths: Vec<u64>,
    sides: Sides,
    index: usize,
    position: u64,
}

impl Album {
    /// The songs on the song playing's side.
    fn side(&self) -> std::ops::Range<usize> {
        if self.index < self.sides.split {
            0..self.sides.split
        } else {
            self.sides.split..self.lengths.len()
        }
    }

    /// How far into its side the song playing is, as a share of the longer
    /// side: how far in on an LP the needle is.
    fn needle(&self) -> f32 {
        let before: u64 = self.lengths[self.side().start..self.index].iter().sum();
        let into = before + self.position.min(self.lengths[self.index]);
        into as f32 / self.sides.longer.max(1) as f32
    }

    /// How far through the whole playlist the song playing is.
    fn through(&self) -> f32 {
        let before: u64 = self.lengths[..self.index].iter().sum();
        let total: u64 = self.lengths.iter().sum();
        (before + self.position.min(self.lengths[self.index])) as f32 / total.max(1) as f32
    }

    /// The side of the LP the song playing is on.
    fn lp(&self) -> vinyl::Record {
        let side = self.side();
        let longer = self.sides.longer.max(1) as f32;
        let mut marks = vec![0.0];
        let mut sum = 0;
        for length in &self.lengths[side.clone()] {
            sum += length;
            marks.push(sum as f32 / longer);
        }
        vinyl::Record {
            artist: String::new(),
            title: self.name.clone(),
            label: self.list.0 as usize,
            lp: Some(vinyl::Lp {
                side: if side.start == 0 { 'A' } else { 'B' },
                songs: side.len(),
                seconds: sum / 1000,
                marks,
            }),
        }
    }
}

/// The playlist playing, on two sides. The Spotify tab's songs aren't a
/// playlist: they play a song a tape or a record.
fn album(view: &View) -> Option<Album> {
    let (list, entry) = player_strip::playing_entry(view)?;
    if list == PlaylistId::SPOTIFY {
        return None;
    }
    let list = view.list(list)?;
    let index = list.index_of(entry)?;
    let lengths: Vec<u64> = list
        .entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .collect();
    let position = match view.player.status {
        Status::Stopped => 0,
        _ => u64::from(view.player.position_now()),
    };
    Some(Album {
        list: list.id,
        name: list.name.clone(),
        sides: sides(&lengths),
        lengths,
        index,
        position,
    })
}

/// The playlist playing on the reels, when the reel deck plays at 3¾.
fn slow_reel(view: &View) -> Option<Album> {
    if !view.settings.reel_slow || view.settings.deck_media != DeckMedia::Reel {
        return None;
    }
    album(view)
}

/// The playlist playing as a mixtape, when the Walkman plays and its switch
/// is at MIX.
fn mixtape(view: &View) -> Option<Mixtape> {
    if !view.settings.cassette_mix || view.settings.deck_media != DeckMedia::Cassette {
        return None;
    }
    let album = album(view)?;
    let (side, share) = place(&album.lengths, &album.sides, album.index, album.position);
    Some(Mixtape {
        name: album.name,
        minutes: album.sides.minutes,
        side,
        share,
    })
}

/// The disc of the playlist playing, and where in it the song plays.
fn disc(view: &View) -> cd::Disc {
    let player = view.player;
    let Some((list, index)) = player_strip::playing_entry(view)
        .and_then(|(list, entry)| Some((view.list(list)?, entry)))
        .and_then(|(list, entry)| Some((list, list.index_of(entry)?)))
    else {
        return cd::Disc::default();
    };
    let lengths: Vec<u64> = list
        .entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .collect();
    let total = lengths.iter().sum::<u64>().max(1) as f32;
    let mut ends = Vec::with_capacity(lengths.len());
    let mut sum = 0;
    for length in &lengths {
        sum += length;
        ends.push(sum as f32 / total);
    }
    let position = match player.status {
        Status::Stopped => 0,
        _ => player.position_now(),
    };
    let before: u64 = lengths[..index].iter().sum();
    let length = lengths[index].max(1);
    cd::Disc {
        ends,
        at: (before + u64::from(position).min(length)) as f32 / total,
        number: index + 1,
        seconds: position / 1000,
        status: player.status,
        angle: 0.0,
    }
}

/// Rises at once to `value`, falls by at most `by`.
fn fall(level: &mut f32, value: f32, by: f32) {
    *level = if value > *level {
        value
    } else {
        (*level - by).max(value)
    };
}

/// A peak mark: up with `level`, held `seconds`, then falling by `by`.
fn hold(top: &mut f32, held: &mut f32, level: f32, seconds: f32, by: f32, dt: f32) {
    if level >= *top {
        *top = level;
        *held = seconds;
    } else if *held > 0.0 {
        *held -= dt;
    } else {
        *top = (*top - by).max(0.0);
    }
}

/// A bar at `angle` from `r0` to `r1` round `x, y`, `width` across: a
/// hub's tooth or key.
#[allow(clippy::too_many_arguments)]
fn spoke(c: &Canvas, x: f32, y: f32, angle: f32, r0: f32, r1: f32, width: f32, colour: Color32) {
    let (ax, ay) = (angle.cos(), angle.sin());
    let (nx, ny) = (-ay * width / 2.0, ax * width / 2.0);
    c.polygon(
        &[
            (x + ax * r0 + nx, y + ay * r0 + ny),
            (x + ax * r1 + nx, y + ay * r1 + ny),
            (x + ax * r1 - nx, y + ay * r1 - ny),
            (x + ax * r0 - nx, y + ay * r0 - ny),
        ],
        colour,
    );
}

/// Draws the deck from the strip's buttons down, the buttons among it;
/// returns its bottom.
pub fn draw(
    ui: &mut Ui,
    strip: Rect,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) -> f32 {
    let keys = player_strip::keys(ui, view, strip);
    let width = keys.width + 2.0 * SIDE;
    let k = width / walkman::WIDTH;
    let band = keys.height + 2.0 * MARGIN;
    let top = keys.row.center().y - band / 2.0;
    let left = strip.left();

    let reading = analyser::read();
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let moving = inputs.deck.step(view, &reading, dt);
    let tape = inputs.deck.tape(view);
    let painter = ui.painter().clone();
    let media = view.settings.deck_media;
    let mut switch = None;
    let mut speeds = None;
    let mut lever = None;
    let (art, keys_top, bottom, spectrum_top) = match media {
        DeckMedia::Reel => {
            let plate = reel::PLATE * k;
            let canvas = Canvas {
                painter: painter.clone(),
                origin: pos2(left, top),
                k,
            };
            lever = Some(reel::draw(&canvas, band / k, &tape));
            let art = Rect::from_min_size(pos2(left, top), vec2(width, plate));
            (art, top + plate + MARGIN, top + plate + band, top)
        }
        DeckMedia::Cassette | DeckMedia::Cd | DeckMedia::Vinyl => {
            let body_top = keys.row.center().y;
            let bottom = top + band + WALKMAN_BODY * k;
            let canvas = Canvas {
                painter: painter.clone(),
                origin: pos2(left, body_top),
                k,
            };
            let height = (bottom - body_top) / k;
            match media {
                DeckMedia::Cd => cd::draw(&canvas, height, &tape),
                DeckMedia::Vinyl => speeds = Some(vinyl::draw(&canvas, height, &tape.turntable)),
                _ => switch = Some(walkman::draw(&canvas, height, &tape)),
            }
            let art = Rect::from_min_max(pos2(left, top + band), pos2(left + width, bottom));
            (art, top + MARGIN, bottom, top + band)
        }
    };
    let other = match media {
        DeckMedia::Cassette => "Play it on reels",
        DeckMedia::Reel => "Play it on a CD",
        DeckMedia::Cd => "Play it on a record",
        DeckMedia::Vinyl => "Play it on a cassette",
    };
    if ui
        .interact(art, ui.id().with("deck media"), Sense::click())
        .on_hover_text(other)
        .clicked()
    {
        actions.push(Action::SetDeckMedia(media.next()));
    }
    if let Some(rect) = switch {
        let mix = view.settings.cassette_mix;
        let tip = if mix {
            "Put only the song playing on the tape (SINGLE)"
        } else {
            "Put the whole playlist on the tape (MIX)"
        };
        if ui
            .interact(rect, ui.id().with("cassette mix"), Sense::click())
            .on_hover_text(tip)
            .clicked()
        {
            actions.push(Action::SetCassetteMix(!mix));
        }
    }
    if let Some(rect) = lever {
        let slow = view.settings.reel_slow;
        let tip = if slow {
            "Play a song a reel (7½ IPS)"
        } else {
            "Play the whole playlist on the reels (3¾ IPS)"
        };
        if ui
            .interact(rect, ui.id().with("reel speed"), Sense::click())
            .on_hover_text(tip)
            .clicked()
        {
            actions.push(Action::SetReelSlow(!slow));
        }
    }
    if let Some([lp, single]) = speeds {
        for (rect, on, tip, id) in [
            (lp, true, "Play the whole playlist on an LP (33)", "33"),
            (single, false, "Play a single for each song (45)", "45"),
        ] {
            if ui
                .interact(rect, ui.id().with(id), Sense::click())
                .on_hover_text(tip)
                .clicked()
                && view.settings.vinyl_lp != on
            {
                actions.push(Action::SetVinylLp(on));
            }
        }
    }
    let row = Rect::from_min_size(pos2(left + SIDE, keys_top), vec2(keys.width, keys.height));
    player_strip::draw_keys(ui, row, &keys, view, inputs, actions);

    let zone = Rect::from_min_max(
        pos2(left + width + GAP, spectrum_top),
        pos2(strip.right(), bottom),
    );
    if zone.width() > 60.0 {
        let spectrum = Rect::from_min_max(
            zone.min,
            pos2(zone.right(), zone.bottom() - LEVELS_HEIGHT - 6.0),
        );
        let levels = Rect::from_min_max(pos2(zone.left(), zone.bottom() - LEVELS_HEIGHT), zone.max);
        draw_spectrum(&painter, spectrum, view, &inputs.deck);
        draw_levels(&painter, levels, view, &inputs.deck);
    }
    if moving {
        ui.ctx().request_repaint();
    }
    bottom
}

/// An LED's colour, lit or not, `place` of the way up its column (0–1).
fn led(place: f32, lit: bool) -> Color32 {
    let colour = if place >= 0.87 {
        RED
    } else if place >= 0.62 {
        YELLOW
    } else {
        GREEN
    };
    if lit {
        colour
    } else {
        colour.gamma_multiply(0.14)
    }
}

fn draw_spectrum(painter: &Painter, rect: Rect, view: &View, motion: &Motion) {
    let palette = view.palette;
    theme::inset(painter, rect, Color32::BLACK, &palette);
    let labels = 16.0;
    let inner = Rect::from_min_max(rect.min + vec2(10.0, 10.0), rect.max - vec2(10.0, labels));
    let column = inner.width() / BANDS as f32;
    let segment = inner.height() / SEGMENTS as f32;
    for band in 0..BANDS {
        let x = inner.left() + band as f32 * column;
        let lit = (motion.bars[band] * SEGMENTS as f32).round() as usize;
        let top = ((motion.tops[band] * SEGMENTS as f32).round() as usize).min(SEGMENTS);
        for s in 0..SEGMENTS {
            let y = inner.bottom() - (s + 1) as f32 * segment;
            let on = s < lit || (top > 0 && s + 1 == top && top > lit);
            let cell = Rect::from_min_size(
                pos2(x + column * 0.12, y + segment * 0.12),
                vec2(column * 0.76, segment * 0.76),
            );
            painter.rect_filled(cell, 0.0, led(s as f32 / SEGMENTS as f32, on));
        }
    }
    let font = FontId::monospace(8.0);
    for (hz, label) in [
        (60.0, "60"),
        (150.0, "150"),
        (400.0, "400"),
        (1000.0, "1K"),
        (2500.0, "2.5K"),
        (6000.0, "6K"),
        (15000.0, "15K"),
    ] {
        let x = inner.left() + analyser::place(hz) * column;
        painter.text(
            pos2(x, rect.bottom() - labels / 2.0 - 2.0),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            palette.chrome,
        );
    }
}

fn draw_levels(painter: &Painter, rect: Rect, view: &View, motion: &Motion) {
    let palette = view.palette;
    theme::inset(painter, rect, Color32::BLACK, &palette);
    let font = FontId::monospace(8.0);
    let left = rect.left() + 20.0;
    let width = rect.right() - 10.0 - left;
    let cell = width / LEVEL_LEDS as f32;
    for (side, name) in ["L", "R"].into_iter().enumerate() {
        let y = rect.top() + 6.0 + side as f32 * 11.0;
        painter.text(
            pos2(rect.left() + 10.0, y + 4.0),
            Align2::CENTER_CENTER,
            name,
            font.clone(),
            palette.chrome,
        );
        let lit = (motion.levels[side] * LEVEL_LEDS as f32).round() as usize;
        let top = (motion.level_tops[side] * LEVEL_LEDS as f32).round() as usize;
        for i in 0..LEVEL_LEDS {
            let db = LEVEL_FLOOR + (i + 1) as f32;
            let colour = if db > -3.0 {
                RED
            } else if db > -10.0 {
                YELLOW
            } else {
                GREEN
            };
            let on = i < lit || (top > 0 && i + 1 == top && top > lit);
            let led =
                Rect::from_min_size(pos2(left + i as f32 * cell + 1.0, y), vec2(cell - 2.5, 8.0));
            painter.rect_filled(
                led,
                0.0,
                if on {
                    colour
                } else {
                    colour.gamma_multiply(0.14)
                },
            );
        }
    }
    for db in [-40, -30, -20, -10, -6, -3, 0] {
        let x = left + (db as f32 - LEVEL_FLOOR) / -LEVEL_FLOOR * width;
        painter.text(
            pos2(x, rect.bottom() - 7.0),
            Align2::CENTER_CENTER,
            db.to_string(),
            font.clone(),
            palette.chrome,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Album, PlaylistId, Sides, place, short_title, sides};

    const MINUTE: u64 = 60_000;

    #[test]
    fn a_mixtape_splits_between_songs() {
        // 39 minutes: side A takes the first 19, the third song doesn't fit.
        let lengths = [9 * MINUTE, 10 * MINUTE, 8 * MINUTE, 12 * MINUTE];
        let mix = sides(&lengths);
        assert_eq!(
            mix,
            Sides {
                split: 2,
                side: 23 * MINUTE,
                minutes: 46,
                longer: 20 * MINUTE,
            }
        );
        assert_eq!(place(&lengths, &mix, 0, 0), ('A', 0.0));
        let (side, share) = place(&lengths, &mix, 1, 0);
        assert_eq!(side, 'A');
        assert!((share - 9.0 / 23.0).abs() < 1e-6);
        // Side B starts with all the tape on the right reel.
        assert_eq!(place(&lengths, &mix, 2, 0), ('B', 1.0));
        let (side, share) = place(&lengths, &mix, 3, MINUTE);
        assert_eq!(side, 'B');
        assert!((share - (1.0 - 9.0 / 23.0)).abs() < 1e-6);
    }

    #[test]
    fn an_lp_has_a_band_a_song_on_its_side() {
        let lengths = vec![9 * MINUTE, 10 * MINUTE, 8 * MINUTE, 12 * MINUTE];
        let album = |index, position| Album {
            list: PlaylistId(3),
            name: "Night Drive".into(),
            sides: sides(&lengths),
            lengths: lengths.clone(),
            index,
            position,
        };
        // The longer side, B, reaches the run-out: side A stops short.
        let a = album(1, MINUTE);
        assert!((a.needle() - 10.0 / 20.0).abs() < 1e-6);
        let record = a.lp();
        let lp = record.lp.as_ref().unwrap();
        assert_eq!((lp.side, lp.songs, lp.seconds), ('A', 2, 19 * 60));
        assert_eq!(lp.marks, vec![0.0, 0.45, 0.95]);
        assert_eq!((record.title.as_str(), record.label), ("Night Drive", 3));
        let b = album(3, 0);
        assert_eq!(b.lp().lp.unwrap().side, 'B');
        assert!((b.needle() - 8.0 / 20.0).abs() < 1e-6);
        // On the reels, one pass through all of it.
        assert!((b.through() - 27.0 / 39.0).abs() < 1e-6);
        // The same side when a song is added: no change of record.
        assert!(a.lp().same(&album(0, 0).lp()));
        assert!(!a.lp().same(&b.lp()));
    }

    #[test]
    fn a_mixtape_takes_the_cassette_it_fits() {
        assert_eq!(sides(&[20 * MINUTE, 20 * MINUTE]).minutes, 46);
        assert_eq!(sides(&[25 * MINUTE, 25 * MINUTE]).minutes, 60);
        assert_eq!(sides(&[44 * MINUTE, 45 * MINUTE]).minutes, 90);
        assert_eq!(sides(&[50 * MINUTE, 55 * MINUTE]).minutes, 120);
        // Longer than a C120: just long enough.
        assert_eq!(sides(&[70 * MINUTE, 73 * MINUTE + 1]).minutes, 148);
        // A song longer than half still goes on side A, alone.
        let mix = sides(&[30 * MINUTE, 5 * MINUTE, 5 * MINUTE]);
        assert_eq!((mix.split, mix.minutes), (1, 60));
        assert_eq!(sides(&[3 * MINUTE]).split, 1);
    }

    #[test]
    fn a_tape_leaves_out_the_version() {
        assert_eq!(short_title("Insight - 2019 Digital Master"), "Insight");
        assert_eq!(short_title("Heroes - 2017 Remaster"), "Heroes");
        assert_eq!(short_title("Starman (Remastered 2012)"), "Starman");
        assert_eq!(short_title("Song [2009 Remaster]"), "Song");
        assert_eq!(
            short_title("(Don't Fear) The Reaper - Digital Remaster"),
            "The Reaper"
        );
        assert_eq!(short_title("Heroes (Single Version)"), "Heroes");
        assert_eq!(
            short_title("(Sittin' On) The Dock (Of the Bay)"),
            "The Dock"
        );
        assert_eq!(short_title("(Untitled)"), "(Untitled)");
        assert_eq!(short_title("Insight"), "Insight");
    }
}
