//! What to download of a file, and when: plain rules over what's downloaded
//! and asked for, carried out by the download (`super`).
//!
//! A file downloads in parts, one at a time. While a song plays, parts are
//! asked for ahead of where it's read: enough to cover the time a server
//! takes to answer, several times over, so the audio never waits. A seek to
//! a part not downloaded yet stops that while the decoder looks for its
//! place (each read then fetches only a small part), and drops the parts
//! asked for before it that haven't started, unless they hold the place
//! read: many seeks would otherwise queue parts left behind ahead of the
//! one playing.

use std::ops::Range;
use std::time::Duration;

use super::ranges::Ranges;

/// The smallest part asked for. The decoder reads in blocks of this size,
/// which must be a power of two over 32 KiB.
pub const MINIMUM_PART: usize = 64 * 1024;
/// The slowest download assumed.
const MINIMUM_THROUGHPUT: usize = 8 * 1024;
/// The time a server takes to answer, until one has been measured.
const INITIAL_PING: Duration = Duration::from_millis(500);
/// A server taking longer to answer is taken to take this long, so the
/// parts asked ahead don't run away.
const MAXIMUM_PING: Duration = Duration::from_millis(1500);
/// How far ahead of the place read a read asks for, while a song plays.
pub const READ_AHEAD: Duration = Duration::from_secs(5);
/// How many times the time a server takes to answer is kept asked for
/// ahead.
const AHEAD_FACTOR: f32 = 4.0;
/// How long a read waits for its part before it gives up: the network is
/// gone. The smallest part, at the slowest download.
pub const READ_TIMEOUT: Duration =
    Duration::from_secs((MINIMUM_PART / MINIMUM_THROUGHPUT) as u64);
/// How long a server may send nothing before what's left of a part is
/// asked of the next. All of them get their turn well before a read gives
/// up, so a stuck server isn't taken for a lost connection. It is also the
/// slack given to a server beyond the time the part takes to play: one
/// sending too slowly to keep up is no better.
pub const SERVER_STALL_LIMIT: Duration = Duration::from_secs(3);

/// A part asked for, with the seeks there had been then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub range: Range<usize>,
    pub seeks: usize,
}

#[derive(Debug)]
pub struct Plan {
    pub size: usize,
    /// The file's nominal rate, which the parts are sized by.
    pub bytes_per_second: usize,
    pub downloaded: Ranges,
    pub requested: Ranges,
    pub read_position: usize,
    /// Reads go on from one to the next: parts are asked ahead. Off while
    /// the decoder looks for its place after a seek.
    pub streaming: bool,
    /// Seeks to a part not downloaded yet, counted.
    pub seeks: usize,
    /// The last few times a server took to answer.
    response_times: Vec<Duration>,
    /// Bytes a second, as measured.
    throughput: usize,
}

impl Plan {
    pub fn new(size: usize, bytes_per_second: usize) -> Self {
        Self {
            size,
            bytes_per_second,
            downloaded: Ranges::new(),
            requested: Ranges::new(),
            read_position: 0,
            streaming: false,
            seeks: 0,
            response_times: Vec::new(),
            throughput: 0,
        }
    }

    pub fn whole(&self) -> bool {
        self.downloaded.run_from(0) >= self.size
    }

    /// Whether the rest of the file, from where it's read, is downloaded.
    pub fn to_end(&self) -> bool {
        self.read_position >= self.size
            || self.downloaded.run_from(self.read_position) >= self.size - self.read_position
    }

    /// The parts to ask for to get `range`: at least the smallest part,
    /// larger while streaming (about a second's worth at the speed
    /// measured), within the file, and only what's neither downloaded nor
    /// asked for already. They count as asked for from now on.
    pub fn ask(&mut self, range: Range<usize>) -> Vec<Part> {
        let mut length = range.len().max(MINIMUM_PART);
        if self.streaming {
            length = length.max(self.throughput);
        }
        let end = (range.start + length).min(self.size);
        let wanted = self
            .downloaded
            .union(&self.requested)
            .missing_in(range.start..end);
        wanted
            .iter()
            .map(|r| {
                self.requested.add(r.clone());
                Part {
                    range: r.clone(),
                    seeks: self.seeks,
                }
            })
            .collect()
    }

