//! The table squeeze, for text in columns, like catalogames' listing of Steam's catalogue: a row a
//! line, and each column in a model of its own, chosen for it and shared by the whole file.
//!
//! A column's cells are coded one of a few ways, whichever costs the fewest bytes, tables included:
//!
//! - **Values:** the cells that repeat, as a Huffman-coded word list: the README's "words from a
//!   fixed list", learnt from the file. A cell off the list is coded by the column's fallback.
//! - **Fallbacks:** `spell`, the cell spelled out in a character code the file shares; `number`,
//!   for counts and ids, as its bit length and then its bits; or `delta`, the same for how far it
//!   is from the number above it, which suits a sorted column (an edit then changes its row and the
//!   next one).
//! - **Lists:** a cell of items joined by `,` or `;` is coded an item at a time, so that tag lists
//!   share their tags.
//!
//! Every list also has `<esc>`, so that a fold can keep an older fold's tables: a value they don't
//! hold then falls back instead of rebuilding them, and every row that didn't change folds to the
//! same line it did before.
//!
//! Lines that aren't rows (comments, lines with another number of cells) are kept as they are,
//! readable, behind a space, which no alphabet has.

use super::text::TextCode;
use super::{Body, Line, Squeezed};
use crate::bits::{BitReader, BitWriter};
use crate::container::{self, Folded, Line as FileLine};
use crate::error::{FoldError, UnfoldError};
use crate::escape::{escape, unescape};
use crate::huffman::{self, Decoder, Encoder};
use std::collections::{HashMap, HashSet};

/// How a table squeeze reads its input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableOptions {
    /// What separates a row's cells: a tab by default.
    pub separator: char,
    /// Lines starting with this are kept as they are instead of folded, as a table's comments
    /// are: `#` by default.
    pub keep: Option<String>,
}

impl Default for TableOptions {
    fn default() -> Self {
        Self { separator: '\t', keep: Some("#".to_string()) }
    }
}

/// Number codes: bit lengths 0 to 64, then [`SPELL`].
const CLASSES: usize = 66;
/// The number code for "not a number here: spelled".
const SPELL: usize = 65;
/// What a body bit costs in bytes, the alphabet not yet known: a base64 character holds 6.
const BYTES_PER_BIT: f64 = 1.0 / 6.0;
/// What a spelled character is guessed to cost, before the spelling is learnt.
const SPELL_BITS: f64 = 5.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Spell,
    Number,
    Delta,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Spell => "spell",
            Kind::Number => "number",
            Kind::Delta => "delta",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Esc,
    End,
    Value(String),
}

#[derive(Debug)]
struct Values {
    entries: Vec<Entry>,
    index: HashMap<String, usize>,
    encoder: Encoder,
    decoder: Decoder,
    esc: usize,
    end: Option<usize>,
}

impl Values {
    fn new(entries: Vec<Entry>, lengths: Vec<u32>) -> Option<Self> {
        let decoder = Decoder::new(&lengths)?;
        let encoder = Encoder::new(lengths)?;
        let esc = entries.iter().position(|entry| *entry == Entry::Esc)?;
        let end = entries.iter().position(|entry| *entry == Entry::End);
        let index = entries
            .iter()
            .enumerate()
            .filter_map(|(at, entry)| match entry {
                Entry::Value(value) => Some((value.clone(), at)),
                _ => None,
            })
            .collect();
        Some(Self { entries, index, encoder, decoder, esc, end })
    }
}

#[derive(Debug)]
struct Numbers {
    encoder: Encoder,
    decoder: Decoder,
}

enum Class {
    Number(u64),
    Spell,
}

impl Numbers {
    fn new(lengths: Vec<u32>) -> Option<Self> {
        (lengths.len() == CLASSES).then_some(())?;
        Some(Self { decoder: Decoder::new(&lengths)?, encoder: Encoder::new(lengths)? })
    }

    fn write(&self, writer: &mut BitWriter, number: u64) {
        let class = bit_length(number);
        self.encoder.write(writer, class);
        if class >= 2 {
            writer.write(number, class as u32 - 1);
        }
    }

