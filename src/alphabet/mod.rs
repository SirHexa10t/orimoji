//! The alphabets a fold writes in, and what each costs.
//!
//! Every alphabet folds two kinds of input:
//!
//! - **Bytes**, exactly: [`Alphabet::fold_bytes`] and [`Alphabet::unfold_bytes`] give back the
//!   same bytes, however many there were.
//! - **Bits** that know where they end, as every squeeze's are: [`Alphabet::fold_bits`] fills the
//!   last character with zeros, and [`Alphabet::unfold_bits`] gives those zeros back too.
//!
//! The alphabets past ASCII hold only characters that survive being text: no combining marks, no
//! right-to-left letters, nothing invisible, nothing Unicode normalization changes. Their tables
//! are frozen, since text folded with them must unfold forever.

mod blocks;
mod number;
mod sliced;
mod tables;

use crate::bits::Bits;
use std::fmt;

/// An alphabet to fold into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Alphabet {
    /// `0-9 a-f`, 4 bits a character.
    Hex,
    /// RFC 4648's `A-Z 2-7`, 5 bits a character.
    Base32,
    /// Crockford's base32, `0-9 A-Z` without `I L O U`: read back in either case, and with `O`
    /// taken for `0` and `I`, `L` for `1`, so it survives being typed.
    Crockford,
    /// Bitcoin's base58: no `0 O I l`, nothing a double-click stops at.
    Base58,
    /// URL-safe base64, `A-Z a-z 0-9 - _`, 6 bits a character, no padding.
    Base64Url,
    /// ZeroMQ's Z85: four bytes in five characters.
    Z85,
    /// Joachim Henke's basE91: 13 or 14 bits in two characters.
    Base91,
    /// All of printable ASCII but space, each line read as one number.
    Base94,
    /// The 256 braille patterns, each byte drawn as its dots.
    Braille,
    /// 1024 letters (Latin, IPA, Greek, Cyrillic, Armenian) that UTF-8 writes in two bytes, one
    /// cell wide, 10 bits a character.
    TwoByte,
    /// 8192 one-cell characters that UTF-8 writes in three bytes, 13 bits a character.
    OneCell,
    /// CJK ideographs and Hangul, 15 bits a character, two cells wide.
    Cjk,
    /// The same with Hangul's rest and CJK Extension B, 16 bits a character.
    CjkExtB,
}

impl Alphabet {
    /// Every alphabet, the fewest bits a character first.
    pub const ALL: [Alphabet; 13] = [
        Alphabet::Hex,
        Alphabet::Base32,
        Alphabet::Crockford,
        Alphabet::Base58,
        Alphabet::Base64Url,
        Alphabet::Z85,
        Alphabet::Base91,
        Alphabet::Base94,
        Alphabet::Braille,
        Alphabet::TwoByte,
        Alphabet::OneCell,
        Alphabet::Cjk,
        Alphabet::CjkExtB,
    ];

