//! A fold as parts, for a program that keeps them in a file format of its own: the data, and no
//! file. [`fold`] gives back a line for each line of the input, and the [`Unit`] the lines unfold
//! with: the choices the fold made, the code the lines are spelled in, and the index, when one
//! paid, always together. The caller keeps the lines and the unit where it suits, the unit as one
//! block, and gives both back to [`unfold`]. [`fold_together`] folds several texts with one unit,
//! learnt over all of them; [`fold_grouped`] folds together only those that take less so, and each
//! of the rest alone; and [`fold_keeping`] folds a text with a unit made before.
//!
//! ```
//! use orimoji::{Alphabet, parts};
//!
//! let drawing = "⣿⣿⣿⣿⣿⣿⡇\n⣿⣿⡇ ⢸⣿⡇\n⣿⣿⣿⣿⣿⣿⡇";
//! let folded = parts::fold(drawing, Alphabet::Base64Url, false);
//! assert_eq!(folded.lines.len(), 3, "a line for each line");
//! // The unit, kept as text wherever suits the caller, and read back:
//! let unit = parts::Unit::from_text(&folded.unit.to_text())?;
//! let back = parts::unfold(&folded.lines, &unit, Alphabet::Base64Url, 1 << 20)?;
//! assert_eq!(back, drawing);
//! # Ok::<(), orimoji::UnfoldError>(())
//! ```
//!
//! It is the text squeeze, a line still a line, with what a folded file doesn't have:
//!
//! - **A compact table.** Characters as steps from the one before, and code lengths five bits
//!   each or in a code of their own: a braille drawing's characters, all close together, cost a
//!   byte or so each.
//! - **Runs weighed.** A run of a unit whose code is short costs more than the unit written again;
//!   each run is written whichever way is shorter.
//! - **Counts in a code chosen for them.** A run's count is in Elias gamma, or with its last `k`
//!   bits written as they are, or with its length in a Huffman code of its own: whichever writes
//!   this text's counts shortest.
//!
//! Wherever the fold could write something more than one way, it weighs them and takes the
//! shortest, and the choices record each decision ([`decisions`] reads them). The choices also
//! carry a fingerprint of the table and the index, so a unit whose parts don't belong together, a
//! table or choices from another fold, or an index other than the one the lines were folded with,
//! is refused rather than read wrong; and so is a damaged line, by its number. Unfolding takes a
//! limit, and refuses a fold that would come to more, before making it.
//!
//! Neither the squeeze nor the alphabet is in the unit: the caller knows what it asked for, and
//! gives the same alphabet back to unfold.

use crate::alphabet::Alphabet;
use crate::bits::{BitReader, BitWriter, Bits};
use crate::error::{FoldError, UnfoldError};
use crate::escape::{escape, unescape};
use crate::index::Index;
use crate::sharing;
use crate::squeeze::lengths::LengthsCode;
use crate::squeeze::text::{Misread, TableChoices, TextCode};
use std::collections::HashSet;

/// The parts' format, the first thing the choices say: an orimoji reads its own and every earlier
/// one, and refuses a newer one by its number. Format 2 records how the table writes its code
/// lengths and its characters' steps; format 1 wrote every table one way, the first of each.
pub const FORMAT: u64 = 2;

/// How many bits the choices give the format.
const FORMAT_BITS: u32 = 4;

/// What a fold into parts needs to unfold its lines, kept as one: the choices the fold made, the
/// code the lines are spelled in, and the index, when one paid. Any of the three lost, the lines
/// can't be read, so the three are never kept apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// The choices the fold made, and the fingerprint that ties the unit together, as one line of
    /// the alphabet.
    pub choices: String,
    /// The code the lines are spelled in, as one line of the alphabet.
    pub table: String,
    /// The index, when one paid.
    pub index: Option<Index>,
}

impl Unit {
    /// The unit as text, to keep: its choices, its table, then each value of its index, escaped,
    /// a line each.
    pub fn to_text(&self) -> String {
        let mut text = format!("{}\n{}\n", self.choices, self.table);
        for value in self.index.iter().flat_map(Index::values) {
            text.push_str(&escape(value));
            text.push('\n');
        }
        text
    }

