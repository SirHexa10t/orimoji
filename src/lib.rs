//! Fold text and data into fewer characters, fewer bytes or narrower lines, and unfold it back
//! exactly. *Ori* (折り) is the fold of origami; *moji* (文字), the character of emoji.
//!
//! A fold is two steps:
//!
//! - a [`Squeeze`] makes the data smaller: packing, a Huffman code, a table's column models,
//!   deflate or zstd, nothing, or whichever line squeeze is smallest ([`Squeeze::Auto`]);
//! - an [`Alphabet`] writes what is left as characters: hex, base32, base58, base64, Z85, basE91,
//!   base94, braille, two-byte letters, one-cell symbols or CJK.
//!
//! The folded text names both in its first line, so [`unfold`] needs nothing else, unless the fold
//! made an index:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Indexing, Squeeze};
//!
//! let listing = "app_id\tname\n10\tCounter-Strike\n20\tTeam Fortress Classic\n";
//! let folded = orimoji::fold(listing.as_bytes(), &Fold::new(Squeeze::Text, Alphabet::Base64Url))?;
//! assert!(folded.text.starts_with("=== orimoji 1 text base64url ==="));
//! assert_eq!(folded.indexing, Indexing::Off);
//! assert_eq!(orimoji::unfold(&folded.text, None)?, listing.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Indexes
//!
//! Asked to, the table and text squeezes index: a column's common values, and common words and
//! terminal escapes, are written as their places in a list. The list, the [`Index`], comes back
//! beside the folded text for the caller to keep wherever suits it (in a file format of its own,
//! beside the text, or once for many texts) and to give back when unfolding. [`Indexing`] says
//! what became of it: not asked for, asked for but not worth keeping, or the index to keep.
//!
//! ```
//! use orimoji::{Alphabet, Fold, Index, Squeeze, TableOptions};
//!
//! let studios = ["Valve", "Gearbox Software", "Bethesda"];
//! let rows: String = (0..300).map(|i| format!("{i}\t{}\n", studios[i % 3])).collect();
//! let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
//! let folded = orimoji::fold(rows.as_bytes(), &how)?;
//! let index = folded.indexing.index().expect("three studios are worth listing");
//! assert_eq!(index.values(), ["Bethesda", "Gearbox Software", "Valve"]);
//! assert!(orimoji::needs_index(&folded.text)?);
//! assert!(orimoji::unfold(&folded.text, None).is_err(), "not without its index");
//! let kept = Index::from_text(&index.to_text())?; // as the caller would keep it
//! assert_eq!(orimoji::unfold(&folded.text, Some(&kept))?, rows.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Folds that diff
//!
//! The line squeezes ([`Squeeze::by_line`]) fold every line into a line of its own, so a folded
//! text kept in git diffs like the original. Their tables come from the whole text, though, and a
//! rebuilt table can change every line. [`fold_keeping`] folds a new version with an older fold's
//! tables (and index) instead, so only the lines that changed change:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
//!
//! let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url);
//! let before = orimoji::fold(b"10\tValve\n20\tValve\n30\tGearbox", &how)?.text;
//! let newer = b"10\tValve\n20\tValve\n30\tGearbox\n40\tNew Studio";
//! let after = orimoji::fold_keeping(newer, &before, None, &how)?;
//! let (old, new): (Vec<_>, Vec<_>) = (before.lines().collect(), after.lines().collect());
//! assert_eq!(old[old.len() - 3..], new[new.len() - 4..new.len() - 1], "the old rows fold the same");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Files
//!
//! [`files::fold_files`] and [`files::unfold_files`] do the same to files, in place or beside them
//! with a suffix, as the caller's [`Output`] says; files folded together share one index.
//!
//! # Parts
//!
//! A program with a file format of its own folds into [`parts`] instead: the lines, the code they
//! are spelled in and the choices the fold made, for it to keep where it suits, smaller than a
//! folded file and with no header of their own.

pub mod alphabet;
pub mod bits;
pub mod files;
pub mod huffman;
pub mod index;
pub mod measure;
pub mod parts;
pub mod squeeze;
pub mod words;

mod bignum;
mod buffer;
mod container;
mod deadline;
mod error;
mod escape;