    /// The name a folded file and the command line call it by.
    pub fn name(self) -> &'static str {
        match self {
            Alphabet::Hex => "hex",
            Alphabet::Base32 => "base32",
            Alphabet::Crockford => "crockford",
            Alphabet::Base58 => "base58",
            Alphabet::Base64Url => "base64url",
            Alphabet::Z85 => "z85",
            Alphabet::Base91 => "base91",
            Alphabet::Base94 => "base94",
            Alphabet::Braille => "braille",
            Alphabet::TwoByte => "two-byte",
            Alphabet::OneCell => "one-cell",
            Alphabet::Cjk => "cjk",
            Alphabet::CjkExtB => "cjk-ext-b",
        }
    }

    /// The alphabet called `name`; `base64` is taken for `base64url`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "base64" => Some(Alphabet::Base64Url),
            _ => Self::ALL.into_iter().find(|alphabet| alphabet.name() == name),
        }
    }

    /// Its characters, in words.
    pub fn characters(self) -> &'static str {
        match self {
            Alphabet::Hex => "0-9 a-f",
            Alphabet::Base32 => "A-Z 2-7",
            Alphabet::Crockford => "0-9 A-Z without I L O U",
            Alphabet::Base58 => "1-9 A-Z a-z without I O l",
            Alphabet::Base64Url => "A-Z a-z 0-9 - _",
            Alphabet::Z85 => "0-9 a-z A-Z .-:+=^!/*?&<>()[]{}@%$#",
            Alphabet::Base91 => "! to ~ without - \\ '",
            Alphabet::Base94 => "! to ~",
            Alphabet::Braille => "U+2800-U+28FF",
            Alphabet::TwoByte => "Latin, IPA, Greek, Cyrillic, Armenian letters",
            Alphabet::OneCell => "symbols, syllabaries, scripts of U+0800-U+FFFF",
            Alphabet::Cjk => "U+3400-U+4DBF, U+4E00-U+9FFF, U+AC00-U+C03F",
            Alphabet::CjkExtB => "the CJK ones, U+C040-U+D7A3, U+20000-U+2689B",
        }
    }

    /// How many characters it has.
    pub fn size(self) -> usize {
        match self {
            Alphabet::Hex => 16,
            Alphabet::Base32 | Alphabet::Crockford => 32,
            Alphabet::Base58 => 58,
            Alphabet::Base64Url => 64,
            Alphabet::Z85 => 85,
            Alphabet::Base91 => 91,
            Alphabet::Base94 => 94,
            Alphabet::Braille => 256,
            Alphabet::TwoByte => 1024,
            Alphabet::OneCell => 8192,
            Alphabet::Cjk => 32768,
            Alphabet::CjkExtB => 65536,
        }
    }

    /// The bits a character holds, on average.
    pub fn bits_per_char(self) -> f64 {
        match self {
            Alphabet::Z85 => 6.4,
            Alphabet::Base91 => 6.5,
            _ => (self.size() as f64).log2(),
        }
    }

    /// The bytes UTF-8 spends on a character, on average.
    pub fn bytes_per_char(self) -> f64 {
        match self {
            Alphabet::Braille | Alphabet::OneCell | Alphabet::Cjk => 3.0,
            Alphabet::TwoByte => 2.0,
            // 38,756 of its characters are in the BMP, at three bytes; 26,780 are past it, at four.
            Alphabet::CjkExtB => (38_756.0 * 3.0 + 26_780.0 * 4.0) / 65_536.0,
            _ => 1.0,
        }
    }

    /// The cells a terminal draws a character in.
    pub fn cells_per_char(self) -> usize {
        match self {
            Alphabet::Cjk | Alphabet::CjkExtB => 2,
            _ => 1,
        }
    }

    /// Whether `c` is one of its characters, as it writes them.
    pub fn contains(self, c: char) -> bool {
        let ascii = |digits: &[u8]| c.is_ascii() && digits.contains(&(c as u8));
        match self {
            Alphabet::Z85 => ascii(blocks::Z85),
            Alphabet::Base91 => ascii(blocks::BASE91),
            Alphabet::Base58 => ascii(number::BASE58.digits),
            Alphabet::Base94 => c.is_ascii_graphic(),
            _ => self.sliced().expect("sliced").writes(c),
        }
    }

    /// `bytes`, folded so that [`Alphabet::unfold_bytes`] gives exactly them back.
    pub fn fold_bytes(self, bytes: &[u8]) -> String {
        match self {
            Alphabet::Z85 => blocks::z85_fold(bytes),
            Alphabet::Base91 => blocks::base91_fold(bytes),
            Alphabet::Base58 => number::BASE58.fold(bytes),
            Alphabet::Base94 => number::BASE94.fold(bytes),
            _ => self.sliced().expect("every other alphabet is sliced").fold_bytes(bytes),
        }
    }

    /// The bytes [`Alphabet::fold_bytes`] folded into `text`.
    pub fn unfold_bytes(self, text: &str) -> Result<Vec<u8>, AlphabetError> {
        let problem = |(at, problem)| AlphabetError { alphabet: self, at, problem };
        match self {
            Alphabet::Z85 => blocks::z85_unfold(text).map_err(problem),
            Alphabet::Base91 => blocks::base91_unfold(text).map_err(problem),
            Alphabet::Base58 => number::BASE58.unfold(text).map_err(problem),
            Alphabet::Base94 => number::BASE94.unfold(text).map_err(problem),
            _ => self.sliced().expect("sliced").unfold_bytes(text).map_err(problem),
        }
    }

    /// `bits`, folded with the last character filled out with zeros.
    pub fn fold_bits(self, bits: &Bits) -> String {
        match self.sliced() {
            Some(sliced) => sliced.fold_bits(bits),
            None => self.fold_bytes(bits.as_bytes()),
        }
    }

    /// The bits [`Alphabet::fold_bits`] folded into `text`, with the zeros that filled out the
    /// last character.
    pub fn unfold_bits(self, text: &str) -> Result<Bits, AlphabetError> {
        match self.sliced() {
            Some(sliced) => sliced.unfold_bits(text).map_err(|(at, problem)| AlphabetError {
                alphabet: self,
                at,
                problem,
            }),
            None => self.unfold_bytes(text).map(Bits::from_bytes),
        }
    }

    fn sliced(self) -> Option<&'static sliced::Sliced> {
        match self {
            Alphabet::Hex => Some(&sliced::HEX),
            Alphabet::Base32 => Some(&sliced::BASE32),
            Alphabet::Crockford => Some(&sliced::CROCKFORD),
            Alphabet::Base64Url => Some(&sliced::BASE64URL),
            Alphabet::Braille => Some(&sliced::BRAILLE),
            Alphabet::TwoByte => Some(&sliced::TWO_BYTE),
            Alphabet::OneCell => Some(&sliced::ONE_CELL),
            Alphabet::Cjk => Some(&sliced::CJK),
            Alphabet::CjkExtB => Some(&sliced::CJK_EXT_B),
            Alphabet::Z85 | Alphabet::Base91 | Alphabet::Base58 | Alphabet::Base94 => None,
        }
    }
}

