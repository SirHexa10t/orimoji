//! Canonical Huffman codes: a code length for each symbol is all a folded file stores. The codes
//! follow from the lengths alone (shorter codes first, those of one length in symbol order,
//! counting up), so two builds agree on every bit.

use crate::bits::{BitReader, BitWriter};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// The longest code allowed. Past it, counts are flattened until the code fits; only counts shaped
/// like the Fibonacci numbers, over millions of symbols, ever get there.
pub const MAX_LENGTH: u32 = 30;

/// How many leading bits the decoder resolves with one table lookup.
const TABLE_BITS: u32 = 10;

/// The code length each symbol gets for its count, 0 for a symbol that never occurs. A lone symbol
/// still gets one bit, so that it is there to be read. Ties go to the earlier symbol, so the same
/// counts always give the same code.
pub fn lengths(counts: &[u64]) -> Vec<u32> {
    let mut counts = counts.to_vec();
    loop {
        let lengths = unlimited(&counts);
        if lengths.iter().all(|&length| length <= MAX_LENGTH) {
            return lengths;
        }
        for count in counts.iter_mut().filter(|count| **count > 0) {
            *count = (*count >> 1) | 1;
        }
    }
}

/// Huffman's merge of the two rarest, over and over, with no limit on the length.
fn unlimited(counts: &[u64]) -> Vec<u32> {
    let used: Vec<usize> = (0..counts.len()).filter(|&symbol| counts[symbol] > 0).collect();
    let mut lengths = vec![0; counts.len()];
    match used[..] {
        [] => return lengths,
        [only] => {
            lengths[only] = 1;
            return lengths;
        }
        _ => {}
    }
    // Leaves are the nodes 0..used.len(); each merge makes the next node, so a parent always
    // comes after its children.
    let mut parent = vec![0; 2 * used.len() - 1];
    let mut heap: BinaryHeap<Reverse<(u64, usize)>> =
        used.iter().enumerate().map(|(node, &symbol)| Reverse((counts[symbol], node))).collect();
    let mut next = used.len();
    while let (Some(Reverse((a, first))), Some(Reverse((b, second)))) = (heap.pop(), heap.pop()) {
        parent[first] = next;
        parent[second] = next;
        heap.push(Reverse((a + b, next)));
        next += 1;
    }
    let mut depth = vec![0u32; next];
    for node in (0..next - 1).rev() {
        depth[node] = depth[parent[node]] + 1;
    }
    for (leaf, &symbol) in used.iter().enumerate() {
        lengths[symbol] = depth[leaf];
    }
    lengths
}

/// Whether these lengths make a prefix code at all: none past [`MAX_LENGTH`], and no more codes
/// of each length than the shorter ones leave room for.
pub fn is_prefix_code(lengths: &[u32]) -> bool {
    let room = 1u64 << MAX_LENGTH;
    let mut taken = 0u64;
    for &length in lengths {
        if length > MAX_LENGTH {
            return false;
        }
        if length > 0 {
            taken += room >> length;
        }
    }
    taken <= room
}

/// The code each symbol's length gives it, canonically.
fn codes(lengths: &[u32]) -> Vec<u32> {
    let mut codes = vec![0; lengths.len()];
    let (mut code, mut length) = (0u32, 0u32);
    for symbol in by_code(lengths) {
        code <<= lengths[symbol] - length;
        length = lengths[symbol];
        codes[symbol] = code;
        code += 1;
    }
    codes
}

/// The symbols that have codes, in the order their codes count up in.
fn by_code(lengths: &[u32]) -> Vec<usize> {
    let mut symbols: Vec<usize> = (0..lengths.len()).filter(|&s| lengths[s] > 0).collect();
    symbols.sort_by_key(|&symbol| (lengths[symbol], symbol));
    symbols
}

/// Writes symbols in their codes.
#[derive(Clone, Debug)]
pub struct Encoder {
    lengths: Vec<u32>,
    codes: Vec<u32>,
}

impl Encoder {
    /// The code these lengths give, if they make a prefix code.
    pub fn new(lengths: Vec<u32>) -> Option<Self> {
        is_prefix_code(&lengths).then(|| Self { codes: codes(&lengths), lengths })
    }

    /// Writes `symbol`, which must have a code.
    pub fn write(&self, writer: &mut BitWriter, symbol: usize) {
        debug_assert!(self.lengths[symbol] > 0, "symbol {symbol} has no code");
        writer.write(self.codes[symbol] as u64, self.lengths[symbol]);
    }

    /// How long `symbol`'s code is, 0 when it has none.
    pub fn length(&self, symbol: usize) -> u32 {
        self.lengths[symbol]
    }

    /// Every symbol's length.
    pub fn lengths(&self) -> &[u32] {
        &self.lengths
    }
}

