//! The squeezes: what makes data smaller before an alphabet folds it.
//!
//! A squeeze works a line at a time, so that the folded file has a line for every line of the
//! original and diffs the way it does, or on the whole file at once, which is smaller but changes
//! everywhere with any edit. [`Squeeze::by_line`] says which.

pub(crate) mod deflate;
mod pack;
mod plain;
mod table;
mod text;
mod zstd;

pub use table::TableOptions;

use crate::alphabet::Alphabet;
use crate::bits::Bits;
use crate::container::{self, Folded, Header, Line as FileLine};
use crate::deadline::Deadline;
use crate::error::{FoldError, UnfoldError};
use crate::index::Index;

/// How data is made smaller before it is folded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Squeeze {
    /// The smallest line squeeze for the input: a table read with these options (where its lines
    /// are rows), text, pack, and deflate leaning on the line above are each tried, and the
    /// smallest fold is the one kept, its index counted. It takes as long as the four folds
    /// together, but none is made twice. Keeping an earlier fold's tables (see
    /// [`crate::fold_keeping`]), it is the squeeze that fold was made with.
    Auto(TableOptions),
    /// None: the bytes as they are, a line at a time, or as one stream.
    None { whole: bool },
    /// Each character as its place among the characters the file uses, in just the bits that
    /// many choices need. Its table changes only when a new character arrives.
    Pack,
    /// Each line spelled in a Huffman code the whole file shares, with terminal escapes and runs
    /// as single symbols.
    Text,
    /// Rows of cells, each column in a model of its own: listed values, numbers, distances,
    /// lists, or spelling.
    Table(TableOptions),
    /// Deflate over the whole file (`lean: None`), or a line at a time, each leaning on `lean`
    /// lines above it.
    Deflate { lean: Option<usize> },
    /// Zstandard over the whole file, at this level (1 to 22).
    Zstd { level: i32 },
}

impl Squeeze {
    /// The name a folded file's header calls it by; `auto` for [`Squeeze::Auto`], which no header
    /// names, since it names the squeeze chosen.
    pub fn name(&self) -> &'static str {
        match self {
            Squeeze::Auto(_) => "auto",
            Squeeze::None { .. } => "none",
            Squeeze::Pack => "pack",
            Squeeze::Text => "text",
            Squeeze::Table(_) => "table",
            Squeeze::Deflate { .. } => "deflate",
            Squeeze::Zstd { .. } => "zstd",
        }
    }

    /// Whether every line folds into a line of its own, so the folded file diffs line by line.
    pub fn by_line(&self) -> bool {
        match self {
            Squeeze::None { whole } => !whole,
            Squeeze::Auto(_) | Squeeze::Pack | Squeeze::Text | Squeeze::Table(_) => true,
            Squeeze::Deflate { lean } => lean.is_some(),
            Squeeze::Zstd { .. } => false,
        }
    }

    /// Whether it reads text, and so takes only UTF-8.
    pub fn reads_text(&self) -> bool {
        matches!(self, Squeeze::Pack | Squeeze::Text | Squeeze::Table(_))
    }

    /// Whether it has tables a later fold can keep (see [`crate::fold_keeping`]), and so can
    /// index (see [`crate::Fold::index`]): text and table, and auto, which may choose either.
    pub fn has_tables(&self) -> bool {
        matches!(self, Squeeze::Auto(_) | Squeeze::Text | Squeeze::Table(_))
    }

    /// The squeezes a fold with it tries, in order: auto's four, or just itself.
    pub(crate) fn candidates(&self) -> Vec<Squeeze> {
        match self {
            Squeeze::Auto(table) => vec![
                Squeeze::Table(table.clone()),
                Squeeze::Text,
                Squeeze::Pack,
                Squeeze::Deflate { lean: Some(1) },
            ],
            squeeze => vec![squeeze.clone()],
        }
    }

    /// The squeeze a folded file's header names, with the options the header holds. A table
    /// reads with `table`, which its own tables override, and zstd, whose level no header keeps,
    /// is level 19.
    pub(crate) fn named_in(header: &Header, table: &TableOptions) -> Result<Squeeze, UnfoldError> {
        let whole = header.has("whole");
        Ok(match header.squeeze.as_str() {
            "none" => Squeeze::None { whole },
            "pack" => Squeeze::Pack,
            "text" => Squeeze::Text,
            "table" => Squeeze::Table(table.clone()),
            "deflate" if whole => Squeeze::Deflate { lean: None },
            "deflate" => Squeeze::Deflate {
                lean: Some(header.value("lean").and_then(|lean| lean.parse().ok()).ok_or_else(
                    || UnfoldError::Header("deflate a line at a time needs `lean=<lines>`".into()),
                )?),
            },
            "zstd" => Squeeze::Zstd { level: 19 },
            other => {
                return Err(UnfoldError::Header(format!(
                    "`{other}` is no squeeze this orimoji knows"
                )));
            }
        })
    }

    /// It, in a few words.
    pub fn describe(&self) -> String {
        match self {
            Squeeze::Auto(_) => "auto, the smallest line squeeze".to_string(),
            Squeeze::None { whole: true } => "none, whole file".to_string(),
            Squeeze::None { whole: false } => "none".to_string(),
            Squeeze::Deflate { lean: None } => "deflate, whole file".to_string(),
            Squeeze::Deflate { lean: Some(0) } => "deflate, each line alone".to_string(),
            Squeeze::Deflate { lean: Some(1) } => "deflate, leaning on the line above".to_string(),
            Squeeze::Deflate { lean: Some(lean) } => {
                format!("deflate, leaning on {lean} lines above")
            }
            Squeeze::Zstd { level } => format!("zstd {level}, whole file"),
            other => other.name().to_string(),
        }
    }
}