    fn read(&self, reader: &mut BitReader<'_>) -> Option<Class> {
        Some(match self.decoder.read(reader)? {
            SPELL => Class::Spell,
            0 => Class::Number(0),
            class => Class::Number((1 << (class - 1)) | reader.read(class as u32 - 1)?),
        })
    }
}

#[derive(Debug)]
struct Column {
    list: Option<char>,
    values: Option<Values>,
    kind: Kind,
    numbers: Option<Numbers>,
}

/// What reading a unit found: a value, or a list's end.
enum Read {
    Unit(String),
    End,
}

impl Column {
    fn write_cell(&self, writer: &mut BitWriter, cell: &str, above: &mut u64, spell: &TextCode) {
        match self.list {
            None => self.write_unit(writer, cell, above, spell),
            Some(separator) => {
                let mut before = 0;
                for item in units(cell, Some(separator)) {
                    self.write_unit(writer, item, &mut before, spell);
                }
                let values = self.values.as_ref().expect("a list has values");
                values.encoder.write(writer, values.end.expect("a list's values have <end>"));
            }
        }
    }

    fn write_unit(&self, writer: &mut BitWriter, unit: &str, before: &mut u64, spell: &TextCode) {
        if let Some(values) = &self.values {
            if let Some(&entry) = values.index.get(unit) {
                values.encoder.write(writer, entry);
                track(unit, before);
                return;
            }
            values.encoder.write(writer, values.esc);
        }
        match &self.numbers {
            None => spell.write(writer, unit),
            Some(numbers) => match numeric(unit, self.kind, *before) {
                Some(number) => numbers.write(writer, number),
                None => {
                    numbers.encoder.write(writer, SPELL);
                    spell.write(writer, unit);
                }
            },
        }
        track(unit, before);
    }

    fn read_cell(
        &self,
        reader: &mut BitReader<'_>,
        above: &mut u64,
        spell: &TextCode,
    ) -> Option<String> {
        match self.list {
            None => match self.read_unit(reader, above, spell)? {
                Read::Unit(unit) => Some(unit),
                Read::End => None,
            },
            Some(separator) => {
                let (mut items, mut before) = (Vec::new(), 0);
                while let Read::Unit(item) = self.read_unit(reader, &mut before, spell)? {
                    items.push(item);
                }
                Some(items.join(&separator.to_string()))
            }
        }
    }

    fn read_unit(
        &self,
        reader: &mut BitReader<'_>,
        before: &mut u64,
        spell: &TextCode,
    ) -> Option<Read> {
        if let Some(values) = &self.values {
            match &values.entries[values.decoder.read(reader)?] {
                Entry::Value(value) => {
                    track(value, before);
                    return Some(Read::Unit(value.clone()));
                }
                Entry::End => return Some(Read::End),
                Entry::Esc => {}
            }
        }
        let unit = match &self.numbers {
            None => spell.read(reader)?,
            Some(numbers) => match numbers.read(reader)? {
                Class::Spell => spell.read(reader)?,
                Class::Number(number) if self.kind == Kind::Delta => {
                    unzigzag(number, *before)?.to_string()
                }
                Class::Number(number) => number.to_string(),
            },
        };
        track(&unit, before);
        Some(Read::Unit(unit))
    }

    /// The units of `cell` this column spells out, as [`Column::write_cell`] would.
    fn spelled<'a>(&self, cell: &'a str, above: &mut u64, out: &mut Vec<&'a str>) {
        let mut own = 0;
        let before = match self.list {
            Some(_) => &mut own,
            None => above,
        };
        for unit in units(cell, self.list) {
            let listed = self.values.as_ref().is_some_and(|values| values.index.contains_key(unit));
            if !listed && (self.numbers.is_none() || numeric(unit, self.kind, *before).is_none()) {
                out.push(unit);
            }
            track(unit, before);
        }
    }

    fn describe(&self) -> String {
        let mut words = Vec::new();
        if let Some(separator) = self.list {
            words.extend(["list".to_string(), escape(&separator.to_string())]);
        }
        if self.values.is_some() {
            words.push("values".to_string());
        }
        words.push(self.kind.name().to_string());
        words.join("\t")
    }
}

