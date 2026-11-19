//! The folded file: a header line, the squeeze's tables, and the body.
//!
//! ```text
//! === orimoji 1 table base64url ===
//! === table ===
//! separator  \t
//! …
//! === body ===
//! wJgK8a…
//! ```
//!
//! The header names the format version, the squeeze, the alphabet and the squeeze's options. The
//! tables are whatever the squeeze shares between lines, as text under `=== name ===` section
//! lines. Unless a fold is asked to keep them readable, that text is deflated and folded in the
//! body's alphabet, under `=== deflated tables ===`, whenever that makes it smaller. Every line after `=== body ===` is body, whatever it looks like,
//! since a body line of base94 may well start with `===` itself. The file ends in a newline, which
//! isn't part of the body; a file whose editor dropped it still reads the same.

use crate::alphabet::Alphabet;
use crate::error::UnfoldError;

/// The format this orimoji writes, and the newest it reads.
pub(crate) const VERSION: u32 = 1;

const BODY: &str = "=== body ===";

/// The first table line, when the tables are deflated and folded like a body.
pub(crate) const DEFLATED: &str = "=== deflated tables ===";

/// What the header line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Header {
    pub(crate) squeeze: String,
    pub(crate) alphabet: Alphabet,
    pub(crate) options: Vec<String>,
}

impl Header {
    fn line(&self) -> String {
        let options: String = self.options.iter().map(|option| format!(" {option}")).collect();
        format!("=== orimoji {VERSION} {} {}{options} ===", self.squeeze, self.alphabet)
    }

    /// Whether the option `name` is given, alone or as `name=…`.
    pub(crate) fn has(&self, name: &str) -> bool {
        self.options.iter().any(|option| option == name || option.starts_with(&format!("{name}=")))
    }

    /// The value of the option `name=value`.
    pub(crate) fn value(&self, name: &str) -> Option<&str> {
        self.options.iter().find_map(|option| option.strip_prefix(name)?.strip_prefix('='))
    }
}

/// A line of a folded file, with its number in the file, counting from 1.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'a> {
    pub(crate) number: usize,
    pub(crate) text: &'a str,
}

/// A table line, with the number of the line in the file it came from: itself, or the
/// `=== deflated tables ===` line for tables that were deflated.
#[derive(Clone, Debug)]
pub(crate) struct TableLine {
    pub(crate) number: usize,
    pub(crate) text: String,
}

/// A folded file, read into its parts.
#[derive(Debug)]
pub(crate) struct Folded<'a> {
    pub(crate) header: Header,
    pub(crate) tables: Vec<TableLine>,
    pub(crate) body: Vec<Line<'a>>,
}

/// The parts of a folded file.
pub(crate) fn parse(folded: &str) -> Result<Folded<'_>, UnfoldError> {
    let folded = folded.strip_suffix('\n').unwrap_or(folded);
    let mut lines = folded.split('\n').enumerate().map(|(index, text)| Line {
        number: index + 1,
        text: text.strip_suffix('\r').unwrap_or(text),
    });
    let header = lines.next().map(|line| header(line.text)).ok_or(UnfoldError::NotFolded)??;
    let mut tables: Vec<Line<'_>> = Vec::new();
    for line in lines.by_ref() {
        if line.text == BODY {
            let mut body: Vec<Line<'_>> = lines.collect();
            if body.is_empty() {
                body.push(Line { number: line.number + 1, text: "" });
            }
            let tables = match tables.first() {
                Some(first) if first.text == DEFLATED => inflate_tables(&header, &tables)?,
                _ => tables
                    .iter()
                    .map(|line| TableLine { number: line.number, text: line.text.to_string() })
                    .collect(),
            };
            return Ok(Folded { header, tables, body });
        }
        tables.push(line);
    }
    Err(UnfoldError::NoBody)
}

