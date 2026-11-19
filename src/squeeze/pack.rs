//! The pack squeeze: each character as its place among the characters the file uses, in just the
//! bits that many choices need. Ten digits take 3.32 bits a character instead of a byte.
//!
//! A line is one number in the base of the character count, read from its last character to its
//! first, with a 1 above it so its length needs no saying: `1·Nⁿ + cₙ₋₁·Nⁿ⁻¹ + … + c₀`. That is
//! within a bit of the least any spelling can take. It is written least significant bit first, so
//! the zeros an alphabet adds to fill its last character are zeros above the number, changing
//! nothing. A long line is cut into segments of [`SEGMENT`] characters (written at a fixed width,
//! their count first) so the arithmetic stays linear.
//!
//! The characters themselves are the table, in code point order. Unlike a Huffman code, it changes
//! only when a character the file never had arrives, so a line folds the same until then.

use super::{Body, Line, Squeezed};
use crate::bignum::Nat;
use crate::bits::{BitReader, BitWriter, Bits};
use crate::container::{self, Folded};
use crate::error::UnfoldError;
use crate::escape::{escape, unescape};
use std::collections::{BTreeSet, HashMap};

/// Characters in a full segment of a long line.
const SEGMENT: usize = 256;

pub(crate) fn squeeze(text: &str) -> Squeezed {
    let chars: Vec<char> =
        text.chars().filter(|&c| c != '\n').collect::<BTreeSet<char>>().into_iter().collect();
    let place: HashMap<char, u64> =
        chars.iter().enumerate().map(|(at, &c)| (c, at as u64)).collect();
    let base = chars.len().max(2) as u64;
    let width = segment_width(base);
    let body = text
        .split('\n')
        .map(|line| match line.is_empty() {
            true => Line::Empty,
            false => {
                let digits: Vec<u64> = line.chars().map(|c| place[&c]).collect();
                Line::Bits(pack(&digits, base, width))
            }
        })
        .collect();
    let mut tables = vec!["=== characters ===".to_string()];
    tables.extend(chars.iter().map(|c| escape(&c.to_string())));
    Squeezed { options: Vec::new(), tables: super::Tables::Lines(tables), body: Body::Lines(body) }
}

/// The bits a full segment takes: enough for `base^SEGMENT - 1`.
fn segment_width(base: u64) -> usize {
    let mut largest = Nat::power(base, SEGMENT);
    largest.decrement();
    largest.bit_len()
}

fn pack(digits: &[u64], base: u64, width: usize) -> Bits {
    let full = digits.len() / SEGMENT;
    let mut writer = BitWriter::new();
    writer.gamma(full as u64 + 1);
    for segment in digits.chunks(SEGMENT).take(full) {
        let mut number = Nat::zero();
        for &digit in segment.iter().rev() {
            number.mul_add(base, digit);
        }
        write_lsb_first(&mut writer, &number, width);
    }
    let mut number = Nat::one();
    for &digit in digits[full * SEGMENT..].iter().rev() {
        number.mul_add(base, digit);
    }
    write_lsb_first(&mut writer, &number, number.bit_len());
    writer.finish()
}

fn write_lsb_first(writer: &mut BitWriter, number: &Nat, width: usize) {
    for index in 0..width {
        writer.bit(number.bit(index));
    }
}

fn read_lsb_first(reader: &mut BitReader<'_>, width: usize) -> Option<Nat> {
    let mut number = Nat::zero();
    for index in 0..width {
        if reader.bit()? {
            number.set_bit(index);
        }
    }
    Some(number)
}