pub use alphabet::Alphabet;
pub use error::{FoldError, UnfoldError};
pub use files::Output;
pub use index::{Index, Indexing};
pub use squeeze::{Squeeze, TableOptions};

use buffer::Buffered;
use deadline::Deadline;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How to fold: a squeeze, an alphabet, and how the folded file is laid out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fold {
    pub squeeze: Squeeze,
    pub alphabet: Alphabet,
    /// The screen cells a body line takes when the body is one stream, as the whole-file squeezes'
    /// is. 100 by default.
    pub wrap: usize,
    /// Whether the squeeze's tables stay readable text. By default they are deflated and folded
    /// like the body wherever that makes them smaller, which is nearly always.
    pub readable: bool,
    /// Whether the table and text squeezes may index: write a column's common values, and common
    /// words and terminal escapes, as their places in a list. A value is listed only if it saves
    /// at least twice the space it takes, and a list that takes over half the space it saves is
    /// left out. The list comes back beside the folded text (see [`Indexing`]), for the caller to
    /// keep wherever it suits and to give back to [`unfold`]. Off by default, since a fold without
    /// an index is simpler to follow; on, it is usually smaller.
    pub index: bool,
    /// Whether to measure, not estimate, whether the index pays, and for how long at most: when
    /// the estimates keep an index, fold without it too, and keep whichever fold is smaller, the
    /// index counted as its text. The fold kept is the one given back, never made again. The fold
    /// without the index may take this long at most; past it, it is given up and the estimates
    /// decide. [`Judged`] says which happened. `None`, the default, measures nothing.
    pub measure_up_to: Option<Duration>,
    /// The size past which a fold waiting to be weighed against another (the estimates' fold
    /// while the fold without the index is made, or the smallest of [`Squeeze::Auto`]'s so far)
    /// is buffered in a temporary file instead of held in memory, so that the two are never in
    /// memory together. [`BUFFER_PAST`] by default.
    pub buffer_past: usize,
    /// Where those temporary files go. `None`, the default, is the system's temporary folder
    /// ([`std::env::temp_dir`], which `TMPDIR` sets), or, folding files, the folder the first of
    /// them is written to. A temporary file is gone once it isn't needed; on Unix it has no name
    /// from the moment it is opened, so not even a killed fold leaves one behind.
    pub buffer_folder: Option<PathBuf>,
}

/// [`Fold::buffer_past`] unless set otherwise: 8 MiB.
pub const BUFFER_PAST: usize = 8 << 20;

impl Fold {
    pub fn new(squeeze: Squeeze, alphabet: Alphabet) -> Self {
        Self {
            squeeze,
            alphabet,
            wrap: 100,
            readable: false,
            index: false,
            measure_up_to: None,
            buffer_past: BUFFER_PAST,
            buffer_folder: None,
        }
    }

    /// The same fold, allowed to index.
    pub fn indexed(self) -> Self {
        Self { index: true, ..self }
    }

    /// The same fold, measuring whether its index pays, for `limit` at most.
    pub fn measured_up_to(self, limit: Duration) -> Self {
        Self { measure_up_to: Some(limit), ..self }
    }
}

/// How a fold decided whether its index pays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Judged {
    /// By the estimates: the fold wasn't asked to measure, or had nothing to measure, as when it
    /// wasn't asked to index or no list was worth keeping.
    Estimated,
    /// By measuring: the bytes it takes indexed, the index's text included, and unindexed. The
    /// smaller was kept, the unindexed one on a tie.
    Measured { indexed: usize, unindexed: usize },
    /// The measurement didn't finish in the time it was given, so the estimates decided.
    OutOfTime,
}

/// A fold: the folded text, the index to keep beside it, if any, how the index was decided, and
/// the squeeze it was folded with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folded {
    pub text: String,
    pub indexing: Indexing,
    pub judged: Judged,
    /// The squeeze asked for, or the one [`Squeeze::Auto`] chose.
    pub squeeze: Squeeze,
}

/// Inputs folded together: their folded texts, in their order, the one index they share, how it
/// was decided, and the squeeze they were folded with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedAll {
    pub texts: Vec<String>,
    pub indexing: Indexing,
    pub judged: Judged,
    /// The squeeze asked for, or the one [`Squeeze::Auto`] chose.
    pub squeeze: Squeeze,
}