    /// The unit [`Unit::to_text`] wrote, or refused where it is cut short or a value doesn't read.
    /// Whether its parts belong together, [`unfold`] says.
    pub fn from_text(text: &str) -> Result<Self, UnfoldError> {
        let text = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line));
        let bad = |why: &str| UnfoldError::Unit(why.to_string());
        let (choices, table) = match (lines.next(), lines.next()) {
            (Some(choices), Some(table)) if !choices.is_empty() && !table.is_empty() => {
                (choices, table)
            }
            _ => return Err(bad("it is cut short: a unit has its choices, then its table")),
        };
        let mut values = Vec::new();
        for line in lines {
            if line.is_empty() {
                return Err(bad("an empty line among its index's values, where none is empty"));
            }
            values.push(unescape(line).map_err(|why| bad(&format!("its index: {why}")))?);
        }
        let index = match values.is_empty() {
            true => None,
            false => Some(Index::new(values)?),
        };
        Ok(Self { choices: choices.to_string(), table: table.to_string(), index })
    }

    /// The bytes its text takes.
    pub fn size(&self) -> usize {
        self.to_text().len()
    }

    /// A fingerprint of the whole unit, FNV-1a over its text: what lines kept apart from their
    /// unit name it by, so that they are never unfolded with another.
    pub fn fingerprint(&self) -> u32 {
        fnv(self.to_text().as_bytes())
    }

    /// What the fold that made it decided.
    pub fn decisions(&self, alphabet: Alphabet) -> Result<Decisions, UnfoldError> {
        decisions(&self.choices, alphabet)
    }
}

/// FNV-1a, 32 bits.
fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |hash, &byte| (hash ^ byte as u32).wrapping_mul(0x0100_0193))
}

/// A fold as parts: a line for each line of the input, in the alphabet (an empty line for an
/// empty one, and nothing else empty), and the unit they unfold with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    pub lines: Vec<String>,
    pub unit: Unit,
}

impl Parts {
    /// The bytes the parts take kept as text: each line with a newline, and the unit's text.
    pub fn size(&self) -> usize {
        lines_size(&self.lines) + self.unit.size()
    }
}

/// Texts folded together: each one's lines, in their order, and the one unit they all unfold
/// with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Together {
    pub lines: Vec<Vec<String>>,
    pub unit: Unit,
}

impl Together {
    /// The bytes they take kept as text: every line with a newline, and the unit's text once.
    pub fn size(&self) -> usize {
        self.lines.iter().map(|lines| lines_size(lines)).sum::<usize>() + self.unit.size()
    }
}

fn lines_size(lines: &[String]) -> usize {
    lines.iter().map(|line| line.len() + 1).sum()
}

/// `input` folded into parts, in `alphabet`. Allowed to index, it folds both with an index and
/// without, and keeps whichever parts are smaller, the index counted.
pub fn fold(input: &str, alphabet: Alphabet, index: bool) -> Parts {
    let Together { mut lines, unit } = fold_together(&[input], alphabet, index);
    Parts { lines: lines.remove(0), unit }
}

/// `inputs` folded together, with one unit: one code, and one index when one pays, learnt over
/// all of them, so that every text spells a word as every other does, and the unit is kept once.
/// Allowed to index, it folds both with an index and without, and keeps whichever is smaller in
/// all, the index counted.
pub fn fold_together<S: AsRef<str>>(inputs: &[S], alphabet: Alphabet, index: bool) -> Together {
    let inputs: Vec<&str> = inputs.iter().map(AsRef::as_ref).collect();
    let plain = together_with(&inputs, alphabet, false);
    if !index {
        return plain;
    }
    let indexed = together_with(&inputs, alphabet, true);
    match &indexed.unit.index {
        Some(_) if indexed.size() < plain.size() => indexed,
        _ => plain,
    }
}

/// What keeping texts costs besides their lines and units, for [`fold_grouped`] to weigh: a text
/// folded alone, a text that shares a unit, and the shared unit, once. Nothing, by default; a
/// file costs its header.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overheads {
    pub alone: usize,
    pub sharing: usize,
    pub shared_unit: usize,
}

/// Texts folded where each takes least: the unit the texts that fold together share, if any do,
/// and each text, in their order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grouped {
    /// The unit the texts folded together share: `None` when each folds alone.
    pub shared: Option<Unit>,
    pub texts: Vec<Member>,
}

/// A text of a [`Grouped`] fold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Member {
    /// Its lines, which unfold with the shared unit.
    Sharing(Vec<String>),
    /// Folded alone, as [`fold`] folds it: its lines and a unit of its own.
    Alone(Parts),
}

