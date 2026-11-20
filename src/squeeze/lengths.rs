//! How a compact code table writes its code lengths: one for each symbol, from 1 to
//! [`huffman::MAX_LENGTH`]. Each way is a variant of [`LengthsCode`], numbered as a fold's choices
//! record it, and everything a way needs is here, behind [`Lengths`]: a better way joins as one
//! more variant, the rest of the table untouched, and a reader refuses a number it doesn't know
//! rather than guess at it.
//!
//! - [`LengthsCode::Fixed`]: five bits each. Nothing to describe first, so the cheapest for a
//!   short table, or for lengths spread evenly.
//! - [`LengthsCode::Huffman`]: the lengths in a Huffman code of their own, as deflate writes its
//!   code lengths. First the longest length, in five bits; then, for each length up to it, the
//!   length of its own code, in five bits, 0 for a length no symbol has; then each symbol's length
//!   in that code. The lengths of a large table bunch together, a few of them most of the symbols,
//!   and this writes them in a third to a half of the bits.
//!
//! A fold tries each, and keeps the one that writes its table's lengths in the fewest bits. Both
//! are naive in one way: they code each length on its own. A length could be told from what it
//! belongs to (a character or a word, how common its kind is), which a later way could use.

use crate::bits::{BitReader, BitWriter};
use crate::huffman::{self, Decoder, Encoder};

/// How many bits a length takes written plainly.
const LENGTH_BITS: u32 = 5;

/// A way of writing a table's code lengths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LengthsCode {
    /// Five bits each.
    Fixed,
    /// In a Huffman code of their own, described first.
    Huffman,
}

impl LengthsCode {
    /// Every way, in the order of their numbers.
    pub(crate) const ALL: [LengthsCode; 2] = [LengthsCode::Fixed, LengthsCode::Huffman];

    /// Its number in a fold's choices.
    pub(crate) fn number(self) -> u64 {
        match self {
            LengthsCode::Fixed => 0,
            LengthsCode::Huffman => 1,
        }
    }

    /// The way numbered `number`, if this orimoji knows it.
    pub(crate) fn numbered(number: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|way| way.number() == number)
    }

    /// Its name, as the decisions of a fold into parts give it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            LengthsCode::Fixed => "fixed",
            LengthsCode::Huffman => "huffman",
        }
    }

    /// The way that writes `lengths` in the fewest bits, what it describes first counted: on a
    /// tie, the earlier way.
    pub(crate) fn cheapest(lengths: &[u32]) -> Self {
        Self::ALL
            .into_iter()
            .min_by_key(|&way| Lengths::writing(way, lengths).size(lengths))
            .expect("there are ways")
    }
}

/// A way of writing code lengths, readied for one table: for [`LengthsCode::Huffman`], the code its
/// lengths are written in.
pub(crate) struct Lengths {
    /// The code each length is written in, its symbols the lengths less one, and its own
    /// lengths, as written first.
    code: Option<(Encoder, Decoder, Vec<u32>)>,
}

impl Lengths {
    /// `way`, readied to write `lengths`, every code length of a table.
    pub(crate) fn writing(way: LengthsCode, lengths: &[u32]) -> Self {
        let code = match way {
            LengthsCode::Fixed => None,
            LengthsCode::Huffman => {
                let longest = lengths.iter().copied().max().unwrap_or(1).max(1);
                let mut counts = vec![0u64; longest as usize];
                for &length in lengths {
                    counts[length as usize - 1] += 1;
                }
                let own = huffman::lengths(&counts);
                let coded = Encoder::new(own.clone()).zip(Decoder::new(&own));
                let (encoder, decoder) = coded.expect("Huffman lengths make a prefix code");
                Some((encoder, decoder, own))
            }
        };
        Self { code }
    }

    /// The bits `lengths` take this way, with what it describes first.
    pub(crate) fn size(&self, lengths: &[u32]) -> u64 {
        match &self.code {
            None => LENGTH_BITS as u64 * lengths.len() as u64,
            Some((encoder, _, own)) => {
                let described = LENGTH_BITS as u64 * (1 + own.len() as u64);
                described
                    + lengths
                        .iter()
                        .map(|&length| encoder.length(length as usize - 1) as u64)
                        .sum::<u64>()
            }
        }
    }

    /// Writes what a reader needs before the lengths: nothing for [`LengthsCode::Fixed`]; for
    /// [`LengthsCode::Huffman`], the longest length, then the length of each length's code.
    pub(crate) fn write_described(&self, writer: &mut BitWriter) {
        if let Some((_, _, own)) = &self.code {
            writer.write(own.len() as u64, LENGTH_BITS);
            for &length in own {
                writer.write(length as u64, LENGTH_BITS);
            }
        }
    }