/// The number a unit is, as a column of `kind` writes it: itself, or its distance from `before`.
fn numeric(unit: &str, kind: Kind, before: u64) -> Option<u64> {
    let number = number(unit)?;
    match kind {
        Kind::Delta => zigzag(number, before),
        _ => Some(number),
    }
}

/// `text` as a number, if it is one spelled the one way numbers are: digits, no leading zero.
fn number(text: &str) -> Option<u64> {
    let digits = text.as_bytes();
    let canonical = !digits.is_empty()
        && digits.len() <= 20
        && digits.iter().all(u8::is_ascii_digit)
        && (digits.len() == 1 || digits[0] != b'0');
    canonical.then(|| text.parse().ok()).flatten()
}

fn track(unit: &str, before: &mut u64) {
    if let Some(number) = number(unit) {
        *before = number;
    }
}

/// How far `number` is from `before`, folded so small distances either way are small numbers.
fn zigzag(number: u64, before: u64) -> Option<u64> {
    let distance = number as i128 - before as i128;
    u64::try_from(match distance >= 0 {
        true => distance * 2,
        false => -distance * 2 - 1,
    })
    .ok()
}

fn unzigzag(folded: u64, before: u64) -> Option<u64> {
    let distance = match folded % 2 {
        0 => (folded / 2) as i128,
        _ => -((folded / 2) as i128) - 1,
    };
    u64::try_from(before as i128 + distance).ok()
}

fn bit_length(number: u64) -> usize {
    (64 - number.leading_zeros()) as usize
}

/// A cell's units: the cell itself, or its items, none for an empty cell.
fn units(cell: &str, list: Option<char>) -> impl Iterator<Item = &str> {
    let (whole, items) = match list {
        None => (Some(cell), None),
        Some(_) if cell.is_empty() => (None, None),
        Some(separator) => (None, Some(cell.split(separator))),
    };
    whole.into_iter().chain(items.into_iter().flatten())
}

/// A column model, chosen and counted, before it is built.
struct Plan {
    list: Option<char>,
    kind: Kind,
    dictionary: Vec<(String, u64)>,
    misses: u64,
    ends: u64,
    classes: [u64; CLASSES],
}

/// The cheapest model for these cells, in row order.
fn plan(cells: &[&str]) -> Plan {
    let mut lists = vec![None];
    lists.extend(
        [',', ';'].into_iter().filter(|&s| cells.iter().any(|cell| cell.contains(s))).map(Some),
    );
    let mut best: Option<(f64, Plan)> = None;
    for list in lists {
        let (mut counts, mut total, mut numbers) = (HashMap::<&str, u64>::new(), 0u64, 0u64);
        for &cell in cells {
            for unit in units(cell, list) {
                *counts.entry(unit).or_default() += 1;
                total += 1;
                numbers += number(unit).is_some() as u64;
            }
        }
        let kinds = match total > 0 && numbers * 2 >= total {
            true => &[Kind::Spell, Kind::Number, Kind::Delta][..],
            false => &[Kind::Spell][..],
        };
        for &kind in kinds {
            let (cost, plan) = evaluate(cells, list, kind, &counts, total);
            if best.as_ref().is_none_or(|(least, _)| cost < *least) {
                best = Some((cost, plan));
            }
        }
    }
    best.expect("there is always the plain model").1
}