fn unpack(bits: &Bits, base: u64, width: usize) -> Option<Vec<u64>> {
    let mut reader = bits.reader();
    let full = reader.gamma()? - 1;
    let mut digits = Vec::new();
    for _ in 0..full {
        let mut number = read_lsb_first(&mut reader, width)?;
        for _ in 0..SEGMENT {
            digits.push(number.div_rem(base));
        }
        if !number.is_zero() {
            return None;
        }
    }
    let rest = reader.remaining();
    let mut number = read_lsb_first(&mut reader, rest)?;
    // Digits come off until the 1 that marks the line's length is all that is left. Anything else
    // left below the base was never written by a fold: dividing it would give zeros forever.
    while !number.is_one() {
        if number.below(base) {
            return None;
        }
        digits.push(number.div_rem(base));
    }
    Some(digits)
}

pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    let sections = container::sections(&folded.tables);
    let lines = sections.iter().find(|(name, _)| *name == "characters").map(|(_, lines)| lines);
    let lines =
        lines.ok_or(UnfoldError::Table { line: 1, problem: "no characters section".into() })?;
    let mut chars = Vec::with_capacity(lines.len());
    for line in lines {
        let problem = |problem: String| UnfoldError::Table { line: line.number, problem };
        let text = unescape(line.text).map_err(problem)?;
        let mut letters = text.chars();
        match (letters.next(), letters.next()) {
            (Some(c), None) => chars.push(c),
            _ => return Err(problem(format!("`{}` is not one character", line.text))),
        }
    }
    let base = chars.len().max(2) as u64;
    let width = segment_width(base);
    let mut out = String::new();
    for (at, line) in folded.body.iter().enumerate() {
        if at > 0 {
            out.push('\n');
        }
        if line.text.is_empty() {
            continue;
        }
        let bits = super::bits_of(folded, line)?;
        let digits = unpack(&bits, base, width)
            .ok_or_else(|| super::corrupt(line, "these bits pack no line"))?;
        for digit in digits {
            out.push(
                *chars
                    .get(digit as usize)
                    .ok_or_else(|| super::corrupt(line, "a character past the table"))?,
            );
        }
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_pack_close_to_their_three_and_a_third_bits() {
        let digits: Vec<u64> = (0..1000u64).map(|i| (i * 7 + i / 3) % 10).collect();
        let width = segment_width(10);
        let bits = pack(&digits, 10, width);
        assert!(bits.len() <= 3322 + 1 + 3 * 8 + 4, "{} bits for 1000 digits", bits.len());
        assert_eq!(unpack(&bits, 10, width).unwrap(), digits);
    }

    #[test]
    fn every_length_around_a_segment_reads_back() {
        let width = segment_width(3);
        for len in [1, 2, SEGMENT - 1, SEGMENT, SEGMENT + 1, 2 * SEGMENT, 3 * SEGMENT + 7] {
            let digits: Vec<u64> = (0..len as u64).map(|i| i % 3).collect();
            let mut bits = pack(&digits, 3, width);
            assert_eq!(unpack(&bits, 3, width).unwrap(), digits, "{len}");
            // An alphabet's zeros on the end change nothing.
            let mut writer = BitWriter::new();
            let mut reader = bits.reader();
            while let Some(bit) = reader.bit() {
                writer.bit(bit);
            }
            writer.write(0, 13);
            bits = writer.finish();
            assert_eq!(unpack(&bits, 3, width).unwrap(), digits, "{len}, padded");
        }
    }

    #[test]
    fn a_number_that_doesnt_end_in_its_marking_one_is_refused() {
        for damaged in [0u64, 5, 9] {
            let mut writer = BitWriter::new();
            writer.gamma(1);
            writer.write(damaged.reverse_bits() >> 60, 4);
            assert_eq!(unpack(&writer.finish(), 10, segment_width(10)), None, "{damaged}");
        }
    }

    #[test]
    fn a_line_of_one_repeated_character_still_counts_its_length() {
        let squeezed = squeeze("aaaa\naa");
        let Body::Lines(lines) = squeezed.body else { unreachable!() };
        let Line::Bits(bits) = &lines[0] else { unreachable!() };
        assert_eq!(unpack(bits, 2, segment_width(2)).unwrap(), [0, 0, 0, 0]);
    }
}
