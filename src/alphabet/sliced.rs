//! The alphabets of a power-of-two size, which take a fixed number of bits a character.
//!
//! Folding bits is plain: the last character is filled out with zeros. Folding bytes has to say
//! where they end. Up to 8 bits a character, the zeros at the end never make a whole byte, so the
//! number of characters says it. Past 8 bits they could, so an alphabet of `k` bits has a SHORT
//! set of `2^(k-8)` more characters: when the last group of bits would leave a byte's worth of
//! zeros, it is written in a short character instead, which holds `k - 8` bits.

use super::Problem;
use super::tables;
use crate::bits::{BitReader, BitWriter, Bits};
use std::sync::OnceLock;

pub(super) struct Sliced {
    /// Bits a character holds.
    bits: u32,
    chars: Chars,
}

/// Where an alphabet's characters come from: its main set, then its short set.
enum Chars {
    /// Up to 64 ASCII characters, and how to read each byte back (`NONE` where it isn't one).
    Ascii {
        digits: &'static [u8],
        reverse: [u8; 128],
    },
    Braille,
    /// A frozen table, in code point order.
    Table(fn() -> &'static [char]),
    /// Runs of code points.
    Ranges {
        main: &'static [(u32, u32)],
        short: &'static [(u32, u32)],
    },
}

const NONE: u8 = u8::MAX;

/// What each ASCII byte stands for; with `fold_case`, a letter's other case too.
const fn reverse(digits: &[u8], fold_case: bool) -> [u8; 128] {
    let mut table = [NONE; 128];
    let mut i = 0;
    while i < digits.len() {
        table[digits[i] as usize] = i as u8;
        if fold_case && digits[i].is_ascii_alphabetic() {
            table[(digits[i] ^ 0x20) as usize] = i as u8;
        }
        i += 1;
    }
    table
}

const HEX_DIGITS: &[u8] = b"0123456789abcdef";
const BASE32_DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
const CROCKFORD_DIGITS: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE64URL_DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub(super) static HEX: Sliced = Sliced {
    bits: 4,
    chars: Chars::Ascii { digits: HEX_DIGITS, reverse: reverse(HEX_DIGITS, true) },
};
pub(super) static BASE32: Sliced = Sliced {
    bits: 5,
    chars: Chars::Ascii { digits: BASE32_DIGITS, reverse: reverse(BASE32_DIGITS, true) },
};
pub(super) static CROCKFORD: Sliced = Sliced {
    bits: 5,
    chars: Chars::Ascii {
        digits: CROCKFORD_DIGITS,
        reverse: {
            // What people type for the digits that were left out because they look alike.
            let mut table = reverse(CROCKFORD_DIGITS, true);
            table[b'O' as usize] = 0;
            table[b'o' as usize] = 0;
            table[b'I' as usize] = 1;
            table[b'i' as usize] = 1;
            table[b'L' as usize] = 1;
            table[b'l' as usize] = 1;
            table
        },
    },
};
pub(super) static BASE64URL: Sliced = Sliced {
    bits: 6,
    chars: Chars::Ascii { digits: BASE64URL_DIGITS, reverse: reverse(BASE64URL_DIGITS, false) },
};
pub(super) static BRAILLE: Sliced = Sliced { bits: 8, chars: Chars::Braille };
pub(super) static TWO_BYTE: Sliced = Sliced { bits: 10, chars: Chars::Table(two_byte) };
pub(super) static ONE_CELL: Sliced = Sliced { bits: 13, chars: Chars::Table(one_cell) };
pub(super) static CJK: Sliced = Sliced {
    bits: 15,
    chars: Chars::Ranges {
        main: &[(0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xAC00, 0xC03F)],
        short: &[(0xC040, 0xC0BF)],
    },
};
pub(super) static CJK_EXT_B: Sliced = Sliced {
    bits: 16,
    chars: Chars::Ranges {
        main: &[(0x3400, 0x4DBF), (0x4E00, 0x9FFF), (0xAC00, 0xD7A3), (0x20000, 0x2689B)],
        short: &[(0x2689C, 0x2699B)],
    },
};

fn two_byte() -> &'static [char] {
    static CHARS: OnceLock<Vec<char>> = OnceLock::new();
    CHARS.get_or_init(|| tables::TWO_BYTE.chars().collect())
}