/// What a model would cost these cells, in bytes, and the model.
fn evaluate(
    cells: &[&str],
    list: Option<char>,
    kind: Kind,
    counts: &HashMap<&str, u64>,
    total: u64,
) -> (f64, Plan) {
    let mut dictionary: Vec<(String, u64)> = counts
        .iter()
        .filter(|&(&unit, &count)| count >= 2 && worth_listing(unit, count, total, kind))
        .map(|(&unit, &count)| (unit.to_string(), count))
        .collect();
    dictionary.sort_unstable();
    let listed: HashSet<&str> = dictionary.iter().map(|(unit, _)| unit.as_str()).collect();
    let (mut misses, mut bits, mut classes) = (0u64, 0f64, [0u64; CLASSES]);
    let mut above = 0u64;
    for &cell in cells {
        let mut own = 0u64;
        let before = match list {
            Some(_) => &mut own,
            None => &mut above,
        };
        for unit in units(cell, list) {
            if !listed.contains(unit) {
                misses += 1;
                match (kind, numeric(unit, kind, *before)) {
                    (Kind::Spell, _) => bits += spell_bits(unit),
                    (_, Some(number)) => {
                        classes[bit_length(number)] += 1;
                        bits += bit_length(number).saturating_sub(1) as f64;
                    }
                    (_, None) => {
                        classes[SPELL] += 1;
                        bits += spell_bits(unit);
                    }
                }
            }
            track(unit, before);
        }
    }
    let ends = if list.is_some() { cells.len() as u64 } else { 0 };
    if list.is_some() || !dictionary.is_empty() {
        let mut weights = vec![misses + 1];
        weights.extend(list.map(|_| ends.max(1)));
        weights.extend(dictionary.iter().map(|&(_, count)| count));
        bits += coded_bits(&weights);
    }
    if kind != Kind::Spell {
        bits += coded_bits(&classes.map(|count| count + 1));
    }
    let entries: f64 = dictionary.iter().map(|(unit, _)| escape(unit).len() as f64 + 4.0).sum();
    let header = entries + if kind == Kind::Spell { 0.0 } else { 200.0 } + 30.0;
    (bits * BYTES_PER_BIT + header, Plan { list, kind, dictionary, misses, ends, classes })
}

/// Whether listing `unit` saves more body than its table line costs.
fn worth_listing(unit: &str, count: u64, total: u64, kind: Kind) -> bool {
    let fallback = match (kind, number(unit)) {
        (Kind::Spell, _) | (_, None) => spell_bits(unit),
        (_, Some(number)) => 4.0 + bit_length(number).saturating_sub(1) as f64,
    };
    let listed = (total as f64 / count as f64).log2() + 1.0;
    count as f64 * (fallback - listed) * BYTES_PER_BIT > escape(unit).len() as f64 + 4.0
}

fn spell_bits(unit: &str) -> f64 {
    (unit.chars().count() + 1) as f64 * SPELL_BITS
}

/// The bits a Huffman code for these counts spends on them.
fn coded_bits(counts: &[u64]) -> f64 {
    let lengths = huffman::lengths(counts);
    counts.iter().zip(&lengths).map(|(&count, &length)| count as f64 * length as f64).sum()
}

fn build(plan: Plan) -> Column {
    let values = (plan.list.is_some() || !plan.dictionary.is_empty()).then(|| {
        let mut entries = vec![Entry::Esc];
        let mut weights = vec![plan.misses + 1];
        if plan.list.is_some() {
            entries.push(Entry::End);
            weights.push(plan.ends.max(1));
        }
        for (value, count) in plan.dictionary {
            entries.push(Entry::Value(value));
            weights.push(count);
        }
        Values::new(entries, huffman::lengths(&weights))
            .expect("Huffman lengths make a prefix code")
    });
    let numbers = (plan.kind != Kind::Spell).then(|| {
        Numbers::new(huffman::lengths(&plan.classes.map(|count| count + 1))).expect("a prefix code")
    });
    Column { list: plan.list, values, kind: plan.kind, numbers }
}

/// A table's models: how its rows split, how each column is coded, and the spelling they share.
#[derive(Debug)]
struct Table {
    separator: char,
    keep: Option<String>,
    columns: Vec<Column>,
    spell: TextCode,
}