/// A body line before it is folded.
pub(crate) enum Line {
    Empty,
    Bits(Bits),
    Bytes(Vec<u8>),
    /// A line kept as it is, already escaped.
    Kept(String),
}

/// A body before it is folded: a line for each line, or one stream folded in line-sized pieces.
pub(crate) enum Body {
    Lines(Vec<Line>),
    Stream(Vec<u8>),
}

/// A squeeze's tables before they are written: as text already, or as the models that write them
/// once the index their values go into is known.
pub(crate) enum Tables {
    Lines(Vec<String>),
    Text(text::TextCode),
    Table(table::Table),
}

impl Tables {
    /// The values these tables write as places in an index.
    pub(crate) fn index_values(&self) -> Vec<String> {
        match self {
            Tables::Lines(_) => Vec::new(),
            Tables::Text(code) => code.symbols(),
            Tables::Table(table) => table.index_values(),
        }
    }

    /// The table lines, the values that are in `index` written as their places.
    fn lines(&self, index: Option<&Index>) -> Vec<String> {
        match self {
            Tables::Lines(lines) => lines.clone(),
            Tables::Text(code) => {
                let mut lines = vec!["=== symbols ===".to_string()];
                lines.extend(code.table(index));
                lines
            }
            Tables::Table(table) => table.header(index),
        }
    }
}

/// What a squeeze made: its header options, its tables, and the body.
pub(crate) struct Squeezed {
    pub(crate) options: Vec<String>,
    pub(crate) tables: Tables,
    pub(crate) body: Body,
}

/// An earlier fold whose tables a fold keeps, and the index it was folded with.
pub(crate) type Kept<'a> = (&'a Folded<'a>, Option<&'a Index>);

