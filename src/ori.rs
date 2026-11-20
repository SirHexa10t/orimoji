//! Files that keep a fold's unit with its lines, so that what they need to unfold is never lost:
//! for the text squeeze, which folds into [`parts`] (the other squeezes keep their index inside
//! their folded files instead).
//!
//! A text folded alone is one `.ori`, its unit inside it:
//!
//! ```text
//! === orimoji 3 text base64url index=2 ===     the format, the squeeze, the alphabet, and how many
//! === unit ===                                 values the unit's index holds, when it has one
//! IgHKTHHM                                     the choices
//! CsqhghG1A-RVVPO00t…                          the code table
//! God                                          the index, a value a line
//! and
//! === body ===
//! _8wTxNYlmw                                   a line for each line of the text
//! ```
//!
//! Texts folded together share one unit, kept once in an `.orinv`, its header and its unit and no
//! body. Each one's `.ori` keeps only its lines, and names the `.orinv`, as a path from its own
//! folder, and the fingerprint of the unit in it, so that it is never unfolded with another:
//!
//! ```text
//! === orimoji 3 text base64url orinv=chapters.orinv unit=3f2a91c0 ===
//! === body ===
//! _8wTxNYlmw
//! ```
//!
//! On the command line, and in [`crate::files`], a unit is only ever given as a file.

use crate::alphabet::Alphabet;
use crate::container::{self, Header};
use crate::error::UnfoldError;
use crate::escape::{escape, unescape};
use crate::parts::{self, Overheads, Unit};

/// The suffix a text squeeze's folded file takes beside the file it was folded from.
pub const SUFFIX: &str = ".ori";

/// The suffix of a file holding the unit that files folded together share.
pub const UNIT_SUFFIX: &str = ".orinv";

/// The format an `.ori` or an `.orinv` is in: the first to keep a unit with its lines. (Format 2
/// is a folded file keeping its index inside it.)
const VERSION: u32 = 3;

/// The squeeze they are of.
pub(crate) const SQUEEZE: &str = "text";

const UNIT: &str = "=== unit ===";
const BODY: &str = "=== body ===";

/// An `.ori` or an `.orinv`, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OriFile {
    /// A text folded alone: its lines, and the unit inside the file.
    Whole { alphabet: Alphabet, unit: Unit, lines: Vec<String> },
    /// The lines of a text folded together with others: the `.orinv` holding their unit, as a
    /// path from the file's own folder, and the fingerprint of that unit.
    Apart { alphabet: Alphabet, orinv: String, fingerprint: u32, lines: Vec<String> },
    /// A unit alone: an `.orinv`.
    Unit { alphabet: Alphabet, unit: Unit },
}

/// The text of an `.ori` holding `lines` and the `unit` they unfold with.
pub fn whole(lines: &[String], unit: &Unit, alphabet: Alphabet) -> String {
    let mut out = header_with_unit(unit, alphabet);
    out.push_str(BODY);
    out.push('\n');
    push_lines(&mut out, lines);
    out
}

/// The text of an `.ori` holding `lines` folded together with other files', whose `unit` is in
/// the `.orinv` at `orinv`, a path from the `.ori`'s own folder.
pub fn apart(lines: &[String], orinv: &str, unit: &Unit, alphabet: Alphabet) -> String {
    // The header's options are words: a space in the path is written as the escape of one.
    let orinv = escape(orinv).replace(' ', "\\u{20}");
    let options = vec![format!("orinv={orinv}"), format!("unit={:08x}", unit.fingerprint())];
    let header = Header { version: VERSION, squeeze: SQUEEZE.to_string(), alphabet, options };
    let mut out = header.line();
    out.push('\n');
    out.push_str(BODY);
    out.push('\n');
    push_lines(&mut out, lines);
    out
}

/// The text of an `.orinv` holding `unit`.
pub fn unit_file(unit: &Unit, alphabet: Alphabet) -> String {
    header_with_unit(unit, alphabet)
}