impl Table {
    /// The cells of `line`, if it is a row.
    fn row<'a>(&self, line: &'a str) -> Option<Vec<&'a str>> {
        row(line, self.separator, self.keep.as_deref(), self.columns.len())
    }

    fn learn(lines: &[&str], options: &TableOptions) -> Result<Self, FoldError> {
        let keep = options.keep.as_deref();
        let mut widths: HashMap<usize, usize> = HashMap::new();
        for line in lines
            .iter()
            .filter(|line| !line.is_empty() && !keep.is_some_and(|k| line.starts_with(k)))
        {
            *widths.entry(line.split(options.separator).count()).or_default() += 1;
        }
        let width = widths
            .into_iter()
            .max_by_key(|&(width, rows)| (rows, width))
            .ok_or(FoldError::NotATable)?
            .0;
        let rows: Vec<Vec<&str>> =
            lines.iter().filter_map(|line| row(line, options.separator, keep, width)).collect();
        let columns: Vec<Column> = (0..width)
            .map(|at| build(plan(&rows.iter().map(|row| row[at]).collect::<Vec<_>>())))
            .collect();
        let mut spelled = Vec::new();
        let mut above = vec![0u64; width];
        for row in &rows {
            for (at, cell) in row.iter().enumerate() {
                columns[at].spelled(cell, &mut above[at], &mut spelled);
            }
        }
        let spell = TextCode::learn(spelled.iter().copied());
        Ok(Self { separator: options.separator, keep: options.keep.clone(), columns, spell })
    }

    fn encode(&self, lines: &[&str]) -> Vec<Line> {
        let mut above = vec![0u64; self.columns.len()];
        lines
            .iter()
            .map(|line| {
                if line.is_empty() {
                    return Line::Empty;
                }
                let Some(cells) = self.row(line) else {
                    return Line::Kept(escape(line));
                };
                let mut writer = BitWriter::new();
                for (at, cell) in cells.iter().enumerate() {
                    self.columns[at].write_cell(&mut writer, cell, &mut above[at], &self.spell);
                }
                Line::Bits(writer.finish())
            })
            .collect()
    }

    fn header(&self) -> Vec<String> {
        let mut out = vec![
            "=== table ===".to_string(),
            format!("separator\t{}", escape(&self.separator.to_string())),
        ];
        out.extend(self.keep.iter().map(|keep| format!("keep\t{}", escape(keep))));
        for (at, column) in self.columns.iter().enumerate() {
            out.push(format!("column\t{}\t{}", at + 1, column.describe()));
        }
        for (at, column) in self.columns.iter().enumerate() {
            if let Some(numbers) = &column.numbers {
                out.push(format!("=== column {} numbers ===", at + 1));
                out.push(
                    numbers
                        .encoder
                        .lengths()
                        .iter()
                        .map(u32::to_string)
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
            if let Some(values) = &column.values {
                out.push(format!("=== column {} values ===", at + 1));
                for (entry, length) in values.entries.iter().zip(values.encoder.lengths()) {
                    out.push(match entry {
                        Entry::Esc => format!("{length}\t<esc>"),
                        Entry::End => format!("{length}\t<end>"),
                        Entry::Value(value) => format!("{length}\t{}", escape(value)),
                    });
                }
            }
        }
        out.push("=== spell ===".to_string());
        out.extend(self.spell.table());
        out
    }

    fn read(folded: &Folded<'_>) -> Result<Self, UnfoldError> {
        let sections = container::sections(&folded.tables);
        let section =
            |name: &str| sections.iter().find(|(found, _)| *found == name).map(|(_, lines)| lines);
        let first = folded.tables.first().map_or(1, |line| line.number);
        let missing = |what: &str| UnfoldError::Table {
            line: first,
            problem: format!("no `{what}` section"),
        };
        let (mut separator, mut keep, mut descriptions) = (None, None, Vec::new());
        for line in section("table").ok_or_else(|| missing("table"))? {
            let problem = |problem: String| UnfoldError::Table { line: line.number, problem };
            let words: Vec<&str> = line.text.split('\t').collect();
            match words[..] {
                ["separator", text] => {
                    separator = Some(
                        single(&unescape(text).map_err(problem)?)
                            .ok_or_else(|| problem("a separator is one character".into()))?,
                    )
                }
                ["keep", text] => keep = Some(unescape(text).map_err(problem)?),
                ["column", number, ref model @ ..] => {
                    if number.parse() != Ok(descriptions.len() + 1) {
                        return Err(problem(format!("column {number} is out of order")));
                    }
                    descriptions.push((
                        line,
                        describe(model).ok_or_else(|| {
                            problem("a column model this orimoji can't read".into())
                        })?,
                    ));
                }
                _ => return Err(problem(format!("`{}` means nothing in a table", line.text))),
            }
        }
        let separator = separator.ok_or_else(|| missing("separator"))?;
        let mut columns = Vec::with_capacity(descriptions.len());
        for (at, (line, (list, has_values, kind))) in descriptions.into_iter().enumerate() {
            let problem = |problem: &str| UnfoldError::Table {
                line: line.number,
                problem: problem.to_string(),
            };
            let values = match has_values {
                false => None,
                true => {
                    let name = format!("column {} values", at + 1);
                    Some(read_values(
                        section(&name).ok_or_else(|| missing(&name))?,
                        list.is_some(),
                    )?)
                }
            };
            if list.is_some() && values.is_none() {
                return Err(problem("a list column needs values, for its <end>"));
            }
            let numbers = match kind {
                Kind::Spell => None,
                _ => {
                    let name = format!("column {} numbers", at + 1);
                    let lines = section(&name).ok_or_else(|| missing(&name))?;
                    let lengths: Option<Vec<u32>> = lines.first().and_then(|line| {
                        line.text.split(' ').map(|length| length.parse().ok()).collect()
                    });
                    Some(
                        lengths
                            .and_then(Numbers::new)
                            .ok_or_else(|| problem("its numbers are no code"))?,
                    )
                }
            };
            columns.push(Column { list, values, kind, numbers });
        }
        let spell = TextCode::from_table(section("spell").ok_or_else(|| missing("spell"))?)?;
        Ok(Self { separator, keep, columns, spell })
    }
}

/// The cells of `line`, if it is a row of a table `width` wide.
fn row<'a>(
    line: &'a str,
    separator: char,
    keep: Option<&str>,
    width: usize,
) -> Option<Vec<&'a str>> {
    if line.is_empty() || keep.is_some_and(|keep| line.starts_with(keep)) {
        return None;
    }
    let cells: Vec<&str> = line.split(separator).collect();
    (cells.len() == width).then_some(cells)
}

