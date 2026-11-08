//! A Spotify Code's bars: the 23 heights a media reference is drawn as,
//! and the reference a code's heights carry.

/// Bar height for each 3 bits: neighbouring heights differ in one bit, so
/// a bar misread by one step costs one bit.
const GRAY: [u8; 8] = [0, 1, 3, 2, 7, 6, 4, 5];
const GEN1: [u8; 7] = [1, 0, 1, 1, 0, 1, 1];
const GEN2: [u8; 7] = [1, 1, 1, 1, 0, 0, 1];

/// The 23 bar heights of a media reference.
pub fn encode(reference: u64) -> [u8; 23] {
    let bits = stream(reference);
    let mut heights = [0u8; 23];
    heights[11] = 7;
    for i in 0..20 {
        let n = (bits >> (3 * i) & 1) << 2 | (bits >> (3 * i + 1) & 1) << 1 | bits >> (3 * i + 2) & 1;
        heights[bar(i)] = GRAY[n as usize];
    }
    heights
}

/// Where the i-th of the 20 bars that carry bits is among the 23.
fn bar(i: usize) -> usize {
    if i < 10 { i + 1 } else { i + 2 }
}

/// The media reference the bars carry, allowing one bar misread by one
/// step when that is the only fix that makes them a code. None if nothing
/// fits.
pub fn decode(heights: &[u8; 23]) -> Option<u64> {
    if let Some(reference) = solve(heights) {
        return Some(reference);
    }
    let mut fits = Vec::new();
    for at in (0..20).map(bar) {
        for to in [heights[at].checked_sub(1), Some(heights[at] + 1)] {
            let Some(to) = to.filter(|to| *to <= 7) else {
                continue;
            };
            let mut tried = *heights;
            tried[at] = to;
            fits.extend(solve(&tried));
        }
    }
    (fits.len() == 1).then(|| fits[0])
}

/// The reference of heights measured as fractions: rounded, and failing
/// that, with up to three of the bars nearest halfway between two heights
/// put the other way, when only one such change makes them a code.
pub(super) fn decode_doubtful(measured: &[f32; 23]) -> Option<u64> {
    let heights: [u8; 23] = std::array::from_fn(|i| measured[i].round().clamp(0.0, 7.0) as u8);
    if let Some(reference) = solve(&heights) {
        return Some(reference);
    }
    // The bars carrying bits, most doubtful first, with their other height.
    let mut doubtful: Vec<(usize, u8, f32)> = (0..20)
        .map(bar)
        .filter_map(|at| {
            let off = measured[at] - heights[at] as f32;
            let other = if off >= 0.0 { heights[at] + 1 } else { heights[at].checked_sub(1)? };
            (other <= 7).then_some((at, other, 0.5 - off.abs()))
        })
        .collect();
    doubtful.sort_by(|a, b| a.2.total_cmp(&b.2));
    doubtful.truncate(8);
    let mut fits: Vec<u64> = Vec::new();
    let n = doubtful.len();
    for mask in 1u32..(1 << n) {
        if mask.count_ones() > 3 {
            continue;
        }
        let mut tried = heights;
        for (k, (at, other, _)) in doubtful.iter().enumerate() {
            if mask >> k & 1 == 1 {
                tried[*at] = *other;
            }
        }
        if let Some(reference) = solve(&tried)
            && !fits.contains(&reference)
        {
            fits.push(reference);
        }
    }
    (fits.len() == 1).then(|| fits[0])
}