/// `input`, squeezed: with the tables of `kept` when given, and allowed to index when `index`. The
/// squeezes that can index give up past `deadline`; the others never take long enough to need to.
pub(crate) fn squeeze(
    input: &[u8],
    squeeze: &Squeeze,
    kept: Option<Kept<'_>>,
    index: bool,
    deadline: Deadline,
) -> Result<Squeezed, FoldError> {
    let text = || {
        std::str::from_utf8(input).map_err(|error| FoldError::NotText { at: error.valid_up_to() })
    };
    match squeeze {
        Squeeze::Auto(_) => {
            unreachable!("auto is resolved to one of its squeezes before squeezing")
        }
        Squeeze::None { whole } => Ok(plain::squeeze(input, *whole)),
        Squeeze::Pack => Ok(pack::squeeze(text()?)),
        Squeeze::Text => text::squeeze(text()?, kept, index, deadline),
        Squeeze::Table(options) => table::squeeze(text()?, options, kept, index, deadline),
        Squeeze::Deflate { lean } => deflate::squeeze(input, *lean),
        Squeeze::Zstd { level } => zstd::squeeze(input, *level),
    }
}

/// The folded file, with its tables deflated where that makes them smaller, unless `readable`.
/// Values its tables put in an index are written as their places in `index`, which must hold them.
pub(crate) fn render(
    squeeze: &str,
    squeezed: &Squeezed,
    alphabet: Alphabet,
    wrap: usize,
    readable: bool,
    index: Option<&Index>,
) -> String {
    render_within(squeeze, squeezed, alphabet, wrap, readable, index, Deadline::NONE)
        .expect("no deadline to pass")
}

/// [`render`], giving up past `deadline`.
pub(crate) fn render_within(
    squeeze: &str,
    squeezed: &Squeezed,
    alphabet: Alphabet,
    wrap: usize,
    readable: bool,
    index: Option<&Index>,
    deadline: Deadline,
) -> Result<String, FoldError> {
    let index = index.filter(|_| !squeezed.tables.index_values().is_empty());
    let mut options = squeezed.options.clone();
    options.extend(index.map(|index| format!("index={}", index.mark())));
    let header = Header { squeeze: squeeze.to_string(), alphabet, options };
    let lines = squeezed.tables.lines(index);
    let tables = match readable || lines.is_empty() {
        true => lines,
        false => {
            let packed = deflate::deflate(lines.join("\n").as_bytes(), &[])
                .expect("deflate takes any bytes");
            let mut deflated = vec![container::DEFLATED.to_string()];
            deflated.extend(stream(&packed, alphabet, wrap));
            let size = |lines: &[String]| lines.iter().map(|line| line.len() + 1).sum::<usize>();
            match size(&deflated) < size(&lines) {
                true => deflated,
                false => lines,
            }
        }
    };
    // The body goes straight into the file, a line at a time.
    let mut out = container::head(&header, &tables);
    match &squeezed.body {
        Body::Lines(lines) => {
            for (at, line) in lines.iter().enumerate() {
                deadline.check_at(at)?;
                out.push('\n');
                match line {
                    Line::Empty => {}
                    Line::Bits(bits) => out.push_str(&alphabet.fold_bits(bits)),
                    Line::Bytes(bytes) => out.push_str(&alphabet.fold_bytes(bytes)),
                    Line::Kept(text) => {
                        out.push(' ');
                        out.push_str(text);
                    }
                }
            }
        }
        Body::Stream(bytes) => {
            for piece in stream(bytes, alphabet, wrap) {
                out.push('\n');
                out.push_str(&piece);
            }
        }
    }
    out.push('\n');
    deadline.check()?;
    Ok(out)
}

/// Bytes folded in pieces that each make a line about `wrap` cells wide, one piece at a time: an
/// empty line for no bytes at all.
fn stream(bytes: &[u8], alphabet: Alphabet, wrap: usize) -> impl Iterator<Item = String> + '_ {
    let chars = (wrap / alphabet.cells_per_char()).max(1);
    let piece = ((chars as f64 * alphabet.bits_per_char() / 8.0) as usize).max(1);
    let empty = bytes.is_empty().then(String::new);
    empty.into_iter().chain(bytes.chunks(piece).map(move |piece| alphabet.fold_bytes(piece)))
}