fn single(text: &str) -> Option<char> {
    let mut chars = text.chars();
    chars.next().filter(|_| chars.next().is_none())
}

/// A column's model words: `[list <sep>] [values] spell|number|delta`.
fn describe(words: &[&str]) -> Option<(Option<char>, bool, Kind)> {
    let (list, words) = match words {
        ["list", separator, rest @ ..] => (Some(single(&unescape(separator).ok()?)?), rest),
        _ => (None, words),
    };
    let (has_values, words) = match words {
        ["values", rest @ ..] => (true, rest),
        _ => (false, words),
    };
    let kind = match words {
        ["spell"] => Kind::Spell,
        ["number"] => Kind::Number,
        ["delta"] => Kind::Delta,
        _ => return None,
    };
    Some((list, has_values, kind))
}

fn read_values(lines: &[FileLine<'_>], list: bool) -> Result<Values, UnfoldError> {
    let (mut entries, mut lengths) =
        (Vec::with_capacity(lines.len()), Vec::with_capacity(lines.len()));
    for line in lines {
        let problem = |problem: String| UnfoldError::Table { line: line.number, problem };
        let (length, text) = line.text.split_once('\t').ok_or_else(|| problem("no tab".into()))?;
        lengths.push(length.parse().map_err(|_| problem(format!("`{length}` is no code length")))?);
        entries.push(match text {
            "<esc>" => Entry::Esc,
            "<end>" => Entry::End,
            _ => Entry::Value(unescape(text).map_err(problem)?),
        });
    }
    let line = lines.first().map_or(1, |line| line.number);
    let values = Values::new(entries, lengths).ok_or_else(|| UnfoldError::Table {
        line,
        problem: "these values are no code with an <esc>".into(),
    })?;
    match list && values.end.is_none() {
        true => Err(UnfoldError::Table { line, problem: "a list's values need an <end>".into() }),
        false => Ok(values),
    }
}

pub(crate) fn squeeze(
    text: &str,
    options: &TableOptions,
    kept: Option<&Folded<'_>>,
) -> Result<Squeezed, FoldError> {
    let lines: Vec<&str> = text.split('\n').collect();
    let table = match kept {
        Some(previous) => Table::read(previous).map_err(FoldError::Previous)?,
        None => Table::learn(&lines, options)?,
    };
    Ok(Squeezed {
        options: Vec::new(),
        tables: table.header(),
        body: Body::Lines(table.encode(&lines)),
    })
}

pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    let table = Table::read(folded)?;
    let separator = table.separator.to_string();
    let mut above = vec![0u64; table.columns.len()];
    let mut out = String::new();
    for (at, line) in folded.body.iter().enumerate() {
        if at > 0 {
            out.push('\n');
        }
        if line.text.is_empty() {
            continue;
        }
        if let Some(kept) = line.text.strip_prefix(' ') {
            out.push_str(&unescape(kept).map_err(|problem| super::corrupt(line, &problem))?);
            continue;
        }
        let bits = super::bits_of(folded, line)?;
        let mut reader = bits.reader();
        for (column_at, column) in table.columns.iter().enumerate() {
            if column_at > 0 {
                out.push_str(&separator);
            }
            let cell = column.read_cell(&mut reader, &mut above[column_at], &table.spell);
            out.push_str(&cell.ok_or_else(|| {
                super::corrupt(line, &format!("column {} doesn't decode", column_at + 1))
            })?);
        }
        if !reader.rest_is_zero() {
            return Err(super::corrupt(line, "bits are left over after the row"));
        }
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_distances_read_back_as_written() {
        for (number, before) in
            [(0, 0), (5, 9), (1 << 62, 0), (0, 1 << 62), (1000, 999), (u64::MAX, u64::MAX - 3)]
        {
            assert_eq!(zigzag(number, before).and_then(|z| unzigzag(z, before)), Some(number));
        }
        assert_eq!(zigzag(u64::MAX, 0), None, "too far to fold: the cell is spelled instead");
        assert_eq!(number("0"), Some(0));
        assert_eq!(number("007"), None, "a leading zero would come back without it");
        assert_eq!(number("12a"), None);
        assert_eq!(number("99999999999999999999"), None, "past u64");
    }

    #[test]
    fn a_column_of_few_values_gets_a_list_of_them() {
        let cells: Vec<&str> =
            (0..500).map(|i| ["Valve", "Gearbox Software", "?"][i % 3]).collect();
        let plan = plan(&cells);
        assert_eq!(plan.kind, Kind::Spell);
        assert_eq!(plan.dictionary.len(), 3);
    }

    #[test]
    fn a_sorted_column_of_ids_is_coded_by_distance() {
        let cells: Vec<String> = (0..2000u64).map(|i| (10 + i * 10 + i % 7).to_string()).collect();
        let cells: Vec<&str> = cells.iter().map(String::as_str).collect();
        assert_eq!(plan(&cells).kind, Kind::Delta);
    }

    #[test]
    fn a_column_of_comma_lists_is_coded_by_item() {
        // Tag lists picked at random: whole cells hardly repeat, their tags do.
        let tags: Vec<String> = (0..20).map(|tag| format!("{tag:x}a")).collect();
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut cells = Vec::new();
        for _ in 0..400 {
            let mut picked = Vec::new();
            for tag in &tags {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                if x % 4 == 0 {
                    picked.push(tag.as_str());
                }
            }
            cells.push(picked.join(","));
        }
        let cells: Vec<&str> = cells.iter().map(String::as_str).collect();
        let plan = plan(&cells);
        assert_eq!(plan.list, Some(','));
        assert!(plan.dictionary.len() >= 15);
    }
}
