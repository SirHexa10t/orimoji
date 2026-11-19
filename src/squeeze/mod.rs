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
use crate::error::{FoldError, UnfoldError};

/// How data is made smaller before it is folded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Squeeze {
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
    /// The name a folded file's header calls it by.
    pub fn name(&self) -> &'static str {
        match self {
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
            Squeeze::Pack | Squeeze::Text | Squeeze::Table(_) => true,
            Squeeze::Deflate { lean } => lean.is_some(),
            Squeeze::Zstd { .. } => false,
        }
    }

    /// Whether it reads text, and so takes only UTF-8.
    pub fn reads_text(&self) -> bool {
        matches!(self, Squeeze::Pack | Squeeze::Text | Squeeze::Table(_))
    }

    /// Whether it has tables a later fold can keep (see [`crate::fold_keeping`]).
    pub fn has_tables(&self) -> bool {
        matches!(self, Squeeze::Text | Squeeze::Table(_))
    }

    /// It, in a few words.
    pub fn describe(&self) -> String {
        match self {
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

/// What a squeeze made: its header options, its tables, and the body.
pub(crate) struct Squeezed {
    pub(crate) options: Vec<String>,
    pub(crate) tables: Vec<String>,
    pub(crate) body: Body,
}

pub(crate) fn squeeze(
    input: &[u8],
    squeeze: &Squeeze,
    kept: Option<&Folded<'_>>,
) -> Result<Squeezed, FoldError> {
    let text = || {
        std::str::from_utf8(input).map_err(|error| FoldError::NotText { at: error.valid_up_to() })
    };
    match squeeze {
        Squeeze::None { whole } => Ok(plain::squeeze(input, *whole)),
        Squeeze::Pack => Ok(pack::squeeze(text()?)),
        Squeeze::Text => text::squeeze(text()?, kept),
        Squeeze::Table(options) => table::squeeze(text()?, options, kept),
        Squeeze::Deflate { lean } => deflate::squeeze(input, *lean),
        Squeeze::Zstd { level } => zstd::squeeze(input, *level),
    }
}

/// The folded file, with its tables deflated where that makes them smaller, unless `readable`.
pub(crate) fn render(
    squeeze: &str,
    squeezed: &Squeezed,
    alphabet: Alphabet,
    wrap: usize,
    readable: bool,
) -> String {
    let header =
        Header { squeeze: squeeze.to_string(), alphabet, options: squeezed.options.clone() };
    let body: Vec<String> = match &squeezed.body {
        Body::Lines(lines) => lines
            .iter()
            .map(|line| match line {
                Line::Empty => String::new(),
                Line::Bits(bits) => alphabet.fold_bits(bits),
                Line::Bytes(bytes) => alphabet.fold_bytes(bytes),
                Line::Kept(text) => format!(" {text}"),
            })
            .collect(),
        Body::Stream(bytes) => stream(bytes, alphabet, wrap),
    };
    let tables = match readable || squeezed.tables.is_empty() {
        true => squeezed.tables.clone(),
        false => {
            let packed = deflate::deflate(squeezed.tables.join("\n").as_bytes(), &[])
                .expect("deflate takes any bytes");
            let mut deflated = vec![container::DEFLATED.to_string()];
            deflated.extend(stream(&packed, alphabet, wrap));
            let size = |lines: &[String]| lines.iter().map(|line| line.len() + 1).sum::<usize>();
            match size(&deflated) < size(&squeezed.tables) {
                true => deflated,
                false => squeezed.tables.clone(),
            }
        }
    };
    container::assemble(&header, &tables, &body)
}

/// Bytes folded in pieces that each make a line about `wrap` cells wide.
fn stream(bytes: &[u8], alphabet: Alphabet, wrap: usize) -> Vec<String> {
    let chars = (wrap / alphabet.cells_per_char()).max(1);
    let piece = ((chars as f64 * alphabet.bits_per_char() / 8.0) as usize).max(1);
    match bytes.is_empty() {
        true => vec![String::new()],
        false => bytes.chunks(piece).map(|piece| alphabet.fold_bytes(piece)).collect(),
    }
}

pub(crate) fn unsqueeze(folded: &Folded<'_>) -> Result<Vec<u8>, UnfoldError> {
    match folded.header.squeeze.as_str() {
        "none" => plain::unsqueeze(folded),
        "pack" => pack::unsqueeze(folded),
        "text" => text::unsqueeze(folded),
        "table" => table::unsqueeze(folded),
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