fn one_cell() -> &'static [char] {
    static CHARS: OnceLock<Vec<char>> = OnceLock::new();
    CHARS.get_or_init(|| tables::ONE_CELL.chars().collect())
}

impl Sliced {
    fn main_len(&self) -> u32 {
        1 << self.bits
    }

    /// Bits a short character holds; 0 for an alphabet with no short set.
    fn short_bits(&self) -> u32 {
        self.bits.saturating_sub(8)
    }

    /// Character `index` of the main set followed by the short set.
    fn char_at(&self, index: u32) -> char {
        match &self.chars {
            Chars::Ascii { digits, .. } => digits[index as usize] as char,
            Chars::Braille => char::from_u32(0x2800 + index).expect("braille"),
            Chars::Table(table) => table()[index as usize],
            Chars::Ranges { main, short } => {
                let mut index = index;
                for &(low, high) in main.iter().chain(short.iter()) {
                    if index <= high - low {
                        return char::from_u32(low + index).expect("assigned");
                    }
                    index -= high - low + 1;
                }
                unreachable!("an index past the alphabet")
            }
        }
    }

    /// Where `c` is in the main set followed by the short set, reading what people type for it too.
    fn index_of(&self, c: char) -> Option<u32> {
        match &self.chars {
            Chars::Ascii { reverse, .. } => match reverse.get(c as usize) {
                Some(&index) if index != NONE => Some(index as u32),
                _ => None,
            },
            Chars::Braille => (0x2800..=0x28FF).contains(&(c as u32)).then(|| c as u32 - 0x2800),
            Chars::Table(table) => table().binary_search(&c).ok().map(|index| index as u32),
            Chars::Ranges { main, short } => {
                let mut before = 0;
                for &(low, high) in main.iter().chain(short.iter()) {
                    if (low..=high).contains(&(c as u32)) {
                        return Some(before + c as u32 - low);
                    }
                    before += high - low + 1;
                }
                None
            }
        }
    }

    /// Where `c` is in the main set.
    pub(super) fn main_index(&self, c: char) -> Option<u32> {
        self.index_of(c).filter(|&index| index < self.main_len())
    }

    /// Whether a fold writes `c`, as spelled rather than a variant read for it: one of the main set,
    /// or of the short set, which ends a fold of bytes.
    pub(super) fn writes(&self, c: char) -> bool {
        self.index_of(c).is_some_and(|index| self.char_at(index) == c)
    }

    pub(super) fn fold_bits(&self, bits: &Bits) -> String {
        self.fold(&mut bits.reader(), false)
    }

    pub(super) fn fold_bytes(&self, bytes: &[u8]) -> String {
        self.fold(&mut BitReader::over(bytes), true)
    }

    fn fold(&self, reader: &mut BitReader<'_>, exact: bool) -> String {
        let k = self.bits;
        let mut out = String::with_capacity(reader.remaining() / k as usize * 3 + 4);
        while reader.remaining() >= k as usize {
            out.push(self.char_at(reader.read(k).expect("enough bits") as u32));
        }
        let rest = reader.remaining() as u32;
        if rest > 0 {
            let value = reader.read(rest).expect("the rest") as u32;
            let short = self.short_bits();
            match exact && rest <= short {
                true => out.push(self.char_at(self.main_len() + (value << (short - rest)))),
                false => out.push(self.char_at(value << (k - rest))),
            }
        }
        out
    }

    pub(super) fn unfold_bits(&self, text: &str) -> Result<Bits, (usize, Problem)> {
        let mut writer = BitWriter::new();
        for (at, c) in text.chars().enumerate() {
            writer.write(self.main_index(c).ok_or((at, Problem::Foreign(c)))? as u64, self.bits);
        }
        Ok(writer.finish())
    }

