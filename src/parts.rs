//! A fold as parts, for a program that keeps them in a file format of its own: the data, and no
//! file. [`fold`] gives back a line for each line of the input, the code they are spelled in, the
//! choices the fold made, and an index when one paid; the caller keeps them where it suits, and
//! gives them back to [`unfold`].
//!
//! ```
//! use orimoji::{Alphabet, parts};
//!
//! let drawing = "⣿⣿⣿⣿⣿⣿⡇\n⣿⣿⡇ ⢸⣿⡇\n⣿⣿⣿⣿⣿⣿⡇";
//! let folded = parts::fold(drawing, Alphabet::Base64Url, false);
//! assert_eq!(folded.lines.len(), 3, "a line for each line");
//! // Kept wherever suits the caller, and given back:
//! let back = parts::unfold(&folded.lines, &folded.table, &folded.choices, None, Alphabet::Base64Url, 1 << 20)?;
//! assert_eq!(back, drawing);
//! # Ok::<(), orimoji::UnfoldError>(())
//! ```
//!
//! It is the text squeeze, a line still a line, with three things a folded file doesn't have:
//!
//! - **A compact table.** Code lengths as five bits each, and characters as steps from the one
//!   before, where a folded file writes a line per symbol: a braille drawing's characters, all
//!   close together, cost a byte or so each.
//! - **Runs weighed.** A run of a unit whose code is short costs more than the unit written again;
//!   each run is written whichever way is shorter.
//! - **Counts in a code chosen for them.** A run's count is in Elias gamma, or with its last `k`
//!   bits written as they are, or with its length in a Huffman code of its own: whichever writes
//!   this text's counts shortest. Which, is one of the choices.
//!
//! The choices are a few bits: the parts' format, how counts are written, whether the lines need
//! an index and how much of it, and a fingerprint of all of it and of the table. So parts that
//! don't belong together, a table or choices from another fold, or an index other than the one
//! the lines were folded with, are refused rather than read wrong; and so is a damaged line, by
//! its number. Unfolding takes a limit, and refuses a fold that would come to more, before
//! making it.
//!
//! Neither the squeeze nor the alphabet is in the parts: the caller knows what it asked for, and
//! gives the same alphabet back to unfold.

use crate::alphabet::Alphabet;
use crate::bits::{BitReader, BitWriter, Bits};
use crate::error::{FoldError, UnfoldError};
use crate::index::{Index, Indexing};
use crate::squeeze::text::{Misread, TextCode};

/// The parts' format, the first thing the choices say: an orimoji reads its own and every earlier
/// one, and refuses a newer one by its number.
pub const FORMAT: u64 = 1;

/// How many bits the choices give the format.
const FORMAT_BITS: u32 = 4;

/// A fold as parts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    /// A line for each line of the input, in the alphabet: an empty line for an empty one, and
    /// nothing else empty.
    pub lines: Vec<String>,
    /// The code the lines are spelled in, as one line of the alphabet.
    pub table: String,
    /// The choices the fold made, and the fingerprint that ties the parts together, as one line
    /// of the alphabet.
    pub choices: String,
    /// What became of the index, when the fold was asked to index: [`Indexing::Off`] when it
    /// wasn't, [`Indexing::Unneeded`] when none paid, or the index to keep and give back.
    pub indexing: Indexing,
}

impl Parts {
    /// The bytes the parts take kept as text: each line, the table and the choices with a newline,
    /// and the index's text.
    pub fn size(&self) -> usize {
        let lines: usize = self.lines.iter().map(|line| line.len() + 1).sum();
        let index = self.indexing.index().map_or(0, |index| index.to_text().len());
        lines + self.table.len() + 1 + self.choices.len() + 1 + index
    }
}

/// `input` folded into parts, in `alphabet`. Allowed to index, it folds both with an index and
/// without, and keeps whichever parts are smaller, the index's text counted.
pub fn fold(input: &str, alphabet: Alphabet, index: bool) -> Parts {
    let plain = fold_with(input, alphabet, false);
    if !index {
        return plain;
    }
    let indexed = fold_with(input, alphabet, true);
    match &indexed.indexing {
        Indexing::Index(_) if indexed.size() < plain.size() => indexed,
        _ => Parts { indexing: Indexing::Unneeded, ..plain },
    }
}

