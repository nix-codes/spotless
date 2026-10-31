//! Synced playlists: kept on Spotify too, the same on every device.
//!
//! A synced playlist's songs are a private Spotify playlist's, each known
//! by the ID Spotify gives it there ([`ItemId`]). Where it's up to (its
//! song; Q marks stay on each device) is written in that playlist's
//! description ([`Position`]), as Spotify keeps no data of Spotless's
//! own. Edits, here and on the other devices, meet in [`merge`]: what
//! Spotify has now, with this device's changes since the songs were last
//! the same ([`Link::base`]) made again on it, each by song ID, so it
//! makes no odds whether they were made online or off.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::playlist::EntryId;

/// A song's ID in a Spotify playlist, in hex. A song there twice has two.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ItemId(pub String);

impl ItemId {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        if !self.0.len().is_multiple_of(2) {
            return None;
        }
        (0..self.0.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(self.0.get(i..i + 2)?, 16).ok())
            .collect()
    }

    fn valid(text: &str) -> bool {
        !text.is_empty()
            && text.len().is_multiple_of(2)
            && text.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// What ties a playlist to its Spotify playlist.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    /// The Spotify playlist's URI.
    pub uri: String,
    /// Its name when the songs were last the same here and there: a name
    /// changed since is this device's to send, else Spotify's to take.
    #[serde(default)]
    pub name: String,
    /// Its revision when the songs were last the same here and there, in
    /// hex.
    #[serde(default)]
    pub revision: String,
    /// Its songs then, in order: what this device's changes since are
    /// told from.
    #[serde(default)]
    pub base: Vec<ItemId>,
    /// The position last written or read, so that an unchanged one isn't
    /// written again and an older one doesn't win.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
}

/// Where a synced playlist is up to, as its description holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    /// The cursor's song; None for an empty playlist.
    pub song: Option<ItemId>,
    /// Where to resume it, in seconds: always 0 now, as a song shows
    /// stopped on the other device. Positions are written too seldom for
    /// the time to be right; the field stays for the format.
    pub resume_s: u32,
    /// When it was saved, in seconds since [`EPOCH`]: the newer of two
    /// wins.
    pub saved_at: u64,
    /// Q marks, in the order they play: always none now, as they stay on
    /// each device; the field stays for the format.
    pub marks: Vec<ItemId>,
}

/// 2026-01-01 00:00 UTC, in Unix seconds: when saved times count from.
pub const EPOCH: u64 = 1_767_225_600;

/// What the description starts with: its format, and its version.
const VERSION: &str = "s1";

impl Position {
    /// The description: `s1:song:resume:saved:mark,mark`, where `saved` is
    /// in base 36. Only letters, digits, `:` and `,`, which Spotify keeps
    /// as they are; about 40 characters and 17 a Q mark, of the 512 a
    /// description takes.
    pub fn encode(&self) -> String {
        let song = self.song.as_ref().map_or("", |song| song.0.as_str());
        let marks: Vec<&str> = self.marks.iter().map(|mark| mark.0.as_str()).collect();
        format!(
            "{VERSION}:{song}:{}:{}:{}",
            self.resume_s,
            base36(self.saved_at),
            marks.join(",")
        )
    }

    /// A description written by [`Position::encode`]; None for any other.
    pub fn decode(text: &str) -> Option<Self> {
        let mut fields = text.trim().split(':');
        if fields.next()? != VERSION {
            return None;
        }
        let song = fields.next()?;
        let resume_s = fields.next()?.parse().ok()?;
        let saved_at = u64::from_str_radix(fields.next()?, 36).ok()?;
        let marks = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        let song = match song {
            "" => None,
            song if ItemId::valid(song) => Some(ItemId(song.into())),
            _ => return None,
        };
        let marks = match marks {
            "" => Vec::new(),
            marks => marks
                .split(',')
                .map(|mark| ItemId::valid(mark).then(|| ItemId(mark.into())))
                .collect::<Option<_>>()?,
        };
        Some(Self {
            song,
            resume_s,
            saved_at,
            marks,
        })
    }

    /// Whether it's at the same song as `other`: all a position syncs,
    /// whenever each was saved.
    pub fn same_song(&self, other: &Self) -> bool {
        self.song == other.song
    }
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// A place in a merged list: a song Spotify has, or one added here and
/// not sent yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Item(ItemId),
    New(EntryId),
}

