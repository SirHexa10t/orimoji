//! The text squeeze: each line spelled in one Huffman code the whole file shares, and decoded on its
//! own, so a changed line changes its own body line and no other.
//!
//! The code spells characters, and runs: three or more of the same character, word or escape in
//! a row are written once, then a `<run>` symbol and how many more, as drawings and indentation are
//! full of them. When the fold is asked to index, it also spells two things that are more than a
//! character, each as one symbol:
//!
//! - **Words.** A whole word (letters, digits, `_`): `Simulator`, `Soundtrack`, `Entertainment`.
//! - **Terminal escapes.** A CSI sequence (`ESC [ … m` and its kin), so a colour costs one code
//!   instead of a dozen characters.
//!
//! Either becomes a symbol only if it saves at least twice the space its table line takes.
//!
//! Every code also has `<esc>`, for a character the table doesn't hold: when a fold keeps an older
//! fold's tables, a new character is spelled as its code point instead of rebuilding the table.
//!
//! A fold into parts (see [`crate::parts`]) learns its code the same way, then goes further:
//!
//! - **A run only where it pays.** Under a code that gives a unit very few bits, writing it again is
//!   shorter than a `<run>` and a count; each run is weighed, and the code learnt again from what
//!   the weighing chose, until it settles.
//! - **Counts in a code chosen for them** (see [`CountCode`]): Elias gamma as everywhere else, or
//!   with low bits written as they are, or with their lengths in a Huffman code of their own,
//!   whichever writes this text's counts in the fewest bits.
//! - **A compact table**: code lengths as five bits each, and characters as steps from the one
//!   before, where a folded file writes a line per symbol.

use super::{Body, Kept, Line, Squeezed, Tables};
use crate::bits::{BitReader, BitWriter};
use crate::container::{self, Folded, Line as FileLine};
use crate::deadline::Deadline;
use crate::error::{FoldError, UnfoldError};
use crate::escape::escape;
use crate::huffman::{self, Decoder, Encoder};
use crate::index::{Index, read_entry, write_entry};
use std::collections::{BTreeMap, HashMap, HashSet};

/// The most repeats one `<run>` stands for; a longer run is written as several. So a damaged or
/// hostile file asks for at most this many units per `<run>`, which takes at least two bits.
const LONGEST_RUN: u64 = 4096;

/// What a text code spells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Symbol {
    End,
    Esc,
    Run,
    Text(String),
}

/// A Huffman code for spelling text.
#[derive(Clone, Debug)]
pub(crate) struct TextCode {
    symbols: Vec<Symbol>,
    index: HashMap<String, usize>,
    encoder: Encoder,
    decoder: Decoder,
    end: usize,
    esc: Option<usize>,
    run: Option<usize>,
    /// How a run's count is written: Elias gamma in a folded file's code.
    counts: CountCode,
    /// Whether a run is weighed against writing its unit again, as a fold into parts does; a folded
    /// file's code writes every run of three or more as one.
    per_run: bool,
}

/// How a run's count is written. The count, the repeats past the first two, is split into a high
/// part and its `k` low bits. The high part, plus one, is written as its length in bits (its
/// bucket), then its bits after the leading one; then the `k` low bits, as they are. The bucket is
/// written in unary, as Elias gamma writes a number's length, or in a Huffman code of its own. In
/// unary with `k = 0`, it is Elias gamma itself, the code every folded file's runs are written in.
#[derive(Clone, Debug)]
pub(crate) struct CountCode {
    k: u32,
    buckets: Option<(Encoder, Decoder)>,
}

/// The most `k` a fold into parts tries: past it, short counts pay more than long ones save.
pub(crate) const MOST_LOW_BITS: u32 = 7;

/// The most buckets a count code may have: enough for any count of [`LONGEST_RUN`] repeats.
const MOST_BUCKETS: u64 = 13;

/// How many bits a code length takes in a compact table: lengths run from 1 to
/// [`huffman::MAX_LENGTH`].
const LENGTH_BITS: u32 = 5;

/// One step of spelling: a character or escape, or a run of the one before.
enum Event<'a> {
    Unit(&'a str),
    Run(u64),
}

impl TextCode {
    /// The code that spells `strings` in the fewest bits; with `index`, words and escapes that pay
    /// for themselves twice over are single symbols.
    #[cfg(test)]
    pub(crate) fn learn<'a, I>(strings: I, index: bool) -> Self
    where
        I: IntoIterator<Item = &'a str>,
        I::IntoIter: Clone,
    {
        Self::learn_within(strings, index, Deadline::NONE).expect("no deadline to pass")
    }