/// Tables that were deflated, as the text they were.
fn inflate_tables(header: &Header, lines: &[Line<'_>]) -> Result<Vec<TableLine>, UnfoldError> {
    let marker = lines[0].number;
    let mut packed = Vec::new();
    for line in &lines[1..] {
        let bytes = header.alphabet.unfold_bytes(line.text);
        packed.extend(bytes.map_err(|error| UnfoldError::Alphabet { line: line.number, error })?);
    }
    let problem = |problem: String| UnfoldError::Table { line: marker, problem };
    // Tables are never more than a few times the body they describe.
    let limit = 64 * packed.len() + (1 << 20);
    let text = crate::squeeze::deflate::inflate(&packed, &[], Some(limit)).map_err(problem)?;
    let text =
        String::from_utf8(text).map_err(|_| problem("the deflated tables aren't UTF-8".into()))?;
    Ok(text.split('\n').map(|text| TableLine { number: marker, text: text.to_string() }).collect())
}

/// The header of a folded text, from its first line alone.
pub(crate) fn header_of(folded: &str) -> Result<Header, UnfoldError> {
    let first = folded.split('\n').next().unwrap_or_default();
    header(first.strip_suffix('\r').unwrap_or(first))
}

fn header(line: &str) -> Result<Header, UnfoldError> {
    let inner = line.strip_prefix("=== orimoji ").and_then(|rest| rest.strip_suffix(" ==="));
    let mut words = inner.ok_or(UnfoldError::NotFolded)?.split(' ');
    let version: u32 =
        words.next().and_then(|word| word.parse().ok()).ok_or(UnfoldError::NotFolded)?;
    if version > VERSION {
        return Err(UnfoldError::NewerVersion(version));
    }
    let squeeze = words.next().ok_or_else(|| UnfoldError::Header("no squeeze is named".into()))?;
    let alphabet =
        words.next().ok_or_else(|| UnfoldError::Header("no alphabet is named".into()))?;
    let alphabet = Alphabet::from_name(alphabet).ok_or_else(|| {
        UnfoldError::Header(format!("`{alphabet}` is no alphabet this orimoji knows"))
    })?;
    Ok(Header {
        squeeze: squeeze.to_string(),
        alphabet,
        options: words.map(str::to_string).collect(),
    })
}

/// The file up to its `=== body ===` line: each body line then follows as `\n` and the line, and
/// the file ends in a last `\n`. Written that way, a body is never held twice, as its lines and
/// as the file.
pub(crate) fn head(header: &Header, tables: &[String]) -> String {
    let size = tables.iter().map(|line| line.len() + 1).sum::<usize>() + 80;
    let mut out = String::with_capacity(size);
    out.push_str(&header.line());
    for line in tables.iter().map(String::as_str).chain([BODY]) {
        out.push('\n');
        out.push_str(line);
    }
    out
}

/// The whole file, from its parts.
#[cfg(test)]
pub(crate) fn assemble(header: &Header, tables: &[String], body: &[String]) -> String {
    let mut out = head(header, tables);
    for line in body {
        out.push('\n');
        out.push_str(line);
    }
    out.push('\n');
    out
}

/// Table lines grouped under their `=== name ===` lines; lines before the first one go under "".
pub(crate) fn sections(tables: &[TableLine]) -> Vec<(&str, Vec<Line<'_>>)> {
    let mut sections: Vec<(&str, Vec<Line<'_>>)> = vec![("", Vec::new())];
    for table_line in tables {
        let line = Line { number: table_line.number, text: &table_line.text };
        match line.text.strip_prefix("=== ").and_then(|rest| rest.strip_suffix(" ===")) {
            Some(name) => sections.push((name, Vec::new())),
            None => sections.last_mut().expect("there is always one").1.push(line),
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_back_into_the_parts_it_was_made_of() {
        let header = Header {
            squeeze: "deflate".into(),
            alphabet: Alphabet::Cjk,
            options: vec!["lean=2".into()],
        };
        let file = assemble(
            &header,
            &["=== symbols ===".into(), "1\t<end>".into()],
            &["A".into(), "".into()],
        );
        assert_eq!(
            file,
            "=== orimoji 1 deflate cjk lean=2 ===\n=== symbols ===\n1\t<end>\n=== body ===\nA\n\n"
        );
        let parsed = parse(&file).unwrap();
        assert_eq!(parsed.header, header);
        assert_eq!(parsed.header.value("lean"), Some("2"));
        assert!(parsed.header.has("lean") && !parsed.header.has("whole"));
        assert_eq!(parsed.tables.len(), 2);
        let body: Vec<_> = parsed.body.iter().map(|line| (line.number, line.text)).collect();
        assert_eq!(body, [(5, "A"), (6, "")]);
    }

    #[test]
    fn what_is_no_folded_file_says_so() {
        assert!(matches!(parse("hello\n"), Err(UnfoldError::NotFolded)));
        assert!(matches!(
            parse("=== orimoji 9 none hex ===\n=== body ===\n"),
            Err(UnfoldError::NewerVersion(9))
        ));
        assert!(matches!(parse("=== orimoji 1 none klingon ===\n"), Err(UnfoldError::Header(_))));
        assert!(matches!(parse("=== orimoji 1 none hex ===\nAB\n"), Err(UnfoldError::NoBody)));
    }

    #[test]
    fn line_endings_an_editor_changed_still_read() {
        let parsed = parse("=== orimoji 1 none hex ===\r\n=== body ===\r\nab\r\n").unwrap();
        assert_eq!(parsed.body[0].text, "ab");
        let bare = parse("=== orimoji 1 none hex ===\n=== body ===").unwrap();
        assert_eq!(bare.body.len(), 1, "a body cut off before its one empty line is that line");
    }
}