/// Reads symbols back out of their codes.
#[derive(Clone, Debug)]
pub struct Decoder {
    /// For each length: its first code, how many codes it has, and where its symbols start in
    /// `symbols`.
    first: Vec<u32>,
    count: Vec<u32>,
    start: Vec<u32>,
    symbols: Vec<u32>,
    longest: u32,
    /// Indexed by the next `table_bits` bits: a symbol and its length (`symbol << 5 | length`), or
    /// 0 where the code is longer than the table.
    table: Vec<u32>,
    table_bits: u32,
}

impl Decoder {
    /// The decoder for these lengths, if they make a prefix code.
    pub fn new(lengths: &[u32]) -> Option<Self> {
        if !is_prefix_code(lengths) {
            return None;
        }
        let longest = lengths.iter().copied().max().unwrap_or(0);
        let mut count = vec![0u32; longest as usize + 1];
        for &length in lengths.iter().filter(|&&length| length > 0) {
            count[length as usize] += 1;
        }
        let (mut first, mut start) = (vec![0u32; count.len()], vec![0u32; count.len()]);
        let (mut code, mut at) = (0u32, 0u32);
        for length in 1..count.len() {
            first[length] = code;
            start[length] = at;
            code = (code + count[length]) << 1;
            at += count[length];
        }
        let symbols: Vec<u32> = by_code(lengths).into_iter().map(|symbol| symbol as u32).collect();
        let table_bits = longest.min(TABLE_BITS);
        let mut table = vec![0u32; 1 << table_bits];
        let codes = codes(lengths);
        for (symbol, &length) in lengths.iter().enumerate() {
            if length > 0 && length <= table_bits {
                let base = (codes[symbol] << (table_bits - length)) as usize;
                for slot in &mut table[base..base + (1 << (table_bits - length))] {
                    *slot = (symbol as u32) << 5 | length;
                }
            }
        }
        Some(Self { first, count, start, symbols, longest, table, table_bits })
    }

    /// The next symbol, or `None` when the bits run out first or spell no code.
    pub fn read(&self, reader: &mut BitReader<'_>) -> Option<usize> {
        if self.table_bits > 0 {
            let entry = self.table[reader.peek(self.table_bits) as usize];
            if entry != 0 {
                let length = (entry & 31) as usize;
                if length > reader.remaining() {
                    return None;
                }
                reader.skip(length);
                return Some((entry >> 5) as usize);
            }
        }
        let mut code = 0u32;
        for length in 1..=self.longest as usize {
            code = (code << 1) | reader.read(1)? as u32;
            let offset = code.wrapping_sub(self.first[length]);
            if code >= self.first[length] && offset < self.count[length] {
                return Some(self.symbols[(self.start[length] + offset) as usize] as usize);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kraft(lengths: &[u32]) -> f64 {
        lengths.iter().filter(|&&l| l > 0).map(|&l| 0.5f64.powi(l as i32)).sum()
    }

    #[test]
    fn huffman_lengths_are_complete_and_favour_the_common() {
        let lengths = lengths(&[50, 0, 10, 10, 5, 1, 1]);
        assert_eq!(lengths[1], 0, "a symbol that never occurs gets no code");
        assert!((kraft(&lengths) - 1.0).abs() < 1e-12, "a complete code wastes nothing");
        assert!(lengths[0] < lengths[2] && lengths[2] <= lengths[5]);
    }

    #[test]
    fn nothing_and_one_symbol_are_coded_too() {
        assert_eq!(lengths(&[0, 0]), [0, 0]);
        assert_eq!(lengths(&[0, 7]), [0, 1]);
    }

    #[test]
    fn counts_that_would_need_long_codes_are_flattened_to_fit() {
        // Fibonacci counts make the deepest possible tree.
        let mut counts = vec![1u64, 1];
        while counts.len() < 60 {
            counts.push(counts[counts.len() - 1] + counts[counts.len() - 2]);
        }
        let lengths = lengths(&counts);
        assert!(lengths.iter().all(|&length| (1..=MAX_LENGTH).contains(&length)));
        assert!(kraft(&lengths) <= 1.0);
    }

    #[test]
    fn codes_read_back_as_their_symbols() {
        let counts: Vec<u64> = (0..3000).map(|symbol| 1 + (symbol * 7919 % 1000)).collect();
        let lengths = lengths(&counts);
        let encoder = Encoder::new(lengths.clone()).unwrap();
        let decoder = Decoder::new(&lengths).unwrap();
        let message: Vec<usize> = (0..20_000).map(|i| (i * 31 + i / 7) % 3000).collect();
        let mut writer = BitWriter::new();
        for &symbol in &message {
            encoder.write(&mut writer, symbol);
        }
        let bits = writer.finish();
        let mut reader = bits.reader();
        for &symbol in &message {
            assert_eq!(decoder.read(&mut reader), Some(symbol));
        }
        assert_eq!(decoder.read(&mut reader), None, "the bits are used up");
    }

    #[test]
    fn lengths_that_are_no_prefix_code_are_refused() {
        assert!(Decoder::new(&[1, 1, 1]).is_none());
        assert!(Encoder::new(vec![MAX_LENGTH + 1]).is_none());
        assert!(Decoder::new(&[1, 2, 2]).is_some());
    }
}