    /// [`TextCode::learn`], giving up past `deadline`.
    pub(crate) fn learn_within<'a, I>(
        strings: I,
        index: bool,
        deadline: Deadline,
    ) -> Result<Self, FoldError>
    where
        I: IntoIterator<Item = &'a str>,
        I::IntoIter: Clone,
    {
        let strings = strings.into_iter();
        let known = known_tokens(strings.clone(), index, deadline)?;
        let (mut counts, mut ends, mut runs) = (HashMap::<&str, u64>::new(), 0u64, 0u64);
        for (at, string) in strings.enumerate() {
            deadline.check_at(at)?;
            for event in events(&units(string, |token| known.contains(token))) {
                match event {
                    Event::Unit(unit) => *counts.entry(unit).or_default() += 1,
                    Event::Run(_) => runs += 1,
                }
            }
            ends += 1;
        }
        let mut texts: Vec<(&str, u64)> = counts.into_iter().collect();
        texts.sort_unstable();
        let mut symbols = vec![Symbol::End, Symbol::Esc, Symbol::Run];
        // Every symbol gets a code, used or not, so that a later fold can keep this table.
        let mut weights = vec![ends.max(1), 1, runs + 1];
        for (text, count) in texts {
            symbols.push(Symbol::Text(text.to_string()));
            weights.push(count);
        }
        Ok(Self::new(symbols, huffman::lengths(&weights))
            .expect("Huffman lengths make a prefix code"))
    }

    fn new(symbols: Vec<Symbol>, lengths: Vec<u32>) -> Option<Self> {
        let decoder = Decoder::new(&lengths)?;
        let encoder = Encoder::new(lengths)?;
        let find = |wanted: &Symbol| symbols.iter().position(|symbol| symbol == wanted);
        let (end, esc, run) = (find(&Symbol::End)?, find(&Symbol::Esc), find(&Symbol::Run));
        let index = symbols
            .iter()
            .enumerate()
            .filter_map(|(at, symbol)| match symbol {
                Symbol::Text(text) => Some((text.clone(), at)),
                _ => None,
            })
            .collect();
        let (counts, per_run) = (CountCode::GAMMA, false);
        Some(Self { symbols, index, encoder, decoder, end, esc, run, counts, per_run })
    }

    /// The symbols longer than a character: the words and escapes it indexed.
    pub(crate) fn symbols(&self) -> Vec<String> {
        self.symbols
            .iter()
            .filter_map(|symbol| match symbol {
                Symbol::Text(text) if text.chars().nth(1).is_some() => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Spells `text`, then `<end>`.
    pub(crate) fn write(&self, writer: &mut BitWriter, text: &str) {
        let known = |token: &str| self.index.contains_key(token);
        let units = units(text, known);
        let events = match self.per_run {
            true => plan(&units, self.longest_run(), |unit, same| self.run_pays(unit, same)),
            false => events(&units),
        };
        for event in events {
            match event {
                Event::Unit(unit) => match self.index.get(unit) {
                    Some(&symbol) => self.encoder.write(writer, symbol),
                    None => {
                        let esc = self.esc.expect("every table this orimoji writes has <esc>");
                        self.encoder.write(writer, esc);
                        let c = unit.chars().next().expect("a unit is never empty");
                        writer.gamma(c as u64 + 1);
                    }
                },
                Event::Run(repeats) => {
                    match self.run {
                        Some(run) => self.encoder.write(writer, run),
                        None => unreachable!("every table this orimoji writes has <run>"),
                    }
                    self.counts.write(writer, repeats - 2);
                }
            }
        }
        self.encoder.write(writer, self.end);
    }

    /// What [`TextCode::write`] spelled, or `None` where the bits don't spell it.
    pub(crate) fn read(&self, reader: &mut BitReader<'_>) -> Option<String> {
        self.read_within(reader, usize::MAX).ok()
    }

    /// What [`TextCode::write`] spelled, refused where the bits don't spell it or where it would
    /// come to more than `room` bytes, before those bytes are made.
    pub(crate) fn read_within(
        &self,
        reader: &mut BitReader<'_>,
        room: usize,
    ) -> Result<String, Misread> {
        let mut out = String::new();
        let mut last: Option<(usize, usize)> = None;
        let fits = |out: &String, more: usize| match out.len().checked_add(more) {
            Some(size) if size <= room => Ok(()),
            _ => Err(Misread::TooLarge),
        };
        loop {
            let symbol = self.decoder.read(reader).ok_or(Misread::Corrupt)?;
            match &self.symbols[symbol] {
                Symbol::End => return Ok(out),
                Symbol::Text(text) => {
                    fits(&out, text.len())?;
                    last = Some((out.len(), out.len() + text.len()));
                    out.push_str(text);
                }
                Symbol::Esc => {
                    let point = reader.gamma().ok_or(Misread::Corrupt)? - 1;
                    let c = u32::try_from(point).ok().and_then(char::from_u32);
                    let c = c.ok_or(Misread::Corrupt)?;
                    fits(&out, c.len_utf8())?;
                    last = Some((out.len(), out.len() + c.len_utf8()));
                    out.push(c);
                }
                Symbol::Run => {
                    let repeats = self.counts.read(reader).ok_or(Misread::Corrupt)? + 2;
                    let (start, end) = last.ok_or(Misread::Corrupt)?;
                    if repeats > LONGEST_RUN {
                        return Err(Misread::Corrupt);
                    }
                    fits(&out, (end - start) * repeats as usize)?;
                    let unit = out[start..end].to_string();
                    for _ in 0..repeats {
                        out.push_str(&unit);
                    }
                }
            }
        }
    }

    /// The most repeats past a unit one `<run>` writes: [`LONGEST_RUN`], or fewer where the count
    /// code has no bucket for more.
    fn longest_run(&self) -> u64 {
        LONGEST_RUN.min(self.counts.most().saturating_add(2))
    }

    /// Whether `same` more of `unit` cost fewer bits, or as many, as a `<run>` than written again.
    /// A count past the last bucket of a code learnt from shorter runs is weighed as Elias gamma
    /// writes it, as the code learnt next will give it a bucket.
    fn run_pays(&self, unit: &str, same: u64) -> bool {
        let run = self.run.map_or(0, |run| self.encoder.length(run)) as u64;
        let unary = CountCode { k: self.counts.k, buckets: None };
        let count = self.counts.bits(same - 2).or_else(|| unary.bits(same - 2));
        let count = count.expect("unary writes any count") as u64;
        run + count <= same * self.unit_bits(unit)
    }

    /// The bits `unit` costs: its code, or `<esc>` and its code point.
    fn unit_bits(&self, unit: &str) -> u64 {
        match self.index.get(unit) {
            Some(&symbol) => self.encoder.length(symbol) as u64,
            None => {
                let esc = self.esc.map_or(0, |esc| self.encoder.length(esc)) as u64;
                let point = unit.chars().next().map_or(0, |c| c as u64);
                esc + gamma_bits(point + 1) as u64
            }
        }
    }

    /// The code a fold into parts spells `strings` in: learnt as a folded file's is, then learnt
    /// again from where its runs pay, until that settles, with its counts in the code that writes
    /// them shortest. Its symbols are in a compact table's order: `<end>`, `<esc>`, `<run>`, the
    /// characters by code point, then the longer texts, in order, which is the index's.
    pub(crate) fn learn_for_parts(strings: &[&str], index: bool) -> Self {
        let known = known_tokens(strings.iter().copied(), index, Deadline::NONE)
            .expect("no deadline to pass");
        let units: Vec<Vec<&str>> =
            strings.iter().map(|string| units(string, |token| known.contains(token))).collect();
        let mut code = Self::planned(&units, None);
        // Each round weighs the runs under the code before it. It settles in a round or two; the
        // limit only bounds a code that would flip between two forever.
        for _ in 0..4 {
            let next = Self::planned(&units, Some(&code));
            if next.same_code_as(&code) {
                break;
            }
            code = next;
        }
        code
    }

    /// The code that spells `lines` of units, its runs where `under` says they pay (all of them,
    /// without one), its counts in the code that writes them shortest.
    fn planned(lines: &[Vec<&str>], under: Option<&TextCode>) -> Self {
        let (mut counts, mut runs, mut ends) = (BTreeMap::<&str, u64>::new(), 0u64, 0u64);
        let mut values = Vec::new();
        for line in lines {
            let events = match under {
                Some(code) => plan(line, LONGEST_RUN, |unit, same| code.run_pays(unit, same)),
                None => events(line),
            };
            for event in events {
                match event {
                    Event::Unit(unit) => *counts.entry(unit).or_default() += 1,
                    Event::Run(repeats) => {
                        runs += 1;
                        values.push(repeats - 2);
                    }
                }
            }
            ends += 1;
        }
        // Characters, then longer texts: each group in order, as a compact table lists them.
        let (characters, texts): (Vec<_>, Vec<_>) =
            counts.into_iter().partition(|(unit, _)| unit.chars().nth(1).is_none());
        let mut symbols = vec![Symbol::End, Symbol::Esc, Symbol::Run];
        // Every symbol gets a code, used or not, so that a later fold can keep this table.
        let mut weights = vec![ends.max(1), 1, runs + 1];
        for (unit, count) in characters.into_iter().chain(texts) {
            symbols.push(Symbol::Text(unit.to_string()));
            weights.push(count);
        }
        let code = Self::new(symbols, huffman::lengths(&weights))
            .expect("Huffman lengths make a prefix code");
        Self { counts: CountCode::cheapest(&values), per_run: true, ..code }
    }

    /// Whether `other` spells every symbol in the same bits, its counts too.
    fn same_code_as(&self, other: &TextCode) -> bool {
        self.symbols == other.symbols
            && self.encoder.lengths() == other.encoder.lengths()
            && self.counts.same_as(&other.counts)
    }

    /// How its counts are written: `k`, and whether their buckets have a code of their own.
    pub(crate) fn count_choices(&self) -> (u32, bool) {
        (self.counts.k, self.counts.buckets.is_some())
    }

    /// Its compact table: how many characters and how many longer texts it spells; the code
    /// lengths of `<end>`, `<esc>` and `<run>`; each character as a step from the one before (the
    /// first, from nothing) and its code length; each longer text as a step from the place in
    /// `index` of the one before, and its code length; and, when its counts' buckets have a code
    /// of their own, how many buckets there are and each one's code length. Numbers are in Elias
    /// gamma, code lengths five bits each.
    pub(crate) fn write_compact(&self, writer: &mut BitWriter, index: Option<&Index>) {
        let texts = |many: bool| {
            self.symbols[3..].iter().filter_map(move |symbol| match symbol {
                Symbol::Text(text) if text.chars().nth(1).is_some() == many => Some(text),
                _ => None,
            })
        };
        let (characters, longer) = (texts(false).count(), texts(true).count());
        writer.gamma(characters as u64 + 1);
        writer.gamma(longer as u64 + 1);
        for symbol in 0..3 {
            writer.write(self.encoder.length(symbol) as u64, LENGTH_BITS);
        }
        let mut before = None;
        for text in texts(false) {
            let point = text.chars().next().expect("a character") as u64;
            writer.gamma(before.map_or(point + 1, |before| point - before));
            writer.write(self.encoder.length(self.index[text.as_str()]) as u64, LENGTH_BITS);
            before = Some(point);
        }
        let mut before = None;
        for text in texts(true) {
            let place = index.and_then(|index| index.place(text));
            let place = place.expect("a fold into parts indexes every longer text") as u64;
            writer.gamma(before.map_or(place + 1, |before| place - before));
            writer.write(self.encoder.length(self.index[text.as_str()]) as u64, LENGTH_BITS);
            before = Some(place);
        }
        if let Some((encoder, _)) = &self.counts.buckets {
            writer.gamma(encoder.lengths().len() as u64);
            for &length in encoder.lengths() {
                writer.write(length as u64, LENGTH_BITS);
            }
        }
    }

    /// The code a [`TextCode::write_compact`] table holds, its counts written with `k` low bits
    /// as they are and, when `coded`, their buckets in a code of their own. Refused, with what is
    /// wrong, where the bits are no table a fold writes.
    pub(crate) fn read_compact(
        reader: &mut BitReader<'_>,
        index: Option<&Index>,
        k: u32,
        coded: bool,
    ) -> Result<Self, String> {
        let short = || "it is cut short".to_string();
        // Each entry takes six bits at least, so a count the bits can't hold is refused before
        // anything is made for it.
        let how_many = |reader: &mut BitReader<'_>| {
            let many = reader.gamma().ok_or_else(short)? - 1;
            match many <= reader.remaining() as u64 / 6 {
                true => Ok(many as usize),
                false => Err(format!("it lists {many} entries, more than it has bits for")),
            }
        };
        let (characters, longer) = (how_many(reader)?, how_many(reader)?);
        let length = |reader: &mut BitReader<'_>| {
            let length = reader.read(LENGTH_BITS).ok_or_else(short)? as u32;
            match (1..=huffman::MAX_LENGTH).contains(&length) {
                true => Ok(length),
                false => Err(format!("a code {length} bits long, which no code is")),
            }
        };
        let mut symbols = vec![Symbol::End, Symbol::Esc, Symbol::Run];
        let mut lengths = vec![length(reader)?, length(reader)?, length(reader)?];
        let mut before = None;
        for _ in 0..characters {
            let step = reader.gamma().ok_or_else(short)?;
            let point = before.map_or(Some(step - 1), |before: u64| before.checked_add(step));
            let c = point.and_then(|point| u32::try_from(point).ok()).and_then(char::from_u32);
            let c = c.ok_or_else(|| "a character past the last there is".to_string())?;
            symbols.push(Symbol::Text(c.to_string()));
            lengths.push(length(reader)?);
            before = Some(c as u64);
        }
        let mut before = None;
        for _ in 0..longer {
            let step = reader.gamma().ok_or_else(short)?;
            let place = before.map_or(Some(step - 1), |before: u64| before.checked_add(step));
            let index = index.ok_or_else(|| "places in an index, and there is none".to_string())?;
            let text = place.and_then(|place| index.value(usize::try_from(place).ok()?));
            let text = text.ok_or_else(|| "a place past the index's end".to_string())?;
            if text.chars().nth(1).is_none() {
                return Err(format!("`{}` in the index, which a fold never indexes", escape(text)));
            }
            symbols.push(Symbol::Text(text.to_string()));
            lengths.push(length(reader)?);
            before = place;
        }
        let counts = match coded {
            false => CountCode { k, buckets: None },
            true => {
                let buckets = reader.gamma().ok_or_else(short)?;
                if buckets > MOST_BUCKETS {
                    return Err(format!("{buckets} buckets of counts, more than any count needs"));
                }
                let lengths: Vec<u32> =
                    (0..buckets).map(|_| length(reader)).collect::<Result<_, _>>()?;
                let coded = Encoder::new(lengths.clone()).zip(Decoder::new(&lengths));
                let coded =
                    coded.ok_or_else(|| "the counts' code is no prefix code".to_string())?;
                CountCode { k, buckets: Some(coded) }
            }
        };
        let code = Self::new(symbols, lengths)
            .ok_or_else(|| "its code lengths are no prefix code".to_string())?;
        Ok(Self { counts, per_run: true, ..code })
    }

    /// The table, a line per symbol: its code length, a tab, and the symbol.
    pub(crate) fn table(&self, index: Option<&Index>) -> Vec<String> {
        let name = |symbol: &Symbol| match symbol {
            Symbol::End => "<end>".to_string(),
            Symbol::Esc => "<esc>".to_string(),
            Symbol::Run => "<run>".to_string(),
            Symbol::Text(text) => write_entry(text, index),
        };
        self.symbols
            .iter()
            .enumerate()
            .map(|(at, symbol)| format!("{}\t{}", self.encoder.length(at), name(symbol)))
            .collect()
    }

    /// The code a [`TextCode::table`] describes.
    pub(crate) fn from_table(
        lines: &[FileLine<'_>],
        index: Option<&Index>,
    ) -> Result<Self, UnfoldError> {
        let (mut symbols, mut lengths) = (Vec::new(), Vec::new());
        for line in lines {
            let problem = |problem: String| UnfoldError::Table { line: line.number, problem };
            let (length, name) =
                line.text.split_once('\t').ok_or_else(|| problem("no tab".into()))?;
            lengths.push(
                length.parse().map_err(|_| problem(format!("`{length}` is no code length")))?,
            );
            symbols.push(match name {
                "<end>" => Symbol::End,
                "<esc>" => Symbol::Esc,
                "<run>" => Symbol::Run,
                _ => Symbol::Text(read_entry(name, index).map_err(problem)?),
            });
        }
        let number = lines.first().map_or(0, |line| line.number);
        Self::new(symbols, lengths).ok_or(UnfoldError::Table {
            line: number,
            problem: "this spelling table is no prefix code with an <end>".into(),
        })
    }
}

/// Why [`TextCode::read_within`] refused a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Misread {
    /// Its bits spell no line this code writes.
    Corrupt,
    /// It would come to more than the room it was given.
    TooLarge,
}

