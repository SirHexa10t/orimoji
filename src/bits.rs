//! Bits, most significant first: what every squeeze writes and every alphabet folds.
//!
//! A [`Bits`] is whole bytes and a length, with the unused end of its last byte held at zero, so
//! its bytes are also what it is as bytes, padded. A [`BitWriter`] builds one and a [`BitReader`]
//! reads it back, peeking past its end as zeros, which is what a table-driven decoder needs.

/// A run of bits, most significant first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bits {
    bytes: Vec<u8>,
    len: usize,
}

impl Bits {
    /// No bits.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every bit of these bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Self {
        let len = bytes.len() * 8;
        Self { bytes, len }
    }

    /// How many bits there are.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bits as bytes, the last one padded with zeros.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A reader starting at the first bit.
    pub fn reader(&self) -> BitReader<'_> {
        BitReader { bytes: &self.bytes, len: self.len, at: 0 }
    }
}

/// Builds [`Bits`], most significant bit first.
#[derive(Debug, Default)]
pub struct BitWriter {
    bytes: Vec<u8>,
    acc: u64,
    held: u32,
}

impl BitWriter {
    /// An empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The low `count` bits of `value`, the highest of them first. `count` may be up to 64.
    pub fn write(&mut self, value: u64, count: u32) {
        if count > 32 {
            self.write(value >> 32, count - 32);
            self.write(value & 0xFFFF_FFFF, 32);
            return;
        }
        if count == 0 {
            return;
        }
        self.acc = (self.acc << count) | (value & ((1u64 << count) - 1));
        self.held += count;
        while self.held >= 8 {
            self.held -= 8;
            self.bytes.push((self.acc >> self.held) as u8);
        }
    }

    /// One bit.
    pub fn bit(&mut self, bit: bool) {
        self.write(bit as u64, 1);
    }

    /// Elias gamma, for a number of 1 or more with no known bound: its length less one as zeros,
    /// then the number itself. Small numbers are short: 1 is one bit, 2 and 3 three.
    pub fn gamma(&mut self, n: u64) {
        debug_assert!(n >= 1, "gamma codes start at 1");
        let width = 64 - n.leading_zeros();
        self.write(0, width - 1);
        self.write(n, width);
    }

    /// How many bits have been written.
    pub fn len(&self) -> usize {
        self.bytes.len() * 8 + self.held as usize
    }

    /// Whether nothing has been written.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What was written.
    pub fn finish(mut self) -> Bits {
        let len = self.len();
        if self.held > 0 {
            self.bytes.push((self.acc << (8 - self.held)) as u8);
        }
        Bits { bytes: self.bytes, len }
    }
}

/// Reads [`Bits`] back in the order they were written.
#[derive(Clone, Debug)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    len: usize,
    at: usize,
}

impl<'a> BitReader<'a> {
    /// A reader over every bit of `bytes`.
    pub fn over(bytes: &'a [u8]) -> Self {
        Self { bytes, len: bytes.len() * 8, at: 0 }
    }
}

impl BitReader<'_> {
    /// How many bits are left.
    pub fn remaining(&self) -> usize {
        self.len - self.at
    }

    /// The next `count` bits (up to 64), or `None` when fewer than that are left.
    pub fn read(&mut self, count: u32) -> Option<u64> {
        if count as usize > self.remaining() {
            return None;
        }
        let value = self.peek(count);
        self.at += count as usize;
        Some(value)
    }

    /// The next `count` bits (up to 64) without taking them, reading zeros past the end.
    pub fn peek(&self, count: u32) -> u64 {
        let (mut value, mut at, mut need) = (0u64, self.at, count);
        while need > 0 {
            let byte = match at < self.len {
                true => self.bytes[at / 8] as u64,
                false => 0,
            };
            let offset = (at % 8) as u32;
            let take = need.min(8 - offset);
            let bits = (byte >> (8 - offset - take)) & ((1 << take) - 1);
            value = (value << take) | bits;
            at += take as usize;
            need -= take;
        }
        value
    }

    /// Passes over `count` bits.
    pub fn skip(&mut self, count: usize) {
        self.at = (self.at + count).min(self.len);
    }

    /// One bit.
    pub fn bit(&mut self) -> Option<bool> {
        self.read(1).map(|bit| bit == 1)
    }

    /// An Elias gamma code, as [`BitWriter::gamma`] writes it.
    pub fn gamma(&mut self) -> Option<u64> {
        let mut zeros = 0;
        while !self.bit()? {
            zeros += 1;
            if zeros > 63 {
                return None;
            }
        }
        Some((1u64 << zeros) | self.read(zeros)?)
    }

    /// Whether every bit left is a zero: all that may follow a squeeze's last code is the padding
    /// an alphabet adds to fill its last character.
    pub fn rest_is_zero(&self) -> bool {
        let mut probe = self.clone();
        while probe.remaining() > 0 {
            let take = probe.remaining().min(64) as u32;
            if probe.read(take) != Some(0) {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_written_reads_back_in_the_same_widths() {
        let fields: [(u64, u32); 9] = [
            (1, 1),
            (0, 3),
            (0x5A, 7),
            (u64::MAX, 64),
            (3, 2),
            (0, 33),
            (0xABCDE, 20),
            (1, 1),
            (7, 3),
        ];
        let mut writer = BitWriter::new();
        for (value, count) in fields {
            writer.write(value, count);
        }
        let bits = writer.finish();
        assert_eq!(bits.len(), fields.iter().map(|&(_, count)| count as usize).sum::<usize>());
        let mut reader = bits.reader();
        for (value, count) in fields {
            let mask = if count == 64 { u64::MAX } else { (1 << count) - 1 };
            assert_eq!(reader.read(count), Some(value & mask));
        }
        assert_eq!(reader.read(1), None, "nothing is left past the end");
    }

    #[test]
    fn gamma_codes_are_short_for_small_numbers_and_read_back() {
        let mut writer = BitWriter::new();
        writer.gamma(1);
        assert_eq!(writer.len(), 1, "1 is a single bit");
        for n in [2, 3, 4, 1000, u64::MAX] {
            writer.gamma(n);
        }
        let bits = writer.finish();
        let mut reader = bits.reader();
        for n in [1, 2, 3, 4, 1000, u64::MAX] {
            assert_eq!(reader.gamma(), Some(n));
        }
    }

    #[test]
    fn a_peek_past_the_end_reads_zeros_and_takes_nothing() {
        let mut writer = BitWriter::new();
        writer.write(0b101, 3);
        let bits = writer.finish();
        let reader = bits.reader();
        assert_eq!(reader.peek(8), 0b1010_0000);
        assert_eq!(reader.remaining(), 3);
        assert!(!reader.rest_is_zero());
        let mut reader = bits.reader();
        reader.skip(1);
        reader.skip(1);
        assert!(!reader.rest_is_zero(), "the last bit is a one");
        reader.skip(1);
        assert!(reader.rest_is_zero(), "nothing left counts as all zeros");
    }
}