/// What `.ori` files cost besides their lines and units, for a grouped fold to weigh: a file
/// alone, its header and markers; a file that shares, its header naming the `.orinv` at `orinv`
/// and the unit's fingerprint, and its marker; and the `.orinv`'s header and marker, once. An
/// index's count in a header isn't counted.
pub(crate) fn overheads(alphabet: Alphabet, orinv: &str) -> Overheads {
    let header = |options: Vec<String>| {
        Header { version: VERSION, squeeze: SQUEEZE.to_string(), alphabet, options }.line().len()
            + 1
    };
    let marker = |line: &str| line.len() + 1;
    let named = escape(orinv).replace(' ', "\\u{20}");
    Overheads {
        alone: header(Vec::new()) + marker(UNIT) + marker(BODY),
        sharing: header(vec![format!("orinv={named}"), format!("unit={:08x}", 0)]) + marker(BODY),
        shared_unit: header(Vec::new()) + marker(UNIT),
    }
}

/// The header, then the unit section.
fn header_with_unit(unit: &Unit, alphabet: Alphabet) -> String {
    let options =
        unit.index.iter().map(|index| format!("index={}", index.len())).collect::<Vec<_>>();
    let header = Header { version: VERSION, squeeze: SQUEEZE.to_string(), alphabet, options };
    format!("{}\n{UNIT}\n{}", header.line(), unit.to_text())
}

/// Each line, then a newline: so the file ends in one, and an empty text is one empty line.
fn push_lines(out: &mut String, lines: &[String]) {
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

/// The `.orinv` a file of lines apart names, as a path from its folder, and the fingerprint of
/// the unit in it: from the file's first line alone, which is all a search through a folder reads.
pub(crate) fn names(first: &str) -> Option<(String, u32)> {
    let header = container::header_of(first).ok()?;
    if header.version != VERSION || header.squeeze != SQUEEZE {
        return None;
    }
    let orinv = unescape(header.value("orinv")?).ok()?;
    let print = header.value("unit")?;
    let fingerprint = u32::from_str_radix(print, 16).ok().filter(|_| print.len() == 8)?;
    Some((orinv, fingerprint))
}

/// Whether `text` is an `.ori` or an `.orinv`, by its first line.
pub fn is_ori(text: &str) -> bool {
    container::header_of(text)
        .is_ok_and(|header| header.version == VERSION && header.squeeze == SQUEEZE)
}

/// What an `.ori` or an `.orinv` holds, or refused, by line, where it holds what no writer
/// writes.
pub fn read(text: &str) -> Result<OriFile, UnfoldError> {
    let header = container::header_of(text)?;
    if header.version != VERSION || header.squeeze != SQUEEZE {
        return Err(UnfoldError::Header(format!(
            "format {} of the {} squeeze, where an .ori is format {VERSION} of text",
            header.version, header.squeeze
        )));
    }
    let alphabet = header.alphabet;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let mut lines = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).skip(1);
    let table =
        |number: usize, problem: &str| UnfoldError::Table { line: number, problem: problem.into() };
    let body = |lines: &mut dyn Iterator<Item = &str>| -> Vec<String> {
        let body: Vec<String> = lines.map(str::to_string).collect();
        match body.is_empty() {
            true => vec![String::new()],
            false => body,
        }
    };
    match (header.value("orinv"), header.value("unit")) {
        (Some(orinv), Some(print)) => {
            let orinv =
                unescape(orinv).map_err(|why| UnfoldError::Header(format!("orinv: {why}")))?;
            let fingerprint = u32::from_str_radix(print, 16).ok().filter(|_| print.len() == 8);
            let fingerprint = fingerprint.ok_or_else(|| {
                UnfoldError::Header(format!("`{print}` is no unit's fingerprint"))
            })?;
            if lines.next() != Some(BODY) {
                return Err(table(2, "the lines of a file folded with others start at once"));
            }
            Ok(OriFile::Apart { alphabet, orinv, fingerprint, lines: body(&mut lines) })
        }
        (None, None) => {
            if lines.next() != Some(UNIT) {
                return Err(table(2, "a unit, where the unit should be"));
            }
            let values: usize = match header.value("index") {
                None => 0,
                Some(count) => count.parse().map_err(|_| {
                    UnfoldError::Header(format!("`index={count}` counts no values"))
                })?,
            };
            let mut unit_text = String::new();
            for at in 0..2 + values {
                let line = lines.next().ok_or_else(|| table(3 + at, "the unit is cut short"))?;
                unit_text.push_str(line);
                unit_text.push('\n');
            }
            let unit = Unit::from_text(&unit_text)?;
            match lines.next() {
                None => Ok(OriFile::Unit { alphabet, unit }),
                Some(BODY) => Ok(OriFile::Whole { alphabet, unit, lines: body(&mut lines) }),
                Some(_) => Err(table(5 + values, "more in the unit than its header counts")),
            }
        }
        _ => Err(UnfoldError::Header(
            "a file folded with others names both its .orinv and its unit's fingerprint".into(),
        )),
    }
}