/// `input` folded with the code of parts folded before, its `table` and `choices`, given the
/// index those were folded with, if any: every line the two have in common folds to the same
/// line, and the table and the choices stay as they were. What the code lacks is spelled out
/// instead, a little larger. Fold afresh now and then, to bring the code up to date.
pub fn fold_keeping(
    input: &str,
    table: &str,
    choices: &str,
    index: Option<&Index>,
    alphabet: Alphabet,
) -> Result<Parts, FoldError> {
    let (code, index) = code_of(table, choices, index, alphabet).map_err(FoldError::Previous)?;
    let indexing = index.map_or(Indexing::Off, |index| Indexing::Index(index.clone()));
    Ok(Parts {
        lines: spelled(&code, input, alphabet),
        table: table.to_string(),
        choices: choices.to_string(),
        indexing,
    })
}

/// What parts [`fold`] made hold: the lines, back in their text, joined by newlines. Refused if
/// the table, the choices and the index aren't the ones the lines were folded with, if a line
/// is damaged (by its number, counting from 1), or if the text would come to more than `limit`
/// bytes, before those bytes are made.
pub fn unfold<S: AsRef<str>>(
    lines: &[S],
    table: &str,
    choices: &str,
    index: Option<&Index>,
    alphabet: Alphabet,
    limit: usize,
) -> Result<String, UnfoldError> {
    let (code, _) = code_of(table, choices, index, alphabet)?;
    let mut out = String::new();
    for (at, line) in lines.iter().enumerate() {
        let number = at + 1;
        if at > 0 {
            if out.len() >= limit {
                return Err(UnfoldError::TooLarge { line: number, limit });
            }
            out.push('\n');
        }
        let line = line.as_ref();
        if line.is_empty() {
            continue;
        }
        let bits = alphabet
            .unfold_bits(line)
            .map_err(|error| UnfoldError::Alphabet { line: number, error })?;
        let mut reader = bits.reader();
        let corrupt =
            |problem: &str| UnfoldError::Corrupt { line: number, problem: problem.into() };
        match code.read_within(&mut reader, limit - out.len()) {
            Err(Misread::TooLarge) => return Err(UnfoldError::TooLarge { line: number, limit }),
            Err(Misread::Corrupt) => return Err(corrupt("these bits spell no line")),
            Ok(_) if !exactly(line, &bits, &reader, alphabet) => {
                return Err(corrupt("more after the line's end than the zeros that fill it out"));
            }
            Ok(text) if text.is_empty() => {
                return Err(corrupt("a line that spells nothing, where an empty line is empty"));
            }
            Ok(text) => out.push_str(&text),
        }
    }
    Ok(out)
}

/// Whether parts with these `choices` need an index to unfold, and so whether one has to be kept
/// with them.
pub fn needs_index(choices: &str, alphabet: Alphabet) -> Result<bool, UnfoldError> {
    Ok(Choices::read(choices, alphabet)?.index.is_some())
}

/// `input` folded into parts, indexing if `index`: the code learnt, the lines spelled in it, the
/// table and the choices written.
fn fold_with(input: &str, alphabet: Alphabet, index: bool) -> Parts {
    let strings: Vec<&str> = input.split('\n').filter(|line| !line.is_empty()).collect();
    let code = TextCode::learn_for_parts(&strings, index);
    let values = code.symbols();
    let indexing = match (index, values.is_empty()) {
        (false, _) => Indexing::Off,
        (true, true) => Indexing::Unneeded,
        (true, false) => {
            Indexing::Index(Index::new(values).expect("a code's symbols are distinct"))
        }
    };
    let mut writer = BitWriter::new();
    code.write_compact(&mut writer, indexing.index());
    let table = alphabet.fold_bits(&writer.finish());
    let (k, coded) = code.count_choices();
    let index_mark = indexing.index().map(|index| (index.len(), index.fingerprint_of(index.len())));
    let index = index_mark.map(|(count, _)| count as u64);
    let choices = Choices { k, coded, index, fingerprint: 0 };
    let fingerprint = choices.fingerprint(index_mark.map(|(_, print)| print), &table);
    let choices = Choices { fingerprint, ..choices }.write(alphabet);
    Parts { lines: spelled(&code, input, alphabet), table, choices, indexing }
}

/// Each line of `input` spelled in `code`, in `alphabet`; an empty line as an empty one.
fn spelled(code: &TextCode, input: &str, alphabet: Alphabet) -> Vec<String> {
    input
        .split('\n')
        .map(|line| match line.is_empty() {
            true => String::new(),
            false => {
                let mut writer = BitWriter::new();
                code.write(&mut writer, line);
                alphabet.fold_bits(&writer.finish())
            }
        })
        .collect()
}

