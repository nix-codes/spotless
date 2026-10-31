//! When the worker connects to Spotify: at the start, on the saved
//! credential, and again whenever the session drops, waiting longer after
//! each try that can't reach Spotify.

use std::time::{Duration, Instant};

/// The waits between connections that can't reach Spotify, doubling.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MOST: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(super) struct Retry {
    /// Connecting on the saved credential at start-up, until Spotify answers.
    starting: bool,
    /// After a failed connection: no new one before then.
    at: Option<Instant>,
    /// The wait after the next failed connection.
    wait: Duration,
}

impl Retry {
    pub(super) fn new() -> Self {
        Self {
            starting: false,
            at: None,
            wait: RETRY_FIRST,
        }
    }

    /// Spotless starts: it connects on the saved credential.
    pub(super) fn start(&mut self) {
        self.starting = true;
    }

    pub(super) fn starting(&self) -> bool {
        self.starting
    }

    /// The start's connection is over: it was made, or a sign-in takes
    /// its place.
    pub(super) fn started(&mut self) {
        self.starting = false;
    }

    /// Whether to connect now: at the start, or with the session
    /// `dropped`, once the wait after the last failure is over.
    pub(super) fn due(&self, dropped: bool, now: Instant) -> bool {
        (dropped || self.starting) && self.at.is_none_or(|at| now >= at)
    }

    /// A try couldn't reach Spotify: the next waits the returned time,
    /// and the one after it twice as long, up to two minutes.
    pub(super) fn failed(&mut self, now: Instant) -> Duration {
        let wait = self.wait;
        self.at = Some(now + wait);
        self.wait = (wait * 2).min(RETRY_MOST);
        wait
    }

    /// Connected, or the network is back: the next try needn't wait, and
    /// a failure after it waits the least.
    pub(super) fn reset(&mut self) {
        self.at = None;
        self.wait = RETRY_FIRST;
    }

    /// The session is let go (signed out, or refused): nothing is tried
    /// until a new one.
    pub(super) fn stop(&mut self) {
        self.starting = false;
        self.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_start_connects_at_once_without_a_session_dropping() {
        let mut retry = Retry::new();
        let now = Instant::now();
        assert!(
            !retry.due(false, now),
            "nothing to connect before the start"
        );
        retry.start();
        assert!(retry.due(false, now));
        retry.started();
        assert!(!retry.due(false, now));
        assert!(retry.due(true, now), "a dropped session connects again");
    }

    #[test]
    fn failures_wait_longer_each_time_up_to_two_minutes() {
        let mut retry = Retry::new();
        let now = Instant::now();
        let waits: Vec<u64> = (0..7).map(|_| retry.failed(now).as_secs()).collect();
        assert_eq!(waits, [5, 10, 20, 40, 80, 120, 120]);
    }

    #[test]
    fn a_failure_holds_the_next_try_until_its_wait_is_over() {
        let mut retry = Retry::new();
        let now = Instant::now();
        let wait = retry.failed(now);
        assert!(!retry.due(true, now));
        assert!(!retry.due(true, now + wait - Duration::from_millis(1)));
        assert!(retry.due(true, now + wait));
    }

    #[test]
    fn a_reset_tries_at_once_and_starts_the_waits_over() {
        let mut retry = Retry::new();
        let now = Instant::now();
        retry.failed(now);
        retry.failed(now);
        retry.reset();
        assert!(retry.due(true, now));
        assert_eq!(retry.failed(now), RETRY_FIRST);
    }

    #[test]
    fn stopping_ends_the_start_and_the_waits() {
        let mut retry = Retry::new();
        let now = Instant::now();
        retry.start();
        retry.failed(now);
        retry.stop();
        assert!(!retry.due(false, now));
        assert!(retry.due(true, now));
    }
}