    pub(super) fn unfold_bytes(&self, text: &str) -> Result<Vec<u8>, (usize, Problem)> {
        let (k, short) = (self.bits, self.short_bits());
        let mut writer = BitWriter::new();
        let (mut chars, mut ends_short) = (0usize, false);
        for (at, c) in text.chars().enumerate() {
            let index = self.index_of(c).ok_or((at, Problem::Foreign(c)))?;
            if ends_short {
                // A short character holds the last bits; nothing may follow it.
                return Err((at - 1, Problem::BadEnd));
            }
            match index < self.main_len() {
                true => writer.write(index as u64, k),
                false => {
                    writer.write((index - self.main_len()) as u64, short);
                    ends_short = true;
                }
            }
            chars = at + 1;
        }
        let bits = writer.finish();
        let bytes = bits.len() / 8;
        // The characters a fold of that many bytes writes, and whether it ends short.
        let (full, rest) = (bytes * 8 / k as usize, (bytes * 8 % k as usize) as u32);
        let (expected, expect_short) = (full + (rest > 0) as usize, rest > 0 && rest <= short);
        let mut tail = bits.reader();
        tail.skip(bytes * 8);
        if chars != expected || ends_short != expect_short || !tail.rest_is_zero() {
            return Err((chars.saturating_sub(1), Problem::BadEnd));
        }
        let mut reader = bits.reader();
        Ok((0..bytes).map(|_| reader.read(8).expect("whole bytes") as u8).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frozen_tables_are_sorted_and_of_their_sizes() {
        for (table, main, short) in [(two_byte(), 1024, 4), (one_cell(), 8192, 32)] {
            assert_eq!(table.len(), main + short);
            assert!(table.windows(2).all(|pair| pair[0] < pair[1]), "binary search needs order");
        }
        assert!(two_byte().iter().all(|&c| c.len_utf8() == 2));
        assert!(one_cell().iter().all(|&c| c.len_utf8() == 3));
    }

    #[test]
    fn the_ranges_hold_exactly_their_sizes() {
        for (sliced, size) in [(&CJK, 32768 + 128), (&CJK_EXT_B, 65536 + 256)] {
            let Chars::Ranges { main, short } = &sliced.chars else { unreachable!() };
            let count: u32 =
                main.iter().chain(short.iter()).map(|(low, high)| high - low + 1).sum();
            assert_eq!(count, size);
            for index in [0, 6591, 6592, 32767, 32768, size - 1] {
                assert_eq!(sliced.index_of(sliced.char_at(index)), Some(index));
            }
        }
    }

    #[test]
    fn a_short_last_character_says_how_many_bytes_there_were() {
        // Two bytes are 16 bits: one 13-bit character and 3 bits, which fit a short one (5 bits).
        let text = ONE_CELL.fold_bytes(&[0xAB, 0xCD]);
        assert_eq!(text.chars().count(), 2);
        assert!(ONE_CELL.main_index(text.chars().last().unwrap()).is_none(), "the last is short");
        assert_eq!(ONE_CELL.unfold_bytes(&text).unwrap(), [0xAB, 0xCD]);
        // Folded as bits instead, the same two bytes end in a main character, padded.
        let bits = ONE_CELL.fold_bits(&Bits::from_bytes(vec![0xAB, 0xCD]));
        assert!(ONE_CELL.main_index(bits.chars().last().unwrap()).is_some());
    }

    #[test]
    fn an_end_no_fold_writes_is_refused() {
        assert_eq!(BASE64URL.unfold_bytes("A"), Err((0, Problem::BadEnd)), "6 bits make no byte");
        assert_eq!(BASE64URL.unfold_bytes("AB"), Err((1, Problem::BadEnd)), "padding isn't zero");
        assert_eq!(BASE64URL.unfold_bytes("AQ"), Ok(vec![1]));
    }
}