    /// Writes one code length.
    pub(crate) fn write(&self, writer: &mut BitWriter, length: u32) {
        match &self.code {
            None => writer.write(length as u64, LENGTH_BITS),
            Some((encoder, _, _)) => encoder.write(writer, length as usize - 1),
        }
    }

    /// `way`, readied to read the lengths of a table, from what [`Lengths::write_described`]
    /// wrote. Refused, with what is wrong, where that describes no code.
    pub(crate) fn reading(way: LengthsCode, reader: &mut BitReader<'_>) -> Result<Self, String> {
        let short = || "it is cut short".to_string();
        let code = match way {
            LengthsCode::Fixed => None,
            LengthsCode::Huffman => {
                let longest = reader.read(LENGTH_BITS).ok_or_else(short)? as u32;
                if !(1..=huffman::MAX_LENGTH).contains(&longest) {
                    return Err(format!("a longest code length of {longest}, which no code has"));
                }
                let own: Vec<u32> = (0..longest)
                    .map(|_| reader.read(LENGTH_BITS).map(|length| length as u32).ok_or_else(short))
                    .collect::<Result<_, _>>()?;
                if own.iter().all(|&length| length == 0) {
                    return Err("a code for its lengths with no length in it".to_string());
                }
                let coded = Encoder::new(own.clone()).zip(Decoder::new(&own));
                let (encoder, decoder) = coded
                    .ok_or_else(|| "the code for its lengths is no prefix code".to_string())?;
                Some((encoder, decoder, own))
            }
        };
        Ok(Self { code })
    }

    /// Reads one code length, refused where it is no length a code has.
    pub(crate) fn read(&self, reader: &mut BitReader<'_>) -> Result<u32, String> {
        let length = match &self.code {
            None => reader.read(LENGTH_BITS).ok_or("it is cut short")? as u32,
            Some((_, decoder, _)) => {
                decoder.read(reader).ok_or("bits that are no length's code")? as u32 + 1
            }
        };
        match (1..=huffman::MAX_LENGTH).contains(&length) {
            true => Ok(length),
            false => Err(format!("a code {length} bits long, which no code is")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lengths written either way read back as they were, and the cheapest is fixed for a short
    /// table and Huffman for one whose lengths bunch together.
    #[test]
    fn lengths_written_either_way_read_back_and_the_cheapest_suits_them() {
        let short = vec![3, 2, 2, 4];
        let bunched: Vec<u32> = (0..80).map(|i| [6, 8, 8, 7, 8, 3, 8, 6][i % 8]).collect();
        let spread: Vec<u32> = (1..=30).collect();
        assert_eq!(LengthsCode::cheapest(&short), LengthsCode::Fixed);
        assert_eq!(LengthsCode::cheapest(&bunched), LengthsCode::Huffman);
        for lengths in [&short, &bunched, &spread] {
            for way in LengthsCode::ALL {
                let writing = Lengths::writing(way, lengths);
                let mut writer = BitWriter::new();
                writing.write_described(&mut writer);
                for &length in lengths.iter() {
                    writing.write(&mut writer, length);
                }
                assert_eq!(
                    writer.len() as u64,
                    writing.size(lengths),
                    "{way:?}: size() is what is written"
                );
                let bits = writer.finish();
                let mut reader = bits.reader();
                let reading = Lengths::reading(way, &mut reader).unwrap();
                let back: Vec<u32> =
                    lengths.iter().map(|_| reading.read(&mut reader).unwrap()).collect();
                assert_eq!(&back, lengths, "{way:?}");
                assert_eq!(LengthsCode::numbered(way.number()), Some(way));
            }
        }
        assert_eq!(LengthsCode::numbered(7), None, "a way this orimoji doesn't know");
    }

    /// What describes no code of lengths is refused: a longest length of 0 or past the longest,
    /// a code with no lengths in it, or one that is no prefix code.
    #[test]
    fn a_code_for_lengths_no_table_writes_is_refused() {
        let described = |write: &dyn Fn(&mut BitWriter)| {
            let mut writer = BitWriter::new();
            write(&mut writer);
            writer.finish()
        };
        for (bits, says) in [
            (described(&|w| w.write(0, 5)), "no code has"),
            (described(&|w| w.write(31, 5)), "no code has"),
            (
                described(&|w| {
                    w.write(2, 5);
                    w.write(0, 10)
                }),
                "no length in it",
            ),
            (
                described(&|w| {
                    w.write(3, 5);
                    w.write(1, 5);
                    w.write(1, 5);
                    w.write(1, 5)
                }),
                "no prefix code",
            ),
            (
                described(&|w| {
                    w.write(4, 5);
                    w.write(1, 5)
                }),
                "cut short",
            ),
        ] {
            let refused = Lengths::reading(LengthsCode::Huffman, &mut bits.reader()).err();
            assert!(
                refused.as_deref().is_some_and(|why| why.contains(says)),
                "{says}: {refused:?}"
            );
        }
    }
}