impl CountCode {
    /// Elias gamma: the bucket in unary, no low bits.
    pub(crate) const GAMMA: CountCode = CountCode { k: 0, buckets: None };

    /// The largest count it can write: any, in unary; up to its last bucket's, coded.
    fn most(&self) -> u64 {
        match &self.buckets {
            None => u64::MAX,
            Some((encoder, _)) => {
                let buckets = encoder.lengths().len() as u32;
                let high = (1u64 << buckets) - 2;
                (high << self.k) | ((1 << self.k) - 1)
            }
        }
    }

    /// The bucket of `count`, and the high part it is the length of, plus one.
    fn split(&self, count: u64) -> (u32, u64) {
        let high = (count >> self.k) + 1;
        (63 - high.leading_zeros(), high)
    }

    /// How many bits `count` takes, or `None` if it has no bucket here.
    fn bits(&self, count: u64) -> Option<u32> {
        let (bucket, _) = self.split(count);
        let prefix = match &self.buckets {
            None => bucket + 1,
            Some((encoder, _)) => *encoder.lengths().get(bucket as usize)?,
        };
        Some(prefix + bucket + self.k)
    }

    /// Writes `count`, which must have a bucket.
    fn write(&self, writer: &mut BitWriter, count: u64) {
        let (bucket, high) = self.split(count);
        match &self.buckets {
            None => writer.write(1, bucket + 1),
            Some((encoder, _)) => encoder.write(writer, bucket as usize),
        }
        writer.write(high, bucket);
        writer.write(count, self.k);
    }

