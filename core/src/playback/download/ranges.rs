//! Sets of byte ranges of a file: what's downloaded, what's asked for.

use std::ops::Range;

/// Disjoint ranges, in order, none empty and none touching another.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ranges(Vec<Range<usize>>);

impl Ranges {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Range<usize>> {
        self.0.iter()
    }

    /// How many bytes they hold.
    pub fn bytes(&self) -> usize {
        self.0.iter().map(|r| r.end - r.start).sum()
    }

    pub fn contains(&self, at: usize) -> bool {
        self.0.iter().any(|r| r.contains(&at))
    }

    /// How many bytes from `at` on they hold without a gap.
    pub fn run_from(&self, at: usize) -> usize {
        self.0
            .iter()
            .find(|r| r.contains(&at))
            .map_or(0, |r| r.end - at)
    }

    pub fn add(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut merged = range;
        let mut kept = Vec::with_capacity(self.0.len() + 1);
        for r in self.0.drain(..) {
            if r.end < merged.start || merged.end < r.start {
                kept.push(r);
            } else {
                merged = merged.start.min(r.start)..merged.end.max(r.end);
            }
        }
        let at = kept.partition_point(|r| r.start < merged.start);
        kept.insert(at, merged);
        self.0 = kept;
    }

    pub fn remove(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let mut kept = Vec::with_capacity(self.0.len() + 1);
        for r in self.0.drain(..) {
            if r.end <= range.start || range.end <= r.start {
                kept.push(r);
                continue;
            }
            if r.start < range.start {
                kept.push(r.start..range.start);
            }
            if range.end < r.end {
                kept.push(range.end..r.end);
            }
        }
        self.0 = kept;
    }

    pub fn remove_all(&mut self, other: &Ranges) {
        for r in other.iter() {
            self.remove(r.clone());
        }
    }

    /// The parts of `range` they don't hold.
    pub fn missing_in(&self, range: Range<usize>) -> Ranges {
        let mut missing = Ranges::new();
        missing.add(range);
        missing.remove_all(self);
        missing
    }

    pub fn union(&self, other: &Ranges) -> Ranges {
        let mut union = self.clone();
        for r in other.iter() {
            union.add(r.clone());
        }
        union
    }

    pub fn first(&self) -> Option<Range<usize>> {
        self.0.first().cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ranges(list: &[Range<usize>]) -> Ranges {
        let mut ranges = Ranges::new();
        for r in list {
            ranges.add(r.clone());
        }
        ranges
    }

    #[test]
    fn adding_merges_what_overlaps_or_touches() {
        let set = ranges(&[10..20, 30..40, 20..25, 50..60, 35..55]);
        assert_eq!(set, Ranges(vec![10..25, 30..60]));
        assert_eq!(set.bytes(), 45);
    }

    #[test]
    fn removing_cuts_and_splits() {
        let mut set = Ranges::new();
        set.add(0..100);
        set.remove(10..20);
        set.remove(90..200);
        assert_eq!(set, Ranges(vec![0..10, 20..90]));
    }

    #[test]
    fn a_run_counts_on_to_the_first_gap() {
        let set = ranges(&[0..10, 20..30]);
        assert_eq!(set.run_from(4), 6);
        assert_eq!(set.run_from(10), 0);
        assert!(set.contains(25) && !set.contains(30));
    }

    #[test]
    fn what_is_missing_in_a_range() {
        let set = ranges(&[0..10, 20..30]);
        assert_eq!(set.missing_in(5..40), Ranges(vec![10..20, 30..40]));
    }
}