impl Grouped {
    /// The bytes they take kept as text, each line with a newline and each unit's text once, and
    /// what `overheads` says keeping them costs besides.
    pub fn size(&self, overheads: Overheads) -> usize {
        let shared = self.shared.as_ref().map_or(0, |unit| unit.size() + overheads.shared_unit);
        let texts: usize = self
            .texts
            .iter()
            .map(|member| match member {
                Member::Sharing(lines) => lines_size(lines) + overheads.sharing,
                Member::Alone(parts) => parts.size() + overheads.alone,
            })
            .sum();
        shared + texts
    }
}

/// How many sets a grouped fold folds together at most, each a smaller one than the last.
const MOST_SETS: usize = 4;

/// `inputs` folded where each takes least: those that take less sharing one unit fold together,
/// learnt over them, and each of the rest alone, with a unit of its own, as [`fold`] folds it.
/// Texts unlike the rest pay for sharing a code: their own lines are spelled in a code that suits
/// them worse, and their symbols crowd everyone else's; they save the part of their unit the rest
/// would keep anyway.
///
/// Every text is folded alone, and all of them together. What each would save the whole by
/// leaving the set is estimated (the set's counts less its own are the rest's, so it is weighed
/// against the others, not against a set it is part of); those that would save leave it, and the
/// set left is folded together again, round by round, while that takes less. The fold kept is
/// whichever takes least of all of them alone and every set tried, by their sizes as folded, and
/// as `overheads` says keeping each costs: no estimate decides it.
pub fn fold_grouped<S: AsRef<str>>(
    inputs: &[S],
    alphabet: Alphabet,
    index: bool,
    overheads: Overheads,
) -> Grouped {
    let texts: Vec<&str> = inputs.iter().map(AsRef::as_ref).collect();
    let alone: Vec<Parts> = texts.iter().map(|text| fold(text, alphabet, index)).collect();
    let sizes: Vec<usize> = alone.iter().map(|parts| parts.size() + overheads.alone).collect();
    let mut members: Vec<Member> = alone.into_iter().map(Member::Alone).collect();
    let shared = group(&texts, alphabet, index, overheads, &sizes).map(|(sharing, together)| {
        for (at, lines) in sharing.into_iter().zip(together.lines) {
            members[at] = Member::Sharing(lines);
        }
        together.unit
    });
    Grouped { shared, texts: members }
}

/// Which of `texts` fold together into one unit where that takes least, as [`fold_grouped`]
/// chooses them, and their fold together; `None` where each takes least alone, in `alone[i]`
/// bytes, everything keeping it costs included, however the caller would keep it alone.
pub(crate) fn group(
    texts: &[&str],
    alphabet: Alphabet,
    index: bool,
    overheads: Overheads,
    alone: &[usize],
) -> Option<(Vec<usize>, Together)> {
    // The fold that takes least so far: which texts share, and their fold together; every text
    // alone, to begin with.
    let mut least = alone.iter().sum::<usize>();
    let mut best: Option<(Vec<usize>, Together)> = None;
    let mut sharing: Vec<usize> = (0..texts.len()).collect();
    // The set to try if leaving all at once takes more: all but the one that saves most.
    let mut fallback: Option<Vec<usize>> = None;
    for round in 0..MOST_SETS {
        if sharing.len() < 2 {
            match fallback.take() {
                Some(next) => {
                    sharing = next;
                    continue;
                }
                None => break,
            }
        }
        let members: Vec<&str> = sharing.iter().map(|&at| texts[at]).collect();
        let together = fold_together(&members, alphabet, index);
        let apart: usize =
            (0..texts.len()).filter(|at| !sharing.contains(at)).map(|at| alone[at]).sum();
        let size =
            together.size() + overheads.shared_unit + sharing.len() * overheads.sharing + apart;
        let smaller = size < least;
        if !smaller && round > 0 {
            match fallback.take() {
                Some(next) => {
                    sharing = next;
                    continue;
                }
                None => break,
            }
        }
        let savings = {
            let known: HashSet<&str> =
                together.unit.index.iter().flat_map(Index::values).map(String::as_str).collect();
            let set = sharing::Set {
                texts: members,
                known,
                table_bits: together.unit.table.chars().count() as f64 * alphabet.bits_per_char(),
                lines: together.lines.iter().map(|lines| lines_size(lines)).collect(),
            };
            let alone: Vec<usize> = sharing.iter().map(|&at| alone[at]).collect();
            sharing::savings(&set, &alone, overheads.sharing, alphabet)
        };
        if smaller {
            least = size;
            best = Some((sharing.clone(), together));
        }
        let mut leaving: Vec<(usize, f64)> = sharing
            .iter()
            .zip(savings)
            .filter(|&(_, saving)| saving > 0.0)
            .map(|(&at, saving)| (at, saving))
            .collect();
        if leaving.is_empty() {
            break;
        }
        leaving.sort_by(|a, b| b.1.total_cmp(&a.1));
        fallback = (leaving.len() > 1)
            .then(|| sharing.iter().copied().filter(|&at| at != leaving[0].0).collect());
        sharing.retain(|at| !leaving.iter().any(|(leaver, _)| leaver == at));
    }
    best
}

