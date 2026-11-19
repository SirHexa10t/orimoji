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

use super::{Body, Kept, Line, Squeezed, Tables};
use crate::bits::{BitReader, BitWriter};
use crate::container::{self, Folded, Line as FileLine};
use crate::deadline::Deadline;
use crate::error::{FoldError, UnfoldError};
use crate::escape::escape;
use crate::huffman::{self, Decoder, Encoder};
use crate::index::{Index, read_entry, write_entry};
use std::collections::{HashMap, HashSet};

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
}

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
        let mut tokens: HashMap<&str, u64> = HashMap::new();
        if index {
            for (at, string) in strings.clone().enumerate() {
                deadline.check_at(at)?;
                let words =
                    string.split(|c: char| !wordy(c)).filter(|word| word.chars().nth(1).is_some());
                for token in csi_tokens(string).chain(words) {
                    *tokens.entry(token).or_default() += 1;
                }
            }
        }
        let known: HashSet<&str> = tokens
            .into_iter()
            .filter(|&(token, count)| worth_a_symbol(token, count))
            .map(|(token, _)| token)
            .collect();
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
        Some(Self { symbols, index, encoder, decoder, end, esc, run })
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
        for event in events(&units(text, known)) {
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
                    writer.gamma(repeats - 1);
                }
            }
        }
        self.encoder.write(writer, self.end);
    }

    /// What [`TextCode::write`] spelled, or `None` where the bits don't spell it.
    pub(crate) fn read(&self, reader: &mut BitReader<'_>) -> Option<String> {
        let mut out = String::new();
        let mut last: Option<(usize, usize)> = None;
        loop {
            let symbol = self.decoder.read(reader)?;
            match &self.symbols[symbol] {
                Symbol::End => return Some(out),
                Symbol::Text(text) => {
                    last = Some((out.len(), out.len() + text.len()));
                    out.push_str(text);
                }
                Symbol::Esc => {
                    let c = char::from_u32(u32::try_from(reader.gamma()? - 1).ok()?)?;
                    last = Some((out.len(), out.len() + c.len_utf8()));
                    out.push(c);
                }
                Symbol::Run => {
                    let repeats = reader.gamma()? + 1;
                    let (start, end) = last?;
                    if repeats > LONGEST_RUN {
                        return None;
                    }
                    let unit = out[start..end].to_string();
                    for _ in 0..repeats {
                        out.push_str(&unit);
                    }
                }
            }
        }
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
    let mut events = Vec::with_capacity(units.len());
    let mut at = 0;
    while at < units.len() {
        events.push(Event::Unit(units[at]));
        let same = units[at + 1..].iter().take_while(|&&unit| unit == units[at]).count();
        let same = same.min(LONGEST_RUN as usize);
        match same >= 2 {
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