/// The line of an `.ori` the first of its lines is on: after the header, the unit's marker, its
/// choices, its table, its index's values, and the body's marker.
fn first_line(file: &OriFile) -> usize {
    match file {
        OriFile::Whole { unit, .. } => 6 + unit.index.as_ref().map_or(0, |index| index.len()),
        OriFile::Apart { .. } => 3,
        OriFile::Unit { .. } => 0,
    }
}

/// What an `.ori` holds: its text, given the `unit` its lines unfold with when the file keeps them
/// apart from it, in an `.orinv`. Refused if it needs a unit and has none, or another; if its
/// lines are damaged, by the line in the file; or if the text would come to more than `limit`
/// bytes, before those bytes are made.
pub fn unfold(text: &str, unit: Option<&Unit>, limit: usize) -> Result<String, UnfoldError> {
    let file = read(text)?;
    let first = first_line(&file);
    let (lines, unit, alphabet) = match &file {
        OriFile::Whole { alphabet, unit, lines } => (lines, unit, *alphabet),
        OriFile::Apart { alphabet, orinv, fingerprint, lines } => match unit {
            None => return Err(UnfoldError::NeedsOrinv(orinv.clone())),
            Some(unit) if unit.fingerprint() == *fingerprint => (lines, unit, *alphabet),
            Some(_) => return Err(UnfoldError::WrongOrinv),
        },
        OriFile::Unit { .. } => {
            return Err(UnfoldError::Unit(
                "an .orinv holds a unit and no lines: unfold the .ori files that name it".into(),
            ));
        }
    };
    parts::unfold(lines, unit, alphabet, limit).map_err(|error| in_file(error, first))
}