/// `input` folded with a `unit` made before: every line it shares with what that unit was made
/// for folds to the same line, and the unit stays as it was. What its code lacks is spelled out
/// instead, a little larger. Fold afresh now and then, to bring the unit up to date.
pub fn fold_keeping(
    input: &str,
    unit: &Unit,
    alphabet: Alphabet,
) -> Result<Vec<String>, FoldError> {
    let code = code_of(unit, alphabet).map_err(FoldError::Previous)?;
    Ok(spelled(&code, input, alphabet))
}

/// What `lines` folded with `unit` hold, back in their text, joined by newlines. Refused if the
/// unit's choices, table and index don't belong together, if a line is damaged (by its number,
/// counting from 1), or if the text would come to more than `limit` bytes, before those bytes
/// are made.
pub fn unfold<S: AsRef<str>>(
    lines: &[S],
    unit: &Unit,
    alphabet: Alphabet,
    limit: usize,
) -> Result<String, UnfoldError> {
    let code = code_of(unit, alphabet)?;
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

/// Whether lines folded with these `choices` need an index to unfold.
pub fn needs_index(choices: &str, alphabet: Alphabet) -> Result<bool, UnfoldError> {
    Ok(Choices::read(choices, alphabet)?.index.is_some())
}

/// What a fold into parts decided, as its choices record it: every way it could write something
/// one of several ways, and the way it took.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decisions {
    /// The parts' format.
    pub format: u64,
    /// How many low bits of a run's count are written as they are.
    pub count_low_bits: u32,
    /// Whether a run's count has its length in bits in a Huffman code of its own, rather than in
    /// unary.
    pub counts_coded: bool,
    /// How many low bits of a character's step from the one before, in the table, are written as
    /// they are.
    pub step_low_bits: u32,
    /// How the table writes its code lengths: `"fixed"`, five bits each, or `"huffman"`, in a code
    /// of their own.
    pub lengths: &'static str,
    /// How many of the index's values the lines were folded with, if they need an index.
    pub index_values: Option<u64>,
}

/// What the fold that wrote these `choices` decided.
pub fn decisions(choices: &str, alphabet: Alphabet) -> Result<Decisions, UnfoldError> {
    let choices = Choices::read(choices, alphabet)?;
    Ok(Decisions {
        format: choices.format,
        count_low_bits: choices.k,
        counts_coded: choices.coded,
        step_low_bits: choices.table.step_bits,
        lengths: choices.table.lengths.name(),
        index_values: choices.index,
    })
}

/// `inputs` folded with one code learnt over all their lines, indexing if `index`.
fn together_with(inputs: &[&str], alphabet: Alphabet, index: bool) -> Together {
    let strings: Vec<&str> =
        inputs.iter().flat_map(|input| input.split('\n')).filter(|line| !line.is_empty()).collect();
    let code = TextCode::learn_for_parts(&strings, index);
    let unit = unit_of(&code, index, alphabet, code.table_choices());
    let lines = inputs.iter().map(|input| spelled(&code, input, alphabet)).collect();
    Together { lines, unit }
}

