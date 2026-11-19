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
        }
    }
}

impl std::error::Error for FoldError {}

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