    /// A count [`CountCode::write`] wrote, or `None` where the bits spell none, or one past
    /// any run's.
    fn read(&self, reader: &mut BitReader<'_>) -> Option<u64> {
        let bucket = match &self.buckets {
            None => {
                let mut zeros = 0;
                while !reader.bit()? {
                    zeros += 1;
                    if zeros as u64 >= MOST_BUCKETS {
                        return None;
                    }
                }
                zeros
            }
            Some((_, decoder)) => decoder.read(reader)? as u32,
        };
        let high = (1u64 << bucket) | reader.read(bucket)?;
        let low = reader.read(self.k)?;
        Some(((high - 1) << self.k) | low)
    }

    /// Whether `other` writes every count in the same bits.
    fn same_as(&self, other: &CountCode) -> bool {
        let lengths =
            |code: &CountCode| code.buckets.as_ref().map(|(encoder, _)| encoder.lengths().to_vec());
        self.k == other.k && lengths(self) == lengths(other)
    }

    /// The code that writes `counts` in the fewest bits, the length of its buckets' code counted
    /// against it: each `k` up to [`MOST_LOW_BITS`], the buckets in unary or coded. On a tie, unary
    /// and the smaller `k`. No counts at all, Elias gamma.
    fn cheapest(counts: &[u64]) -> CountCode {
        let mut best = (u64::MAX, CountCode::GAMMA);
        for k in 0..=MOST_LOW_BITS {
            let unary = CountCode { k, buckets: None };
            let buckets: Vec<u32> = counts.iter().map(|&count| unary.split(count).0).collect();
            let size: u64 = buckets.iter().map(|&bucket| (2 * bucket + 1 + k) as u64).sum();
            if size < best.0 {
                best = (size, unary);
            }
            let Some(&last) = buckets.iter().max() else { continue };
            let mut weights = vec![1u64; last as usize + 1];
            for &bucket in &buckets {
                weights[bucket as usize] += 1;
            }
            let lengths = huffman::lengths(&weights);
            let table =
                gamma_bits(weights.len() as u64) as u64 + LENGTH_BITS as u64 * weights.len() as u64;
            let size: u64 = table
                + buckets
                    .iter()
                    .map(|&bucket| (lengths[bucket as usize] + bucket + k) as u64)
                    .sum::<u64>();
            if size < best.0 {
                let coded = Encoder::new(lengths.clone()).zip(Decoder::new(&lengths));
                best = (size, CountCode { k, buckets: coded });
            }
        }
        best.1
    }
}