/// The list as it should be: Spotify's songs now (`remote`), with this
/// device's changes since `base` made on it. `local` is this device's list:
/// each entry with its song ID, or None if not sent yet.
///
/// - A song removed here is removed, if still there.
/// - A song moved here goes after the song before it here, if both are
///   still there.
/// - A song added here goes after the song before it here, or at the end
///   if that one is gone.
///
/// Songs added on the other devices stay where they put them.
pub fn merge(base: &[ItemId], local: &[(EntryId, Option<ItemId>)], remote: &[ItemId]) -> Vec<Slot> {
    let here: HashSet<&ItemId> = local.iter().filter_map(|(_, item)| item.as_ref()).collect();
    let removed: HashSet<&ItemId> = base.iter().filter(|item| !here.contains(item)).collect();
    let mut merged: Vec<Slot> = remote
        .iter()
        .filter(|item| !removed.contains(item))
        .cloned()
        .map(Slot::Item)
        .collect();
    let at = |merged: &[Slot], slot: &Slot| merged.iter().position(|s| s == slot);

    // Moves: the songs out of their order of then, fewest first.
    let then: HashMap<&ItemId, usize> = base.iter().enumerate().map(|(i, it)| (it, i)).collect();
    let known: Vec<(usize, &ItemId)> = local
        .iter()
        .enumerate()
        .filter_map(|(i, (_, item))| {
            item.as_ref()
                .filter(|it| then.contains_key(it))
                .map(|it| (i, it))
        })
        .collect();
    let order: Vec<usize> = known.iter().map(|(_, item)| then[item]).collect();
    let stay: HashSet<usize> = longest_increasing(&order)
        .into_iter()
        .map(|k| known[k].0)
        .collect();
    for &(i, item) in &known {
        let slot = Slot::Item(item.clone());
        if stay.contains(&i) || at(&merged, &slot).is_none() {
            continue;
        }
        let Some(from) = at(&merged, &slot) else {
            continue;
        };
        merged.remove(from);
        // After the nearest song before it here that's still there.
        let anchor = local[..i]
            .iter()
            .rev()
            .filter_map(|(_, item)| item.clone())
            .find_map(|item| at(&merged, &Slot::Item(item)));
        merged.insert(anchor.map_or(0, |a| a + 1), slot);
    }

    // Songs added here.
    for (i, (id, item)) in local.iter().enumerate() {
        if item.is_some() {
            continue;
        }
        let to = match i.checked_sub(1).map(|before| &local[before]) {
            None => 0,
            Some((before, None)) => {
                at(&merged, &Slot::New(*before)).map_or(merged.len(), |a| a + 1)
            }
            Some((_, Some(item))) => {
                at(&merged, &Slot::Item(item.clone())).map_or(merged.len(), |a| a + 1)
            }
        };
        merged.insert(to, Slot::New(*id));
    }
    merged
}

/// The indices of a longest strictly increasing subsequence of `values`.
fn longest_increasing(values: &[usize]) -> Vec<usize> {
    // tails[k]: the index ending the best run of length k + 1.
    let mut tails: Vec<usize> = Vec::new();
    let mut before: Vec<Option<usize>> = vec![None; values.len()];
    for (i, &value) in values.iter().enumerate() {
        let k = tails.partition_point(|&t| values[t] < value);
        before[i] = k.checked_sub(1).map(|k| tails[k]);
        if k == tails.len() {
            tails.push(i);
        } else {
            tails[k] = i;
        }
    }
    let mut run = Vec::with_capacity(tails.len());
    let mut at = tails.last().copied();
    while let Some(i) = at {
        run.push(i);
        at = before[i];
    }
    run.reverse();
    run
}

/// One change to a Spotify playlist, by index in the list as it is when
/// the change is made, the ones before it made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Removes `len` songs from `at`.
    Remove { at: usize, len: usize },
    /// Takes the song at `from` out and puts it back at `to`, counted
    /// after it's taken out.
    Move { from: usize, to: usize },
    /// Inserts these songs, added here, before the song at `at`.
    Add { at: usize, songs: Vec<EntryId> },
}