/// `input`, folded.
pub fn fold(input: &[u8], how: &Fold) -> Result<Folded, FoldError> {
    let FoldedAll { mut texts, indexing, judged, squeeze } = fold_all(&[input], how)?;
    Ok(Folded { text: texts.remove(0), indexing, judged, squeeze })
}

/// `inputs`, each folded on its own but into one shared index, kept once for them all: every
/// value any of them lists, sorted. Each input still decides by itself what is worth listing, and
/// one that lists nothing doesn't need the index at all. [`Squeeze::Auto`] chooses one squeeze for
/// them all, the one that folds them smallest together.
pub fn fold_all(inputs: &[&[u8]], how: &Fold) -> Result<FoldedAll, FoldError> {
    let folder = how.buffer_folder.clone().unwrap_or_else(std::env::temp_dir);
    fold_each(inputs, how, &folder).map_err(|(_, error)| error)
}

/// A fold made: its squeeze, its texts, its indexing, and the bytes it costs the caller, its
/// index's text included.
struct Made {
    squeeze: Squeeze,
    texts: Buffered,
    indexing: Indexing,
    size: usize,
}

/// [`fold_all`], buffering in `folder`, and saying which input a fold failed on, if it failed on
/// one rather than on its buffer.
pub(crate) fn fold_each(
    inputs: &[&[u8]],
    how: &Fold,
    folder: &Path,
) -> Result<FoldedAll, (Option<usize>, FoldError)> {
    let on_input = |(at, error)| (Some(at), error);
    // The estimates' fold: with the squeeze asked for, or the smallest of auto's. Each fold is
    // weighed as it is made, and only the smallest so far is kept, buffered while the next is made.
    let (mut smallest, mut failed): (Option<Made>, _) = (None, None);
    for squeeze in how.squeeze.candidates() {
        if let Some(waiting) = &mut smallest {
            waiting.texts.buffer(how.buffer_past, folder);
        }
        match fold_unmeasured(inputs, &squeeze, how, how.index, Deadline::NONE) {
            Ok(made) if smallest.as_ref().is_none_or(|smallest| made.size < smallest.size) => {
                smallest = Some(made);
            }
            Ok(_) => {}
            // A squeeze that doesn't take these inputs: auto goes on to the others, and a squeeze
            // asked for alone fails the fold with this.
            Err(error) => {
                failed.get_or_insert(error);
            }
        }
    }
    let Some(Made { squeeze, mut texts, indexing, size }) = smallest else {
        return Err(on_input(failed.expect("a squeeze was tried")));
    };
    // The measurement: the same squeeze without the index, while the estimates' fold waits.
    let judged = match (how.measure_up_to, &indexing) {
        (Some(limit), Indexing::Index(_)) => {
            let deadline = Deadline::after(limit);
            texts.buffer(how.buffer_past, folder);
            match fold_unmeasured(inputs, &squeeze, how, false, deadline) {
                Ok(plain) if plain.size <= size => {
                    let judged = Judged::Measured { indexed: size, unindexed: plain.size };
                    drop(texts);
                    let texts =
                        plain.texts.take().map_err(|error| (None, FoldError::Buffer(error)))?;
                    return Ok(FoldedAll { texts, indexing: Indexing::Unneeded, judged, squeeze });
                }
                Ok(plain) => Judged::Measured { indexed: size, unindexed: plain.size },
                Err((_, FoldError::OutOfTime)) => Judged::OutOfTime,
                Err(error) => return Err(on_input(error)),
            }
        }
        _ => Judged::Estimated,
    };
    let texts = texts.take().map_err(|error| (None, FoldError::Buffer(error)))?;
    Ok(FoldedAll { texts, indexing, judged, squeeze })
}