/// The bits Elias gamma writes `n`, 1 or more, in.
fn gamma_bits(n: u64) -> u32 {
    2 * (63 - n.leading_zeros()) + 1
}

/// The words and escapes in `strings` worth a symbol of their own, if asked to index at all.
fn known_tokens<'a>(
    strings: impl Iterator<Item = &'a str>,
    index: bool,
    deadline: Deadline,
) -> Result<HashSet<&'a str>, FoldError> {
    let mut tokens: HashMap<&str, u64> = HashMap::new();
    if index {
        for (at, string) in strings.enumerate() {
            deadline.check_at(at)?;
            let words =
                string.split(|c: char| !wordy(c)).filter(|word| word.chars().nth(1).is_some());
            for token in csi_tokens(string).chain(words) {
                *tokens.entry(token).or_default() += 1;
            }
        }
    }
    Ok(tokens
        .into_iter()
        .filter(|&(token, count)| worth_a_symbol(token, count))
        .map(|(token, _)| token)
        .collect())
}

/// Whether `c` is part of a word.
fn wordy(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Whether a word or escape seen `count` times saves, as one symbol, at least twice the space its
/// table line takes. A character is guessed at 5 bits, the symbol's own code at 12, and a bit of
/// the body at a sixth of a byte, as in base64.
fn worth_a_symbol(token: &str, count: u64) -> bool {
    let saved = count as f64 * (token.chars().count() as f64 * 5.0 - 12.0) / 6.0;
    saved >= 2.0 * (escape(token).len() as f64 + 4.0)
}

/// The CSI sequences in `text`: `ESC [`, parameter bytes, intermediate bytes, a final byte.
fn csi_tokens(text: &str) -> impl Iterator<Item = &str> + '_ {
    text.match_indices('\u{1b}').filter_map(move |(at, _)| csi_at(text, at))
}