/// The index `folded` needs, from the one given: none if it needs none, and refused if it needs one
/// and `given` isn't it.
pub(crate) fn needed<'a>(
    folded: &Folded<'_>,
    given: Option<&'a Index>,
) -> Result<Option<&'a Index>, UnfoldError> {
    match (folded.header.value("index"), given) {
        (None, _) => Ok(None),
        (Some(_), None) => Err(UnfoldError::NeedsIndex),
        (Some(mark), Some(index)) if index.fits(mark) => Ok(Some(index)),
        (Some(_), Some(_)) => Err(UnfoldError::WrongIndex),
    }
}

pub(crate) fn unsqueeze(
    folded: &Folded<'_>,
    index: Option<&Index>,
) -> Result<Vec<u8>, UnfoldError> {
    let index = needed(folded, index)?;
    match folded.header.squeeze.as_str() {
        "none" => plain::unsqueeze(folded),
        "pack" => pack::unsqueeze(folded),
        "text" => text::unsqueeze(folded, index),
        "table" => table::unsqueeze(folded, index),
        "deflate" => deflate::unsqueeze(folded),
        "zstd" => zstd::unsqueeze(folded),
        other => Err(UnfoldError::Header(format!("`{other}` is no squeeze this orimoji knows"))),
    }
}

/// A body line's bits.
pub(crate) fn bits_of(folded: &Folded<'_>, line: &FileLine<'_>) -> Result<Bits, UnfoldError> {
    folded
        .header
        .alphabet
        .unfold_bits(line.text)
        .map_err(|error| UnfoldError::Alphabet { line: line.number, error })
}

/// A body line's bytes.
pub(crate) fn bytes_of(folded: &Folded<'_>, line: &FileLine<'_>) -> Result<Vec<u8>, UnfoldError> {
    folded
        .header
        .alphabet
        .unfold_bytes(line.text)
        .map_err(|error| UnfoldError::Alphabet { line: line.number, error })
}

/// The options a whole-file squeeze writes: that it is one, and the size of what it folded, which
/// bounds and checks the unfold.
pub(crate) fn whole(input: &[u8]) -> Vec<String> {
    vec!["whole".to_string(), format!("size={}", input.len())]
}

/// The size a whole-file squeeze says it folded.
pub(crate) fn size(folded: &Folded<'_>) -> Result<usize, UnfoldError> {
    let size = folded.header.value("size").and_then(|size| size.parse().ok());
    size.ok_or_else(|| UnfoldError::Header("a whole-file squeeze needs `size=<bytes>`".into()))
}

/// What a whole-file squeeze unfolded, if it is the size its header says.
pub(crate) fn sized(folded: &Folded<'_>, unfolded: Vec<u8>) -> Result<Vec<u8>, UnfoldError> {
    let size = size(folded)?;
    match unfolded.len() == size {
        true => Ok(unfolded),
        false => Err(corrupt(
            &folded.body[0],
            &format!("{} bytes came out, where {size} went in", unfolded.len()),
        )),
    }
}

/// The bytes of a body that is one stream in pieces.
pub(crate) fn stream_of(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    let mut stream = Vec::new();
    for line in &folded.body {
        stream.extend(bytes_of(folded, line)?);
    }
    Ok(stream)
}

/// A body of a line for each line, each turned back into its bytes by `unsqueeze`.
pub(crate) fn lines_of(
    folded: &Folded<'_>,
    unsqueeze: impl Fn(&FileLine<'_>, Vec<u8>) -> Result<Vec<u8>, UnfoldError>,
) -> Result<Vec<u8>, UnfoldError> {
    let mut out = Vec::new();
    for (at, line) in folded.body.iter().enumerate() {
        if at > 0 {
            out.push(b'\n');
        }
        if !line.text.is_empty() {
            out.extend(unsqueeze(line, bytes_of(folded, line)?)?);
        }
    }
    Ok(out)
}

pub(crate) fn corrupt(line: &FileLine<'_>, problem: &str) -> UnfoldError {
    UnfoldError::Corrupt { line: line.number, problem: problem.to_string() }
}
