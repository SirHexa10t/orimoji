//! What can go wrong folding and unfolding. Lines are counted from 1, in the folded file.

use crate::alphabet::AlphabetError;
use std::fmt;

/// Why something couldn't be folded.
#[derive(Debug)]
#[non_exhaustive]
pub enum FoldError {
    /// The squeeze reads text, and the input stops being UTF-8 at this byte.
    NotText { at: usize },
    /// A table squeeze found no line that splits into columns.
    NotATable,
    /// The tables of the fold to keep don't fit this input.
    TablesDontFit(String),
    /// The fold whose tables were to be kept doesn't read.
    Previous(UnfoldError),
    /// The fold to keep tables from was squeezed another way, which has other tables or none.
    PreviousSqueeze(String),
    /// This build has no zstd.
    NoZstd,
    /// A compressor failed.
    Compressor(String),
    /// A measurement ran out of the time it was given. orimoji's own folds catch this and fall
    /// back to the estimates, so a caller never sees it from them.
    OutOfTime,
    /// A fold buffered in a temporary file while another was made couldn't be read back.
    Buffer(std::io::Error),
}

impl fmt::Display for FoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FoldError::NotText { at } => {
                write!(f, "this squeeze reads text, and the input isn't UTF-8 from byte {at} on")
            }
            FoldError::NotATable => write!(f, "no line splits into columns, so there is no table"),
            FoldError::TablesDontFit(why) => write!(f, "the kept tables don't fit: {why}"),
            FoldError::Previous(error) => write!(f, "the fold to keep tables from: {error}"),
            FoldError::PreviousSqueeze(squeeze) => {
                write!(
                    f,
                    "the fold to keep tables from was squeezed with {squeeze}, not this squeeze"
                )
            }
            FoldError::NoZstd => write!(f, "this build of orimoji has no zstd"),
            FoldError::Compressor(why) => write!(f, "the compressor failed: {why}"),
            FoldError::OutOfTime => write!(f, "the measurement ran out of time"),
            FoldError::Buffer(error) => {
                write!(f, "a fold buffered in a temporary file couldn't be read back: {error}")
            }
        }
    }
}

impl std::error::Error for FoldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FoldError::Buffer(error) => Some(error),
            _ => None,
        }
    }
}

/// Why a folded file couldn't be unfolded.
#[derive(Debug)]
#[non_exhaustive]
pub enum UnfoldError {
    /// Its first line isn't an orimoji header: it was never folded.
    NotFolded,
    /// It was folded by a newer orimoji, in a format version this one can't read.
    NewerVersion(u32),
    /// Its header names something this orimoji doesn't know.
    Header(String),
    /// It has no `=== body ===` line.
    NoBody,
    /// A line of its tables doesn't read.
    Table { line: usize, problem: String },
    /// A body line has text its alphabet didn't write.
    Alphabet { line: usize, error: AlphabetError },
    /// A body line's bits don't decode: the file was changed, or cut short.
    Corrupt { line: usize, problem: String },
    /// It was squeezed with zstd, which this build lacks.
    NoZstd,
    /// It was folded with an index, and none was given.
    NeedsIndex,
    /// The index given isn't the one it was folded with.
    WrongIndex,
    /// An index's text doesn't read.
    Index(String),
    /// The choices of a fold into parts don't read, or are a newer format's.
    Choices(String),
    /// The code table of a fold into parts doesn't read.
    CodeTable(String),
    /// The table, the choices or the index aren't the ones a fold into parts made its lines with:
    /// one is damaged, or comes from another fold.
    PartsDontFit,
    /// A line would make the text more than the limit it was unfolded with.
    TooLarge { line: usize, limit: usize },
}

impl fmt::Display for UnfoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnfoldError::NotFolded => {
                write!(f, "this isn't a folded file: its first line is no orimoji header")
            }
            UnfoldError::NewerVersion(version) => {
                write!(f, "folded in format version {version}, newer than this orimoji reads")
            }
            UnfoldError::Header(why) => write!(f, "header: {why}"),
            UnfoldError::NoBody => write!(f, "there is no `=== body ===` line"),
            UnfoldError::Table { line, problem } => write!(f, "line {line}: {problem}"),
            UnfoldError::Alphabet { line, error } => write!(f, "line {line}: {error}"),
            UnfoldError::Corrupt { line, problem } => write!(f, "line {line}: {problem}"),
            UnfoldError::NoZstd => {
                write!(f, "it was squeezed with zstd, and this build of orimoji has none")
            }
            UnfoldError::NeedsIndex => {
                write!(f, "it was folded with an index, and unfolding it needs that index")
            }
            UnfoldError::WrongIndex => {
                write!(f, "the index given isn't the one it was folded with")
            }
            UnfoldError::Index(why) => write!(f, "the index: {why}"),
            UnfoldError::Choices(why) => write!(f, "the choices: {why}"),
            UnfoldError::CodeTable(why) => write!(f, "the code table: {why}"),
            UnfoldError::PartsDontFit => write!(
                f,
                "the table, the choices and the index aren't the ones the lines were folded with: \
                 one is damaged, or from another fold"
            ),
            UnfoldError::TooLarge { line, limit } => {
                write!(f, "line {line}: it would unfold to more than {limit} bytes")
            }
        }
    }
}

impl std::error::Error for UnfoldError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UnfoldError::Alphabet { error, .. } => Some(error),
            _ => None,
        }
    }
}