/// The changes that turn Spotify's list (`remote`) into `merged`, in the
/// order to send them.
pub fn changes(remote: &[ItemId], merged: &[Slot]) -> Vec<Change> {
    let wanted: Vec<&ItemId> = merged
        .iter()
        .filter_map(|slot| match slot {
            Slot::Item(item) => Some(item),
            Slot::New(_) => None,
        })
        .collect();
    let kept: HashSet<&ItemId> = wanted.iter().copied().collect();
    let mut changes = Vec::new();

    // Removals, from the end, a run of neighbours at a time.
    let mut now: Vec<&ItemId> = remote.iter().collect();
    let mut i = now.len();
    while i > 0 {
        i -= 1;
        if kept.contains(now[i]) {
            continue;
        }
        let end = i + 1;
        while i > 0 && !kept.contains(now[i - 1]) {
            i -= 1;
        }
        changes.push(Change::Remove {
            at: i,
            len: end - i,
        });
        now.drain(i..end);
    }

    // Moves, each song put in its place in turn.
    for (to, item) in wanted.iter().enumerate() {
        if now[to] == *item {
            continue;
        }
        let Some(from) = now.iter().position(|it| it == item) else {
            continue;
        };
        let moved = now.remove(from);
        now.insert(to, moved);
        changes.push(Change::Move { from, to });
    }

    // Additions, a run of neighbours at a time, in order.
    let mut i = 0;
    while i < merged.len() {
        if let Slot::Item(_) = merged[i] {
            i += 1;
            continue;
        }
        let at = i;
        let mut songs = Vec::new();
        while let Some(Slot::New(id)) = merged.get(i) {
            songs.push(*id);
            i += 1;
        }
        changes.push(Change::Add { at, songs });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(n: u32) -> ItemId {
        ItemId(format!("{n:016x}"))
    }

    fn items(ns: &[u32]) -> Vec<ItemId> {
        ns.iter().copied().map(item).collect()
    }

    /// Local entries: a number is a song sent (its item and entry IDs
    /// alike), a negative one a song added here.
    fn local(ns: &[i64]) -> Vec<(EntryId, Option<ItemId>)> {
        ns.iter()
            .map(|&n| {
                if n < 0 {
                    (EntryId(n.unsigned_abs() + 1000), None)
                } else {
                    (EntryId(n as u64), Some(item(n as u32)))
                }
            })
            .collect()
    }

    /// A merged list in the same notation.
    fn shown(merged: &[Slot]) -> Vec<i64> {
        merged
            .iter()
            .map(|slot| match slot {
                Slot::Item(item) => i64::from_str_radix(&item.0, 16).unwrap(),
                Slot::New(id) => -((id.0 - 1000) as i64),
            })
            .collect()
    }

    /// Makes the changes on `remote`, as Spotify would.
    fn apply(remote: &[ItemId], changes: &[Change]) -> Vec<i64> {
        let mut list: Vec<i64> = shown(&remote.iter().cloned().map(Slot::Item).collect::<Vec<_>>());
        for change in changes {
            match change {
                Change::Remove { at, len } => {
                    list.drain(*at..at + len);
                }
                Change::Move { from, to } => {
                    let song = list.remove(*from);
                    list.insert(*to, song);
                }
                Change::Add { at, songs } => {
                    let new = songs.iter().map(|id| -((id.0 - 1000) as i64));
                    list.splice(*at..*at, new);
                }
            }
        }
        list
    }

    /// Merges, checks the changes make Spotify's list the merged one,
    /// and answers it.
    fn sync(base: &[u32], here: &[i64], there: &[u32]) -> Vec<i64> {
        let remote = items(there);
        let merged = merge(&items(base), &local(here), &remote);
        assert_eq!(apply(&remote, &changes(&remote, &merged)), shown(&merged));
        shown(&merged)
    }

    #[test]
    fn a_position_round_trips_in_few_plain_characters() {
        let position = Position {
            song: Some(ItemId("d671005843c68fbb".into())),
            resume_s: 61,
            saved_at: 25_659_385,
            marks: vec![
                ItemId("a1b2c3d4e5f60718".into()),
                ItemId("0f1e2d3c4b5a6978".into()),
            ],
        };
        let text = position.encode();
        assert_eq!(
            text,
            "s1:d671005843c68fbb:61:f9ywp:a1b2c3d4e5f60718,0f1e2d3c4b5a6978"
        );
        assert!(
            text.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == ':' || c == ',')
        );
        assert_eq!(Position::decode(&text), Some(position));
        let empty = Position::default();
        assert_eq!(empty.encode(), "s1::0:0:");
        assert_eq!(Position::decode(&empty.encode()), Some(empty));
    }

    #[test]
    fn a_description_not_spotlesss_is_no_position() {
        for text in [
            "",
            "My favourite songs",
            "s2:d671005843c68fbb:61:fa0ap:",
            "s1:d671005843c68fbb:61:fa0ap",
            "s1:d671005843c68fbb:61:fa0ap::",
            "s1:not-hex:61:fa0ap:",
            "s1:d671005843c68fbb:soon:fa0ap:",
            "s1:d671005843c68fbb:61:fa0ap:a1,",
        ] {
            assert_eq!(Position::decode(text), None, "{text:?}");
        }
    }

    #[test]
    fn item_ids_are_spotifys_bytes_in_hex() {
        let id = ItemId::from_bytes(&[0xd6, 0x71, 0x00, 0x58]);
        assert_eq!(id.0, "d6710058");
        assert_eq!(id.to_bytes(), Some(vec![0xd6, 0x71, 0x00, 0x58]));
        assert_eq!(ItemId("d67".into()).to_bytes(), None);
    }

    #[test]
    fn with_no_changes_anywhere_nothing_is_sent() {
        let remote = items(&[1, 2, 3]);
        let merged = merge(&remote, &local(&[1, 2, 3]), &remote);
        assert!(changes(&remote, &merged).is_empty());
    }

    #[test]
    fn changes_here_alone_are_made_there() {
        // Removed, added after a song, added first, moved.
        assert_eq!(sync(&[1, 2, 3], &[1, 3], &[1, 2, 3]), [1, 3]);
        assert_eq!(
            sync(&[1, 2, 3], &[1, -7, -8, 2, 3], &[1, 2, 3]),
            [1, -7, -8, 2, 3]
        );
        assert_eq!(sync(&[1, 2], &[-7, 1, 2], &[1, 2]), [-7, 1, 2]);
        assert_eq!(
            sync(&[1, 2, 3, 4], &[4, 1, 2, 3], &[1, 2, 3, 4]),
            [4, 1, 2, 3]
        );
        assert_eq!(
            sync(&[1, 2, 3, 4], &[2, 3, 4, 1], &[1, 2, 3, 4]),
            [2, 3, 4, 1]
        );
        assert_eq!(
            sync(&[1, 2, 3, 4], &[4, 3, 2, 1], &[1, 2, 3, 4]),
            [4, 3, 2, 1]
        );
    }

    #[test]
    fn changes_there_alone_are_taken_as_they_are() {
        assert_eq!(sync(&[1, 2, 3], &[1, 2, 3], &[3, 1, 9]), [3, 1, 9]);
    }

    #[test]
    fn changes_on_both_sides_meet() {
        // Removed here, a song added there.
        assert_eq!(sync(&[1, 2, 3], &[1, 3], &[1, 2, 3, 9]), [1, 3, 9]);
        // Removed on both: no harm.
        assert_eq!(sync(&[1, 2, 3], &[1, 3], &[1, 3]), [1, 3]);
        // Added here after a song removed there: at the end.
        assert_eq!(sync(&[1, 2, 3], &[1, 2, -7, 3], &[1, 3]), [1, 3, -7]);
        // Added here after one added here: they stay together.
        assert_eq!(sync(&[1, 2], &[1, -7, -8, 2], &[2, 1]), [2, 1, -7, -8]);
        // Moved here, removed there: stays removed.
        assert_eq!(sync(&[1, 2, 3], &[3, 1, 2], &[1, 2]), [1, 2]);
        // Moved here after a song removed there: after the one before it.
        assert_eq!(sync(&[1, 2, 3, 4], &[1, 2, 4, 3], &[1, 3, 4]), [1, 4, 3]);
        // Moved here, while songs were added there.
        assert_eq!(sync(&[1, 2, 3], &[3, 1, 2], &[1, 9, 2, 3]), [3, 1, 9, 2]);
    }

    #[test]
    fn a_song_twice_is_two_songs() {
        // 1 and 2 are the same track; only the one removed goes.
        assert_eq!(sync(&[1, 2, 3], &[2, 3], &[1, 2, 3]), [2, 3]);
    }

    #[test]
    fn the_longest_run_in_order_stays() {
        let values = [3, 0, 1, 4, 2];
        let run: Vec<usize> = longest_increasing(&values)
            .iter()
            .map(|&i| values[i])
            .collect();
        assert_eq!(run, [0, 1, 2]);
        assert!(longest_increasing(&[]).is_empty());
    }
}