/// The unit of `code`: its choices, its table written as `table_choices` say, and, when `index`,
/// its longer texts as its index.
fn unit_of(code: &TextCode, index: bool, alphabet: Alphabet, table_choices: TableChoices) -> Unit {
    let values = code.symbols();
    let index = (index && !values.is_empty())
        .then(|| Index::new(values).expect("a code's symbols are distinct"));
    let mut writer = BitWriter::new();
    code.write_compact(&mut writer, index.as_ref(), table_choices);
    let table = alphabet.fold_bits(&writer.finish());
    let (k, coded) = code.count_choices();
    let mark = index.as_ref().map(|index| (index.len() as u64, index.fingerprint_of(index.len())));
    let values = mark.map(|(count, _)| count);
    let choices =
        Choices { format: FORMAT, k, coded, table: table_choices, index: values, fingerprint: 0 };
    let fingerprint = choices.fingerprint(mark.map(|(_, print)| print), &table);
    Unit { choices: Choices { fingerprint, ..choices }.write(alphabet), table, index }
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

/// The code `unit` holds, once its choices say its table and index are the ones they were made
/// with.
fn code_of(unit: &Unit, alphabet: Alphabet) -> Result<TextCode, UnfoldError> {
    let choices = Choices::read(&unit.choices, alphabet)?;
    let print = match (choices.index, &unit.index) {
        (None, None) => None,
        (None, Some(_)) => {
            return Err(UnfoldError::Unit("an index, for lines that need none".to_string()));
        }
        (Some(_), None) => return Err(UnfoldError::NeedsIndex),
        (Some(count), Some(index)) if count <= index.len() as u64 => {
            Some(index.fingerprint_of(count as usize))
        }
        (Some(_), Some(_)) => return Err(UnfoldError::WrongIndex),
    };
    if choices.fingerprint(print, &unit.table) != choices.fingerprint {
        return Err(UnfoldError::PartsDontFit);
    }
    let table = &unit.table;
    let bits =
        alphabet.unfold_bits(table).map_err(|error| UnfoldError::CodeTable(error.to_string()))?;
    let mut reader = bits.reader();
    let index = unit.index.as_ref();
    let code = TextCode::read_compact(&mut reader, index, choices.k, choices.coded, choices.table)
        .map_err(UnfoldError::CodeTable)?;
    match exactly(table, &bits, &reader, alphabet) {
        true => Ok(code),
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

/// What the choices say: their format, how counts are written, how the table is written, how
/// much of an index the lines need, and the fingerprint of it all and of the table.
#[derive(Clone, Copy, Debug)]
struct Choices {
    format: u64,
    /// The count's low bits written as they are.
    k: u32,
    /// Whether the count's buckets have a code of their own.
    coded: bool,
    /// How the table writes its code lengths and its characters' steps.
    table: TableChoices,
    /// How many of the index's values the lines were folded with, if they need one.
    index: Option<u64>,
    fingerprint: u32,
}

/// How many bits the choices give the low bits of a run's count.
const COUNT_BITS: u32 = 3;

/// How many bits the choices give the low bits of a character's step.
const STEP_BITS: u32 = 4;

/// How many bits the choices give the way the table writes its code lengths.
const LENGTHS_BITS: u32 = 2;

impl Choices {
    /// The bits: the format in four, `k` in three, whether counts are coded in one, the step's
    /// low bits in four, the way of writing code lengths in two, whether there is an index in one,
    /// how much of the index in Elias gamma, and the fingerprint in 32. Format 1 had neither the
    /// step's low bits nor the way of writing lengths.
    fn write(&self, alphabet: Alphabet) -> String {
        let mut writer = BitWriter::new();
        writer.write(self.format, FORMAT_BITS);
        writer.write(self.k as u64, COUNT_BITS);
        writer.bit(self.coded);
        if self.format >= 2 {
            writer.write(self.table.step_bits as u64, STEP_BITS);
            writer.write(self.table.lengths.number(), LENGTHS_BITS);
        }
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
        let format = match reader.read(FORMAT_BITS).ok_or_else(short)? {
            0 => return Err(bad("no format is numbered 0")),
            format if format <= FORMAT => format,
            newer => {
                let why = format!("parts format {newer}, newer than this orimoji reads ({FORMAT})");
                return Err(bad(&why));
            }
        };
        let k = reader.read(COUNT_BITS).ok_or_else(short)? as u32;
        let coded = reader.bit().ok_or_else(short)?;
        let table = match format {
            1 => TableChoices::FIRST,
            _ => {
                let step_bits = reader.read(STEP_BITS).ok_or_else(short)? as u32;
                let number = reader.read(LENGTHS_BITS).ok_or_else(short)?;
                let lengths = LengthsCode::numbered(number).ok_or_else(|| {
                    bad(&format!("code lengths written a way this orimoji doesn't know ({number})"))
                })?;
                TableChoices { lengths, step_bits }
            }
        };
        let index = match reader.bit().ok_or_else(short)? {
            true => Some(reader.gamma().ok_or_else(short)?),
            false => None,
        };
        let fingerprint = reader.read(32).ok_or_else(short)? as u32;
        match exactly(text, &bits, &reader, alphabet) {
            true => Ok(Self { format, k, coded, table, index, fingerprint }),
            false => Err(bad("more after them than the zeros that fill them out")),
        }
    }

    /// FNV-1a over what the choices say, the fingerprint of the index's values they count (if
    /// any), and the table, as written.
    fn fingerprint(&self, index: Option<u32>, table: &str) -> u32 {
        let table_said = match self.format {
            1 => String::new(),
            _ => format!("{} {} ", self.table.step_bits, self.table.lengths.number()),
        };
        let said = format!(
            "{} {} {} {table_said}{}.{:08x}\n",
            self.format,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::squeeze::text::MOST_STEP_BITS;

    fn texts() -> Vec<String> {
        let runs: String = (0..40).map(|i| format!("{}|", "x".repeat(3 + i * 37 % 400))).collect();
        vec![
            "⣿⣿⣿⣿⣿⣿⡇ ⢸⣿⣿\n⣿⣿⡇      ⢸⣿⣿⣿⣿\n\n⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿".to_string(),
            "白猫骑士物语 / White Cat Knight\nעברית מימין לשמאל\n🎮👾 ███░░".to_string(),
            runs,
            (0..12)
                .map(|i| format!("\u{1b}[38;2;226;57;57m█ Simulator {i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        ]
    }

    /// Every decision, forced one way and then another, writes parts that unfold to the text they
    /// folded, and choices that say so: each run count code, each way of writing code lengths, and
    /// low bits for the steps from none to the most.
    #[test]
    fn every_decision_writes_parts_that_unfold_and_choices_that_say_so() {
        for text in texts() {
            let strings: Vec<&str> = text.split('\n').filter(|line| !line.is_empty()).collect();
            for index in [false, true] {
                let learnt = TextCode::learn_for_parts(&strings, index);
                for k in 0..=crate::squeeze::text::MOST_LOW_BITS {
                    for coded in [false, true] {
                        for lengths in LengthsCode::ALL {
                            for step_bits in [0, 5, MOST_STEP_BITS] {
                                let table = TableChoices { lengths, step_bits };
                                let code = learnt.clone().with_counts(k, coded);
                                let unit = unit_of(&code, index, Alphabet::Base64Url, table);
                                let lines = spelled(&code, &text, Alphabet::Base64Url);
                                let back = unfold(&lines, &unit, Alphabet::Base64Url, 1 << 24);
                                assert_eq!(
                                    back.ok().as_ref(),
                                    Some(&text),
                                    "{k} {coded} {table:?}"
                                );
                                let decided = unit.decisions(Alphabet::Base64Url).unwrap();
                                assert_eq!(
                                    decided,
                                    Decisions {
                                        format: FORMAT,
                                        count_low_bits: k,
                                        counts_coded: coded,
                                        step_low_bits: step_bits,
                                        lengths: lengths.name(),
                                        index_values: unit
                                            .index
                                            .as_ref()
                                            .map(|index| index.len() as u64),
                                    }
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// A way of writing code lengths this orimoji doesn't know is refused by its number, and
    /// format 0, which no orimoji wrote, as damage.
    #[test]
    fn choices_naming_a_way_this_orimoji_does_not_know_are_refused() {
        let choices = |format: u64, lengths: u64| {
            let mut writer = BitWriter::new();
            writer.write(format, FORMAT_BITS);
            writer.write(0, COUNT_BITS);
            writer.bit(false);
            writer.write(0, STEP_BITS);
            writer.write(lengths, LENGTHS_BITS);
            writer.bit(false);
            writer.write(0, 32);
            Alphabet::Base64Url.fold_bits(&writer.finish())
        };
        for (choices, says) in [
            (choices(2, 3), "a way this orimoji doesn't know (3)"),
            (choices(2, 2), "a way this orimoji doesn't know (2)"),
            (choices(0, 0), "no format is numbered 0"),
        ] {
            match decisions(&choices, Alphabet::Base64Url) {
                Err(UnfoldError::Choices(why)) => assert!(why.contains(says), "{why}"),
                other => panic!("{says}: {other:?}"),
            }
        }
        assert!(decisions(&choices(2, 1), Alphabet::Base64Url).is_ok(), "the ways it knows read");
    }
}