    /// The parts a read of `length` bytes at `offset` needs: with the next
    /// few seconds, while streaming.
    pub fn for_read(&mut self, offset: usize, length: usize) -> Vec<Part> {
        let mut length = length;
        if self.streaming {
            length += (READ_AHEAD.as_secs_f32() * self.bytes_per_second as f32) as usize;
        }
        let end = (offset + length).min(self.size);
        let wanted = self
            .downloaded
            .union(&self.requested)
            .missing_in(offset..end);
        wanted
            .iter()
            .flat_map(|r| self.ask(r.clone()))
            .collect()
    }

    /// What to ask for ahead, now that no part is downloading: while
    /// streaming, enough that what's asked for and not yet in covers the
    /// time a server takes to answer, several times over. After the place
    /// read first, then from the start.
    pub fn ahead(&mut self) -> Vec<Part> {
        if !self.streaming {
            return Vec::new();
        }
        let pending = {
            let mut pending = self.requested.clone();
            pending.remove_all(&self.downloaded);
            pending.bytes()
        };
        let ping = self.ping().as_secs_f32();
        let wanted = ((AHEAD_FACTOR * ping * self.bytes_per_second as f32) as usize)
            .max((ping * self.throughput as f32) as usize);
        if pending >= wanted {
            return Vec::new();
        }
        let missing = self
            .downloaded
            .union(&self.requested)
            .missing_in(0..self.size);
        let tail = {
            let mut tail = missing.clone();
            tail.remove(0..self.read_position.min(self.size));
            tail
        };
        let Some(next) = tail.first().or_else(|| missing.first()) else {
            return Vec::new();
        };
        let length = next.len().min(wanted - pending);
        self.ask(next.start..next.start + length)
    }

    /// Whether a part asked for before a seek, and not started, is left
    /// behind: it doesn't hold the place read.
    pub fn stale(&self, part: &Part) -> bool {
        part.seeks != self.seeks && !part.range.contains(&self.read_position)
    }

    /// A part is in.
    pub fn received(&mut self, range: Range<usize>) {
        self.downloaded.add(range);
    }

    /// A part won't come, or isn't wanted: no longer asked for, so a read
    /// that needs it asks again.
    pub fn dropped(&mut self, range: Range<usize>) {
        self.requested.remove(range);
    }

    /// A seek to `position`. Returns whether it stopped streaming, as it's
    /// to a part not downloaded yet: the decoder's reads then only look for
    /// its place, until one follows on from the last.
    pub fn seek(&mut self, position: usize) -> bool {
        self.read_position = position;
        if self.downloaded.contains(position) {
            return false;
        }
        self.seeks += 1;
        std::mem::replace(&mut self.streaming, false)
    }

    /// The time a server takes to answer: the median of the last three.
    pub fn ping(&self) -> Duration {
        let mut times = self.response_times.clone();
        times.sort_unstable();
        match times.len() {
            0 => INITIAL_PING,
            2 => (times[0] + times[1]) / 2,
            n => times[n / 2],
        }
    }

    pub fn measured_ping(&mut self, time: Duration) {
        if self.response_times.len() >= 3 {
            self.response_times.remove(0);
        }
        self.response_times.push(time.min(MAXIMUM_PING));
    }

    /// A part of `bytes` took `time`: the speed is averaged with what it was.
    pub fn measured_throughput(&mut self, bytes: usize, time: Duration) {
        let millis = time.as_millis();
        if bytes == 0 || millis == 0 {
            return;
        }
        let measured = ((bytes as u128 * 1000 / millis) as usize).max(MINIMUM_THROUGHPUT);
        self.throughput = if self.throughput > 0 {
            (self.throughput + measured) / 2
        } else {
            measured
        };
    }
}

/// How long a server may send nothing before the next is asked: the last
/// server left has no limit, and is waited for until the read gives up.
pub fn server_limit(servers_left: usize) -> Option<Duration> {
    (servers_left > 0).then_some(SERVER_STALL_LIMIT)
}