fn csi_at(text: &str, at: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.get(at + 1) != Some(&b'[') {
        return None;
    }
    let mut end = at + 2;
    while bytes.get(end).is_some_and(|byte| (0x30..=0x3F).contains(byte)) {
        end += 1;
    }
    while bytes.get(end).is_some_and(|byte| (0x20..=0x2F).contains(byte)) {
        end += 1;
    }
    bytes.get(end).filter(|byte| (0x40..=0x7E).contains(*byte)).map(|_| &text[at..=end])
}

/// `text` as its units: its characters, with each known escape and known whole word as one.
fn units(text: &str, known: impl Fn(&str) -> bool) -> Vec<&str> {
    let mut units = Vec::with_capacity(text.len());
    let (mut at, mut after_word) = (0, false);
    while at < text.len() {
        let rest = &text[at..];
        let c = rest.chars().next().expect("inside the text");
        let word = match wordy(c) && !after_word {
            true => Some(&rest[..rest.find(|c: char| !wordy(c)).unwrap_or(rest.len())]),
            false => None,
        };
        let len = match (
            csi_at(text, at).filter(|token| known(token)),
            word.filter(|word| known(word)),
        ) {
            (Some(token), _) => token.len(),
            (None, Some(word)) => word.len(),
            (None, None) => c.len_utf8(),
        };
        after_word = wordy(rest[..len].chars().next_back().expect("a unit has a character"));
        units.push(&rest[..len]);
        at += len;
    }
    units
}

/// Units, with three or more alike in a row written as the first and a run of the rest.
fn events<'a>(units: &[&'a str]) -> Vec<Event<'a>> {
    plan(units, LONGEST_RUN, |_, _| true)
}

/// Units, with three or more alike in a row written as the first and a run of the rest where
/// `run_pays` says so for that unit and that many more, and the rest written again, each weighed
/// in its turn. A run is never longer than `longest` repeats; a longer one is written as several.
fn plan<'a>(
    units: &[&'a str],
    longest: u64,
    run_pays: impl Fn(&str, u64) -> bool,
) -> Vec<Event<'a>> {
    let mut events = Vec::with_capacity(units.len());
    let mut at = 0;
    while at < units.len() {
        events.push(Event::Unit(units[at]));
        let same = units[at + 1..].iter().take_while(|&&unit| unit == units[at]).count();
        let same = same.min(longest as usize);
        match same >= 2 && run_pays(units[at], same as u64) {
            true => {
                events.push(Event::Run(same as u64));
                at += 1 + same;
            }
            false => at += 1,
        }
    }
    events
}

/// Spells every line of `text` with one code, learnt from it or kept from `kept`.
pub(crate) fn squeeze(
    text: &str,
    kept: Option<Kept<'_>>,
    index: bool,
    deadline: Deadline,
) -> Result<Squeezed, FoldError> {
    let lines: Vec<&str> = text.split('\n').collect();
    let code = match kept {
        Some((previous, its_index)) => {
            read_code(previous, its_index).map_err(FoldError::Previous)?
        }
        None => {
            let strings = lines.iter().copied().filter(|line| !line.is_empty());
            TextCode::learn_within(strings, index, deadline)?
        }
    };
    let mut body = Vec::with_capacity(lines.len());
    for (at, line) in lines.iter().enumerate() {
        deadline.check_at(at)?;
        body.push(match line.is_empty() {
            true => Line::Empty,
            false => {
                let mut writer = BitWriter::new();
                code.write(&mut writer, line);
                Line::Bits(writer.finish())
            }
        });
    }
    Ok(Squeezed { options: Vec::new(), tables: Tables::Text(code), body: Body::Lines(body) })
}

fn read_code(folded: &Folded<'_>, index: Option<&Index>) -> Result<TextCode, UnfoldError> {
    let sections = container::sections(&folded.tables);
    let symbols = sections.iter().find(|(name, _)| *name == "symbols");
    let symbols = symbols.ok_or(UnfoldError::Table { line: 1, problem: "no symbols".into() })?;
    TextCode::from_table(&symbols.1, index)
}