/// The reference whose code is exactly these heights. The encoding is
/// affine over GF(2) (linear, plus the CRC's final inversion), so the 60
/// bits give 60 equations in the reference's 37 bits.
fn solve(heights: &[u8; 23]) -> Option<u64> {
    if heights[0] != 0 || heights[11] != 7 || heights[22] != 0 {
        return None;
    }
    let mut bits = 0u64;
    for i in 0..20 {
        let n = GRAY.iter().position(|g| *g == heights[bar(i)])? as u64;
        bits |= (n >> 2 & 1) << (3 * i) | (n >> 1 & 1) << (3 * i + 1) | (n & 1) << (3 * i + 2);
    }
    let zero = stream(0);
    let columns: Vec<u64> = (0..37).map(|b| stream(1 << b) ^ zero).collect();
    let target = bits ^ zero;
    // Rows: which bits of the reference, and the bit they must give.
    let mut rows: Vec<(u64, u64)> = (0..60)
        .map(|j| {
            let mask = (0..37).fold(0, |m, b| m | (columns[b] >> j & 1) << b);
            (mask, target >> j & 1)
        })
        .collect();
    for b in 0..37 {
        let pivot = (b..rows.len()).find(|r| rows[*r].0 >> b & 1 == 1)?;
        rows.swap(b, pivot);
        let (mask, value) = rows[b];
        for (r, row) in rows.iter_mut().enumerate() {
            if r != b && row.0 >> b & 1 == 1 {
                *row = (row.0 ^ mask, row.1 ^ value);
            }
        }
    }
    // The other 23 equations hold too, or these aren't a code.
    if rows[37..].iter().any(|(_, value)| *value != 0) {
        return None;
    }
    let reference = (0..37).fold(0, |r, b| r | rows[b].1 << b);
    (encode(reference) == *heights).then_some(reference)
}

/// The 60 bits the bars carry, in bar order (bit 3i is the i-th bar's top
/// bit).
fn stream(reference: u64) -> u64 {
    let mut data = [0u8; 45];
    for (i, bit) in data.iter_mut().take(37).enumerate() {
        *bit = (reference >> i & 1) as u8;
    }
    let check = crc(&data[..37]);
    data[37..].copy_from_slice(&check);
    // Tail-biting: the register starts with the last 6 bits.
    let mut padded = [0u8; 51];
    padded[..6].copy_from_slice(&data[39..]);
    padded[6..].copy_from_slice(&data);
    let mut encoded = Vec::with_capacity(60);
    for k in 0..45 {
        let parity = |g: &[u8; 7]| (0..7).fold(0, |p, j| p ^ padded[k + j] & g[6 - j]);
        // Punctured: of every three bits, the third is dropped.
        for (i, bit) in [parity(&GEN1), parity(&GEN2)].into_iter().enumerate() {
            if (2 * k + i) % 3 != 2 {
                encoded.push(bit);
            }
        }
    }
    (0..60).fold(0, |s, i| s | (encoded[7 * i % 60] as u64) << i)
}

/// CRC-8, polynomial 0x07, bytes reflected in and out, inverted.
fn crc(bits: &[u8]) -> [u8; 8] {
    let mut bytes = [0u8; 5];
    for (i, bit) in bits.iter().enumerate() {
        bytes[i / 8] |= bit << (7 - i % 8);
    }
    let mut register = 0u8;
    for byte in bytes {
        register ^= byte.reverse_bits();
        for _ in 0..8 {
            register = if register & 0x80 != 0 {
                register << 1 ^ 0x07
            } else {
                register << 1
            };
        }
    }
    let check = register.reverse_bits() ^ 0xff;
    std::array::from_fn(|i| check >> (7 - i) & 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boone's example: "Today's Top Hits".
    const HEIGHTS: [u8; 23] = [
        0, 6, 0, 2, 4, 5, 1, 4, 5, 2, 3, 7, 3, 7, 1, 5, 6, 2, 5, 7, 4, 3, 0,
    ];
    const REFERENCE: u64 = 57268659651;

    #[test]
    fn the_published_example_encodes_and_decodes() {
        assert_eq!(encode(REFERENCE), HEIGHTS);
        assert_eq!(decode(&HEIGHTS), Some(REFERENCE));
    }

    #[test]
    fn one_bar_misread_by_a_step_is_put_right() {
        let mut misread = HEIGHTS;
        misread[18] = 4;
        assert_eq!(decode(&misread), Some(REFERENCE));
    }

    #[test]
    fn bars_that_arent_a_code_give_nothing() {
        assert_eq!(decode(&[0; 23]), None);
        let mut scrambled = HEIGHTS;
        scrambled.swap(1, 2);
        scrambled.swap(3, 4);
        assert_eq!(decode(&scrambled), None);
    }

    #[test]
    fn many_references_go_round() {
        for reference in [0, 1, 12345, 16862907074, 75845227563, (1 << 37) - 1] {
            assert_eq!(decode(&encode(reference)), Some(reference));
        }
    }
}