/// How long a server has to send the rest of a part: the time it takes to
/// play, and the slack.
pub fn part_time(remaining: usize, bytes_per_second: usize, limit: Duration) -> Duration {
    limit + Duration::from_secs_f64(remaining as f64 / bytes_per_second.max(1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: usize = 40 * 1024;

    fn plan(size: usize) -> Plan {
        Plan::new(size, RATE)
    }

    #[test]
    fn a_part_is_at_least_the_smallest_and_never_past_the_end() {
        let mut plan = plan(100_000);
        assert_eq!(
            plan.ask(0..10),
            [Part {
                range: 0..MINIMUM_PART,
                seeks: 0
            }]
        );
        let tail = plan.ask(90_000..90_010);
        assert_eq!(tail[0].range, 90_000..100_000);
    }

    #[test]
    fn what_is_asked_for_already_is_not_asked_again() {
        let mut plan = plan(1_000_000);
        plan.ask(0..MINIMUM_PART);
        plan.received(0..1000);
        let parts = plan.ask(0..2 * MINIMUM_PART);
        assert_eq!(parts[0].range, MINIMUM_PART..2 * MINIMUM_PART);
    }

    #[test]
    fn streaming_reads_ask_a_few_seconds_ahead() {
        let mut plan = plan(10_000_000);
        plan.streaming = true;
        let parts = plan.for_read(0, 4096);
        let asked: usize = parts.iter().map(|p| p.range.len()).sum();
        assert!(asked >= 5 * RATE, "{asked}");
        // Not streaming: just the read's block.
        let mut plan = super::Plan::new(10_000_000, RATE);
        let parts = plan.for_read(0, 4096);
        assert_eq!(parts[0].range, 0..MINIMUM_PART);
    }

    #[test]
    fn ahead_covers_several_answers_worth_after_the_place_read() {
        let mut plan = plan(10_000_000);
        plan.streaming = true;
        plan.read_position = 1_000_000;
        let parts = plan.ahead();
        // 4 × 0.5 s at 40 KiB/s.
        assert_eq!(parts[0].range.start, 1_000_000);
        assert_eq!(parts[0].range.len(), 80 * 1024);
        // With that asked for, nothing more until some comes in.
        assert!(plan.ahead().is_empty());
    }

    #[test]
    fn ahead_goes_back_to_the_start_once_the_rest_is_in() {
        let mut plan = plan(1_000_000);
        plan.streaming = true;
        plan.read_position = 500_000;
        plan.received(500_000..1_000_000);
        let parts = plan.ahead();
        assert_eq!(parts[0].range.start, 0);
    }

    #[test]
    fn nothing_is_asked_ahead_while_looking_for_a_place() {
        let mut plan = plan(10_000_000);
        plan.streaming = true;
        assert!(plan.seek(5_000_000));
        assert!(!plan.streaming);
        assert!(plan.ahead().is_empty());
    }

    #[test]
    fn a_part_asked_before_a_seek_is_left_behind_unless_it_holds_the_place() {
        let mut plan = plan(10_000_000);
        plan.streaming = true;
        let before = plan.ask(0..MINIMUM_PART).remove(0);
        assert!(!plan.stale(&before));
        plan.seek(5_000_000);
        assert!(plan.stale(&before));
        plan.seek(1000);
        assert!(!plan.stale(&before));
        // A seek to what's downloaded leaves the parts asked for be.
        let mut plan = super::Plan::new(10_000_000, RATE);
        plan.received(0..100_000);
        let part = plan.ask(200_000..200_001).remove(0);
        assert!(!plan.seek(50_000));
        assert!(!plan.stale(&part));
    }

    #[test]
    fn the_rest_is_in_once_downloaded_from_the_place_read() {
        let mut plan = plan(1000);
        plan.read_position = 400;
        plan.received(300..1000);
        assert!(plan.to_end() && !plan.whole());
        plan.received(0..300);
        assert!(plan.whole());
    }

    #[test]
    fn the_answer_time_is_a_median_and_capped() {
        let mut plan = plan(1000);
        assert_eq!(plan.ping(), INITIAL_PING);
        for ms in [100, 5000, 200, 300] {
            plan.measured_ping(Duration::from_millis(ms));
        }
        // The last three: 1500 (capped), 200, 300.
        assert_eq!(plan.ping(), Duration::from_millis(300));
    }

    #[test]
    fn a_server_is_given_the_part_s_playing_time_and_the_slack() {
        assert_eq!(server_limit(0), None);
        assert_eq!(server_limit(2), Some(SERVER_STALL_LIMIT));
        let time = part_time(RATE * 2, RATE, SERVER_STALL_LIMIT);
        assert_eq!(time, Duration::from_secs(5));
    }
}