pub(crate) fn unsqueeze(
    folded: &Folded<'_>,
    index: Option<&Index>,
) -> Result<Vec<u8>, UnfoldError> {
    let code = read_code(folded, index)?;
    let mut out = Vec::new();
    for (at, line) in folded.body.iter().enumerate() {
        if at > 0 {
            out.push(b'\n');
        }
        if line.text.is_empty() {
            continue;
        }
        let bits = super::bits_of(folded, line)?;
        let mut reader = bits.reader();
        let text = code.read(&mut reader).filter(|_| reader.rest_is_zero());
        out.extend_from_slice(
            text.ok_or_else(|| super::corrupt(line, "these bits spell no line"))?.as_bytes(),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bits::Bits;

    fn spelled(code: &TextCode, text: &str) -> (usize, String) {
        let mut writer = BitWriter::new();
        code.write(&mut writer, text);
        let bits = writer.finish();
        (bits.len(), code.read(&mut bits.reader()).unwrap())
    }

    #[test]
    fn a_learnt_code_spells_its_strings_back() {
        let strings = [
            "hello world",
            "",
            "aaaaaaaa",
            "\u{1b}[38;2;1;2;3m██\u{1b}[0m",
            "\u{1b}[0m x \u{1b}[0m",
        ];
        for index in [false, true] {
            let code = TextCode::learn(strings.iter().copied(), index);
            for text in strings {
                assert_eq!(spelled(&code, text).1, text);
            }
        }
    }

    #[test]
    fn a_repeated_escape_costs_one_symbol_and_a_run_a_few_bits() {
        let colour = "\u{1b}[38;2;226;57;57m";
        let lines: Vec<String> = (0..6).map(|i| format!("{colour}{}", ['⣿', '⡇'][i % 2])).collect();
        let code = TextCode::learn(lines.iter().map(String::as_str), true);
        assert!(code.index.contains_key(colour), "an escape said six times is one symbol");
        let plain = TextCode::learn(lines.iter().map(String::as_str), false);
        assert!(!plain.index.contains_key(colour), "without an index, it is spelled out");
        let (long_run, _) = spelled(&code, &"⣿".repeat(500));
        assert!(long_run < 40, "500 alike take {long_run} bits");
    }

    #[test]
    fn a_common_word_is_one_symbol_but_never_part_of_a_longer_one() {
        let names: Vec<String> = (0..40).map(|i| format!("Farming Simulator {i}")).collect();
        let mut strings: Vec<&str> = names.iter().map(String::as_str).collect();
        strings.push("Simulators");
        let code = TextCode::learn(strings.iter().copied(), true);
        assert!(code.index.contains_key("Simulator") && code.index.contains_key("Farming"));
        let known = |token: &str| code.index.contains_key(token);
        assert_eq!(units("Farming Simulator 2", known), ["Farming", " ", "Simulator", " ", "2"]);
        assert_eq!(units("Simulators", known).len(), 10, "a longer word is spelled out");
        for text in strings {
            assert_eq!(spelled(&code, text).1, text);
        }
    }

    #[test]
    fn a_character_the_table_lacks_is_spelled_by_its_code_point() {
        let code = TextCode::learn(["abc"], false);
        assert_eq!(spelled(&code, "a白c").1, "a白c");
        let table: Vec<String> = code.table(None);
        let lines: Vec<FileLine<'_>> =
            table.iter().enumerate().map(|(at, text)| FileLine { number: at + 1, text }).collect();
        let read = TextCode::from_table(&lines, None).unwrap();
        assert_eq!(spelled(&read, "cab\u{1b}[0m").1, "cab\u{1b}[0m");
    }

    #[test]
    fn a_run_longer_than_one_run_may_be_is_written_as_several() {
        let code = TextCode::learn(["--"], false);
        let long = "-".repeat(3 * LONGEST_RUN as usize + 5);
        let (bits, back) = spelled(&code, &long);
        assert_eq!(back, long);
        assert!(bits < 120, "{bits} bits");
    }

    /// Every count code writes every count it can back as it was. The cheapest never takes more
    /// bits than Elias gamma, its own table counted: gamma itself for a few small counts, whose
    /// table would cost more than it saves, and low bits as they are for counts that are large.
    #[test]
    fn every_count_code_writes_its_counts_back_and_the_cheapest_suits_them() {
        let few: Vec<u64> = vec![0, 0, 1, 0, 2, 0, 5, 0, 0, 1, 0, 0];
        let small: Vec<u64> = (0..200).map(|i| [0, 0, 1, 0, 2, 0, 5][i % 7]).collect();
        let large: Vec<u64> = (0..200).map(|i| 300 + (i * 37 % 200)).collect();
        let mixed: Vec<u64> = (0..200).map(|i| (i * i % 500) as u64).collect();
        assert!(CountCode::cheapest(&few).same_as(&CountCode::GAMMA), "gamma for a few");
        assert!(CountCode::cheapest(&large).k > 0, "low bits as they are for large ones");
        assert!(CountCode::cheapest(&[]).same_as(&CountCode::GAMMA), "no counts at all");
        let size = |code: &CountCode, counts: &[u64]| {
            let table = code.buckets.as_ref().map_or(0, |(encoder, _)| {
                let buckets = encoder.lengths().len() as u64;
                gamma_bits(buckets) as u64 + LENGTH_BITS as u64 * buckets
            });
            table + counts.iter().map(|&count| code.bits(count).unwrap() as u64).sum::<u64>()
        };
        for counts in [&few, &small, &large, &mixed] {
            let cheapest = CountCode::cheapest(counts);
            assert!(size(&cheapest, counts) <= size(&CountCode::GAMMA, counts), "{counts:?}");
        }
        for counts in [&small, &large, &mixed] {
            let mut codes = vec![CountCode::cheapest(counts)];
            for k in 0..=MOST_LOW_BITS {
                codes.push(CountCode { k, buckets: None });
                let coded = CountCode::cheapest(counts);
                codes.push(CountCode { k, buckets: coded.buckets.clone() });
            }
            for code in codes {
                let fits: Vec<u64> = counts.iter().copied().filter(|&n| n <= code.most()).collect();
                let mut writer = BitWriter::new();
                for &count in &fits {
                    code.write(&mut writer, count);
                }
                let expected: u32 = fits.iter().map(|&count| code.bits(count).unwrap()).sum();
                assert_eq!(writer.len(), expected as usize, "bits() counts what write() writes");
                let bits = writer.finish();
                let mut reader = bits.reader();
                for &count in &fits {
                    assert_eq!(code.read(&mut reader), Some(count), "k {}", code.k);
                }
            }
        }
    }

    /// A unit whose code is a single bit costs less written three times than written once with
    /// `<run>` and a count; a fold into parts writes it so, and reads it back the same.
    #[test]
    fn a_run_of_a_unit_with_a_short_code_is_written_again_instead() {
        // Blocks in pairs, which no run takes, so that a block is most of what is spelled.
        let lines: Vec<String> = (0..40).map(|i| "██░".repeat(3 + i % 4)).collect();
        let strings: Vec<&str> = lines.iter().map(String::as_str).collect();
        let parts = TextCode::learn_for_parts(&strings, false);
        assert_eq!(parts.encoder.length(parts.index["█"]), 1, "the drawing is mostly blocks");
        let classic = TextCode::learn(strings.iter().copied(), false);
        let (parts_bits, back) = spelled(&parts, "███ ░");
        assert_eq!(back, "███ ░");
        let units = units("███ ░", |_| false);
        let planned = plan(&units, parts.longest_run(), |unit, same| parts.run_pays(unit, same));
        assert!(planned.iter().all(|event| matches!(event, Event::Unit(_))), "no run in it");
        assert!(parts_bits < spelled(&classic, "███ ░").0, "fewer bits than a run takes");
        for text in &strings {
            assert_eq!(spelled(&parts, text).1, *text);
        }
    }

    /// A compact table reads back as the code it was written from, an index's places and all,
    /// and spells every line the same.
    #[test]
    fn a_compact_table_reads_back_as_its_code() {
        let colour = "\u{1b}[38;2;226;57;57m";
        let lines: Vec<String> = (0..30)
            .map(|i| format!("{colour}⣿⣿⡇{}\u{1b}[0m Simulator {}", "⠀".repeat(i % 9), i % 4))
            .collect();
        let strings: Vec<&str> = lines.iter().map(String::as_str).collect();
        for index in [false, true] {
            let code = TextCode::learn_for_parts(&strings, index);
            let values = code.symbols();
            let kept = (!values.is_empty()).then(|| Index::new(values).unwrap());
            assert_eq!(kept.is_some(), index, "the escape and the word are worth indexing");
            let mut writer = BitWriter::new();
            code.write_compact(&mut writer, kept.as_ref());
            let bits = writer.finish();
            let (k, coded) = code.count_choices();
            let read = TextCode::read_compact(&mut bits.reader(), kept.as_ref(), k, coded).unwrap();
            assert!(read.same_code_as(&code));
            for text in &strings {
                assert_eq!(spelled(&read, text), spelled(&code, text));
            }
        }
    }

    /// What no compact table holds is refused, and says what is wrong: a code length of 0 or past
    /// the longest, a character past the last there is, places with no index, too few bits.
    #[test]
    fn a_compact_table_no_fold_writes_is_refused() {
        let table = |write: &dyn Fn(&mut BitWriter)| {
            let mut writer = BitWriter::new();
            write(&mut writer);
            writer.finish()
        };
        let cases: Vec<(Bits, &str)> = vec![
            (table(&|w| w.write(0, 5)), "cut short"),
            (
                table(&|w| {
                    w.gamma(1);
                    w.gamma(1);
                    w.write(0, 15)
                }),
                "bits long",
            ),
            (
                table(&|w| {
                    w.gamma(1);
                    w.gamma(1);
                    w.write(31, 5);
                    w.write(1, 10)
                }),
                "bits long",
            ),
            (
                table(&|w| {
                    w.gamma(2);
                    w.gamma(1);
                    w.write(0b00010_00010_00001, 15);
                    w.gamma(0x11_0001);
                    w.write(1, 5)
                }),
                "past the last",
            ),
            (
                table(&|w| {
                    w.gamma(1);
                    w.gamma(2);
                    w.write(0b00010_00010_00001, 15);
                    w.gamma(1);
                    w.write(1, 5)
                }),
                "there is none",
            ),
            (
                table(&|w| {
                    w.gamma(1_000_000);
                    w.gamma(1)
                }),
                "more than it has bits for",
            ),
            (
                table(&|w| {
                    w.gamma(1);
                    w.gamma(1);
                    w.write(0b00001_00001_00001, 15)
                }),
                "no prefix code",
            ),
        ];
        for (bits, says) in cases {
            let refused = TextCode::read_compact(&mut bits.reader(), None, 0, false).err();
            assert!(
                refused.as_deref().is_some_and(|why| why.contains(says)),
                "{says}: {refused:?}"
            );
        }
    }

    #[test]
    fn a_damaged_run_is_refused_rather_than_obeyed() {
        let code = TextCode::learn(["aaaa", "b"], false);
        let mut writer = BitWriter::new();
        code.encoder.write(&mut writer, code.index["a"]);
        code.encoder.write(&mut writer, code.run.unwrap());
        writer.gamma(1 << 40);
        let bits = writer.finish();
        assert_eq!(code.read(&mut bits.reader()), None);
    }
}