/// A refusal of a line, at its line in the file, the lines starting on `first`.
fn in_file(error: UnfoldError, first: usize) -> UnfoldError {
    let at = |line: usize| first + line - 1;
    match error {
        UnfoldError::Corrupt { line, problem } => UnfoldError::Corrupt { line: at(line), problem },
        UnfoldError::Alphabet { line, error } => UnfoldError::Alphabet { line: at(line), error },
        UnfoldError::TooLarge { line, limit } => UnfoldError::TooLarge { line: at(line), limit },
        error => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_and_lines(text: &str, index: bool) -> (Unit, Vec<String>) {
        let folded = parts::fold(text, Alphabet::Base64Url, index);
        (folded.unit, folded.lines)
    }

    fn indexed() -> String {
        (0..12)
            .map(|i| format!("\u{1b}[38;2;226;57;57m█ Simulator {}\u{1b}[0m and more", i % 3))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A whole `.ori` reads back as its unit and its lines, with an index and without, and
    /// unfolds alone.
    #[test]
    fn a_whole_ori_holds_its_unit_and_unfolds_alone() {
        for (text, index) in [("⣿⣿⡇\n\n⢸⣿", false), (indexed().as_str(), true), ("", false)]
        {
            let (unit, lines) = unit_and_lines(text, index);
            assert_eq!(unit.index.is_some(), index, "{text:?}");
            let file = whole(&lines, &unit, Alphabet::Base64Url);
            assert!(is_ori(&file));
            let read = read(&file).unwrap();
            assert_eq!(read, OriFile::Whole { alphabet: Alphabet::Base64Url, unit, lines });
            assert_eq!(unfold(&file, None, usize::MAX).ok().as_deref(), Some(text));
        }
    }

    /// An `.ori` of lines apart names its `.orinv` and its unit's fingerprint, and unfolds with
    /// that unit only; the `.orinv` holds the unit and no lines.
    #[test]
    fn lines_apart_unfold_with_their_unit_only() {
        let (unit, lines) = unit_and_lines(&indexed(), true);
        let file = apart(&lines, "../books/my chapters.orinv", &unit, Alphabet::Base64Url);
        let first = file.lines().next().unwrap();
        assert!(first.contains("orinv=../books/my\\u{20}chapters.orinv"), "{first}");
        match read(&file).unwrap() {
            OriFile::Apart { orinv, fingerprint, .. } => {
                assert_eq!(orinv, "../books/my chapters.orinv");
                assert_eq!(fingerprint, unit.fingerprint());
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(unfold(&file, Some(&unit), usize::MAX).ok(), Some(indexed()));
        assert!(matches!(unfold(&file, None, usize::MAX), Err(UnfoldError::NeedsOrinv(_))));
        let (other, _) = unit_and_lines("something else", false);
        assert!(matches!(unfold(&file, Some(&other), usize::MAX), Err(UnfoldError::WrongOrinv)));
        let orinv = unit_file(&unit, Alphabet::Base64Url);
        assert_eq!(read(&orinv).unwrap(), OriFile::Unit { alphabet: Alphabet::Base64Url, unit });
        assert!(matches!(unfold(&orinv, None, usize::MAX), Err(UnfoldError::Unit(_))));
    }

    /// A damaged line is refused at its line in the file, counting the header and the unit.
    #[test]
    fn a_damaged_line_is_refused_at_its_line_in_the_file() {
        let (unit, mut lines) = unit_and_lines(&indexed(), true);
        lines[2].push('_');
        let file = whole(&lines, &unit, Alphabet::Base64Url);
        let at = file.lines().position(|line| line == lines[2]).unwrap() + 1;
        match unfold(&file, None, usize::MAX) {
            Err(UnfoldError::Corrupt { line, .. }) => assert_eq!(line, at),
            other => panic!("{other:?}"),
        }
    }

    /// What no writer writes is refused: a unit cut short, more in the unit than the header
    /// counts, the lines of a file apart not starting at once, a file naming its `.orinv` but
    /// not its unit, a fingerprint that is none.
    #[test]
    fn what_no_writer_of_an_ori_writes_is_refused() {
        let (unit, lines) = unit_and_lines(&indexed(), true);
        let file = whole(&lines, &unit, Alphabet::Base64Url);
        let values = unit.index.as_ref().map_or(0, |index| index.len());
        let counted = format!("index={values}");
        let cut = file.replacen(&counted, &format!("index={}", values + 1), 1);
        let more = file.replacen(&counted, &format!("index={}", values - 1), 1);
        let apart_file = apart(&lines, "x.orinv", &unit, Alphabet::Base64Url);
        let unbodied = apart_file.replacen("=== body ===\n", "", 1);
        let half = apart_file.replacen(&format!(" unit={:08x}", unit.fingerprint()), "", 1);
        let bad_print =
            apart_file.replacen(&format!("unit={:08x}", unit.fingerprint()), "unit=xyz", 1);
        for damaged in [cut, more, unbodied, half, bad_print] {
            assert!(read(&damaged).is_err() || unfold(&damaged, Some(&unit), usize::MAX).is_err());
        }
    }
}