/// `inputs`, folded with `squeeze`, indexing or not as `index` says, by the estimates alone, and
/// given up past `deadline`.
fn fold_unmeasured(
    inputs: &[&[u8]],
    squeeze: &Squeeze,
    how: &Fold,
    index: bool,
    deadline: Deadline,
) -> Result<Made, (usize, FoldError)> {
    let squeezed: Vec<squeeze::Squeezed> = inputs
        .iter()
        .enumerate()
        .map(|(at, input)| {
            squeeze::squeeze(input, squeeze, None, index, deadline).map_err(|error| (at, error))
        })
        .collect::<Result<_, _>>()?;
    let values: BTreeSet<String> =
        squeezed.iter().flat_map(|squeezed| squeezed.tables.index_values()).collect();
    let indexing = match (index, values.is_empty()) {
        (false, _) => Indexing::Off,
        (true, true) => Indexing::Unneeded,
        (true, false) => Indexing::Index(Index::new(values).expect("a set holds no value twice")),
    };
    let texts = squeezed
        .into_iter()
        .enumerate()
        .map(|(at, squeezed)| {
            let (name, alphabet, index) = (squeeze.name(), how.alphabet, indexing.index());
            squeeze::render_within(
                name,
                &squeezed,
                alphabet,
                how.wrap,
                how.readable,
                index,
                deadline,
            )
            .map_err(|error| (at, error))
        })
        .collect::<Result<_, _>>()?;
    let texts = Buffered::new(texts);
    let size = texts.size() + indexing.index().map_or(0, |index| index.to_text().len());
    Ok(Made { squeeze: squeeze.clone(), texts, indexing, size })
}

/// A time limit as people write it: a number, then `ms`, `s`, `m`, `h` or `d`, as in `30ms`, `5s`,
/// `1.5h` or `2d`.
pub fn time_limit(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let split = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let seconds_each = match unit {
        "ms" => 0.001,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86_400.0,
        _ => {
            return Err(format!(
                "`{text}` is no time: a number, then ms, s, m, h or d, as in 30ms or 5s"
            ));
        }
    };
    let number: f64 =
        number.parse().map_err(|_| format!("`{text}` has no number before its unit"))?;
    Duration::try_from_secs_f64(number * seconds_each)
        .map_err(|_| format!("`{text}` is no time this can wait"))
}

/// `input`, folded with the tables of `previous`, an earlier fold of an earlier version of it, and
/// with `index`, the index that fold needs, if any. Every line the two have in common folds to the
/// same body line, and the index stays as it was: what the old tables and index don't hold is
/// spelled out instead, a little larger. Fold afresh now and then to bring them up to date. The
/// squeeze must be the one `previous` was folded with, or [`Squeeze::Auto`], which takes that
/// one; the alphabet may differ.
pub fn fold_keeping(
    input: &[u8],
    previous: &str,
    index: Option<&Index>,
    how: &Fold,
) -> Result<String, FoldError> {
    let folded = container::parse(previous).map_err(FoldError::Previous)?;
    let squeeze = match &how.squeeze {
        Squeeze::Auto(table) => {
            Squeeze::named_in(&folded.header, table).map_err(FoldError::Previous)?
        }
        squeeze if squeeze.name() == folded.header.squeeze => squeeze.clone(),
        _ => return Err(FoldError::PreviousSqueeze(folded.header.squeeze)),
    };
    let its_index = squeeze::needed(&folded, index).map_err(FoldError::Previous)?;
    let kept = Some((&folded, its_index));
    let squeezed = squeeze::squeeze(input, &squeeze, kept, how.index, Deadline::NONE)?;
    let name = squeeze.name();
    Ok(squeeze::render(name, &squeezed, how.alphabet, how.wrap, how.readable, its_index))
}

/// What a folded text holds, given the index it was folded with, if it needs one.
pub fn unfold(folded: &str, index: Option<&Index>) -> Result<Vec<u8>, UnfoldError> {
    squeeze::unsqueeze(&container::parse(folded)?, index)
}

/// Whether `folded` needs an index to unfold, and so whether one has to be kept with it. Only its
/// first line is read.
pub fn needs_index(folded: &str) -> Result<bool, UnfoldError> {
    Ok(container::header_of(folded)?.has("index"))
}

/// The squeeze `folded` was folded with, as named in its header, and its alphabet. Only its first
/// line is read.
pub fn folded_with(folded: &str) -> Result<(String, Alphabet), UnfoldError> {
    let header = container::header_of(folded)?;
    Ok((header.squeeze, header.alphabet))
}