impl fmt::Display for Alphabet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Text that an alphabet didn't write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlphabetError {
    pub alphabet: Alphabet,
    /// Which character, counting from 0.
    pub at: usize,
    pub problem: Problem,
}

/// What was wrong with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// A character that isn't in the alphabet.
    Foreign(char),
    /// Characters that end part-way into a byte, or a padding that isn't zeros, which no fold
    /// writes.
    BadEnd,
    /// A group that spells a number too big for the bytes it stands for.
    TooBig,
}

impl fmt::Display for AlphabetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let alphabet = self.alphabet;
        match self.problem {
            Problem::Foreign(c) => {
                write!(f, "character {} ({c:?}, U+{:04X}) is not {alphabet}", self.at + 1, c as u32)
            }
            Problem::BadEnd => write!(f, "the {alphabet} ends in a way no fold writes it"),
            Problem::TooBig => {
                write!(f, "the {alphabet} group at character {} is too big", self.at + 1)
            }
        }
    }
}

impl std::error::Error for AlphabetError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::BitWriter;

    fn noise(len: usize, seed: u64) -> Vec<u8> {
        let mut x = seed | 1;
        (0..len)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn every_alphabet_gives_back_bytes_of_every_length() {
        for alphabet in Alphabet::ALL {
            for len in (0..70).chain([255, 1000]) {
                for seed in [1, 2] {
                    let mut bytes = noise(len, seed);
                    if seed == 2 && len > 2 {
                        bytes[0] = 0;
                        bytes[1] = 0;
                    }
                    let text = alphabet.fold_bytes(&bytes);
                    assert!(text.chars().all(|c| alphabet.contains(c)), "{alphabet}: {text:?}");
                    assert_eq!(
                        alphabet.unfold_bytes(&text).as_deref(),
                        Ok(&bytes[..]),
                        "{alphabet} {len}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_alphabet_gives_back_bits_with_only_zeros_after() {
        for alphabet in Alphabet::ALL {
            for len in 0..40 {
                let mut writer = BitWriter::new();
                for i in 0..len {
                    writer.bit(i % 3 == 0);
                }
                let bits = writer.finish();
                let back = alphabet.unfold_bits(&alphabet.fold_bits(&bits)).unwrap();
                assert!(back.len() >= bits.len(), "{alphabet}");
                let (mut sent, mut got) = (bits.reader(), back.reader());
                for _ in 0..len {
                    assert_eq!(sent.bit(), got.bit(), "{alphabet}");
                }
                assert!(got.rest_is_zero(), "{alphabet}");
            }
        }
    }

    #[test]
    fn a_fold_costs_what_its_alphabet_says() {
        let bytes = noise(3000, 9);
        for alphabet in Alphabet::ALL {
            let text = alphabet.fold_bytes(&bytes);
            let chars = text.chars().count() as f64;
            let expected = 3000.0 * 8.0 / alphabet.bits_per_char();
            assert!(
                (chars - expected).abs() / expected < 0.02,
                "{alphabet}: {chars} vs {expected}"
            );
            let utf8 = text.len() as f64 / chars;
            assert!((utf8 - alphabet.bytes_per_char()).abs() < 0.05, "{alphabet}: {utf8}");
        }
    }

    #[test]
    fn a_foreign_character_is_named_where_it_is() {
        let error = Alphabet::Base64Url.unfold_bytes("AAA*").unwrap_err();
        assert_eq!((error.at, error.problem), (3, Problem::Foreign('*')));
        assert!(Alphabet::Cjk.unfold_bytes("a").is_err());
        assert!(Alphabet::TwoByte.unfold_bits("x").is_err());
    }

    #[test]
    fn crockford_reads_what_people_type() {
        let text = Alphabet::Crockford.fold_bytes(b"orimoji");
        let typed = text.to_lowercase().replace('0', "o").replace('1', "l");
        assert_eq!(Alphabet::Crockford.unfold_bytes(&typed).unwrap(), b"orimoji");
    }

    #[test]
    fn names_go_both_ways() {
        for alphabet in Alphabet::ALL {
            assert_eq!(Alphabet::from_name(alphabet.name()), Some(alphabet));
        }
        assert_eq!(Alphabet::from_name("base64"), Some(Alphabet::Base64Url));
        assert_eq!(Alphabet::from_name("base65"), None);
    }
}