/// The code `table` holds, once `choices` say it is the one they were made with, and with
/// `index`, which must then be the one it was folded with if it needs one: the code, and the
/// index it needs.
fn code_of<'a>(
    table: &str,
    choices: &str,
    index: Option<&'a Index>,
    alphabet: Alphabet,
) -> Result<(TextCode, Option<&'a Index>), UnfoldError> {
    let choices = Choices::read(choices, alphabet)?;
    let (index, print) = match (choices.index, index) {
        (None, _) => (None, None),
        (Some(_), None) => return Err(UnfoldError::NeedsIndex),
        (Some(count), Some(index)) if count <= index.len() as u64 => {
            (Some(index), Some(index.fingerprint_of(count as usize)))
        }
        (Some(_), Some(_)) => return Err(UnfoldError::WrongIndex),
    };
    if choices.fingerprint(print, table) != choices.fingerprint {
        return Err(UnfoldError::PartsDontFit);
    }
    let bits =
        alphabet.unfold_bits(table).map_err(|error| UnfoldError::CodeTable(error.to_string()))?;
    let mut reader = bits.reader();
    let code = TextCode::read_compact(&mut reader, index, choices.k, choices.coded)
        .map_err(UnfoldError::CodeTable)?;
    match exactly(table, &bits, &reader, alphabet) {
        true => Ok((code, index)),
        false => Err(UnfoldError::CodeTable(
            "more after the code than the zeros that fill it out".to_string(),
        )),
    }
}

/// Whether `text` is exactly the bits `reader` has read of `bits` folded: what a writer wrote,
/// with nothing after it but the zeros that fill out its last character. So a part with anything
/// more, even a character of zeros, is refused as one no writer wrote.
fn exactly(text: &str, bits: &Bits, reader: &BitReader<'_>, alphabet: Alphabet) -> bool {
    alphabet.fold_bits(&bits.prefix(reader.position())) == text
}

/// What the choices say: how counts are written, how much of an index the lines need, and the
/// fingerprint of it all and of the table.
#[derive(Clone, Copy, Debug)]
struct Choices {
    /// The count's low bits written as they are.
    k: u32,
    /// Whether the count's buckets have a code of their own.
    coded: bool,
    /// How many of the index's values the lines were folded with, if they need one.
    index: Option<u64>,
    fingerprint: u32,
}

impl Choices {
    /// The bits: the format in four, `k` in three, whether counts are coded and whether there is
    /// an index in one each, how much of the index in Elias gamma, and the fingerprint in 32.
    fn write(&self, alphabet: Alphabet) -> String {
        let mut writer = BitWriter::new();
        writer.write(FORMAT, FORMAT_BITS);
        writer.write(self.k as u64, 3);
        writer.bit(self.coded);
        writer.bit(self.index.is_some());
        if let Some(count) = self.index {
            writer.gamma(count);
        }
        writer.write(self.fingerprint as u64, 32);
        alphabet.fold_bits(&writer.finish())
    }

    fn read(text: &str, alphabet: Alphabet) -> Result<Self, UnfoldError> {
        let bad = |why: &str| UnfoldError::Choices(why.to_string());
        let bits = alphabet.unfold_bits(text).map_err(|error| bad(&error.to_string()))?;
        let mut reader = bits.reader();
        let short = || bad("they are cut short");
        match reader.read(FORMAT_BITS).ok_or_else(short)? {
            FORMAT => {}
            0 => return Err(bad("no format is numbered 0")),
            newer => {
                let why = format!("parts format {newer}, newer than this orimoji reads ({FORMAT})");
                return Err(bad(&why));
            }
        }
        let k = reader.read(3).ok_or_else(short)? as u32;
        let coded = reader.bit().ok_or_else(short)?;
        let index = match reader.bit().ok_or_else(short)? {
            true => Some(reader.gamma().ok_or_else(short)?),
            false => None,
        };
        let fingerprint = reader.read(32).ok_or_else(short)? as u32;
        match exactly(text, &bits, &reader, alphabet) {
            true => Ok(Self { k, coded, index, fingerprint }),
            false => Err(bad("more after them than the zeros that fill them out")),
        }
    }

    /// FNV-1a over what the choices say, the fingerprint of the index's values they count (if
    /// any), and the table, as written.
    fn fingerprint(&self, index: Option<u32>, table: &str) -> u32 {
        let said = format!(
            "{FORMAT} {} {} {}.{:08x}\n",
            self.k,
            self.coded as u8,
            self.index.unwrap_or(0),
            index.unwrap_or(0)
        );
        let mut hash = 0x811c_9dc5u32;
        for &byte in said.as_bytes().iter().chain(table.as_bytes()) {
            hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
        }
        hash
    }
}
