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
//! The folded text names both in its first line and holds whatever else it needs, so [`unfold`]
//! needs nothing else. The text squeeze folds into an `.ori` ([`ori`]), any other into a folded
//! file:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Indexing, Squeeze};
//!
//! let listing = "app_id\tname\n10\tCounter-Strike\n20\tTeam Fortress Classic\n";
//! let folded = orimoji::fold(listing.as_bytes(), &Fold::new(Squeeze::Text, Alphabet::Base64Url))?;
//! assert!(folded.text.starts_with("=== orimoji 3 text base64url ==="));
//! assert_eq!(folded.indexing, Indexing::Off);
//! assert_eq!(orimoji::unfold(&folded.text, None)?, listing.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Indexes
//!
//! Asked to, the table and text squeezes index: a column's common values, and common words and
//! terminal escapes, are written as their places in a list, the [`Index`]. [`Indexing`] says what
//! became of it: not asked for, asked for but not worth keeping, or the index. It is kept with the
//! code, inside the folded text, so the text still unfolds alone:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
//!
//! let studios = ["Valve", "Gearbox Software", "Bethesda"];
//! let rows: String = (0..300).map(|i| format!("{i}\t{}\n", studios[i % 3])).collect();
//! let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
//! let folded = orimoji::fold(rows.as_bytes(), &how)?;
//! let index = folded.indexing.index().expect("three studios are worth listing");
//! assert_eq!(index.values(), ["Bethesda", "Gearbox Software", "Valve"]);
//! assert!(!orimoji::needs_index(&folded.text)?, "it is inside");
//! assert_eq!(orimoji::unfold(&folded.text, None)?, rows.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Folds of orimoji's first format kept it beside the text instead, for the caller to keep and to
//! give back; [`Fold::index_inside`] off still folds that way:
//!
//! ```
//! # use orimoji::{Alphabet, Fold, Index, Squeeze, TableOptions};
//! # let studios = ["Valve", "Gearbox Software", "Bethesda"];
//! # let rows: String = (0..300).map(|i| format!("{i}\t{}\n", studios[i % 3])).collect();
//! let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
//! let folded = orimoji::fold(rows.as_bytes(), &Fold { index_inside: false, ..how })?;
//! let index = folded.indexing.index().expect("the same three");
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
//! [`files::fold_files`] and [`files::unfold_files`] do the same to files, exactly as the command
//! line does: in place, or beside them with a suffix, as the caller's [`Output`] says. Text files
//! folded together share one unit, in an `.orinv` beside them ([`ori`]); files of any other
//! squeeze fold each on its own.
//!
//! # Parts
//!
//! A program with a file format of its own folds into [`parts`] instead: the lines, and their
//! [`parts::Unit`] (the choices the fold made, the code table the lines are spelled in, and the
//! index), as values for it to keep where it suits, smaller than a folded file and with no header
//! of their own. The unit's three always go together.

pub mod alphabet;
pub mod bits;
pub mod files;
pub mod huffman;
pub mod index;
pub mod measure;
pub mod ori;
pub mod parts;
pub mod squeeze;
pub mod words;

mod bignum;
mod buffer;
mod container;
mod deadline;
mod error;
mod escape;
mod sharing;

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
    /// Whether a folded text that needs an index keeps it inside, in format 2, so that it unfolds
    /// alone and the two are never parted: on, the default, and always so for files orimoji
    /// writes. Off, the index comes back beside the text, to be kept and given back, as orimoji's
    /// format 1 did. (The text squeeze's parts keep theirs in their unit, see [`parts`].)
    pub index_inside: bool,
    /// Where files folded together keep the `.orinv` they share, when [`files`] writes them (the
    /// text squeeze's, see [`ori`]): `None`, the default, is beside the first of them, named after
    /// its folder.
    pub orinv_dir: Option<PathBuf>,
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
            index_inside: true,
            orinv_dir: None,
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
    /// By folding both with the index and without, as the text squeeze's `.ori` always does when
    /// it may index: the smaller was kept.
    Weighed,
}

/// A fold: the folded text; what became of its index, kept inside the text or, with
/// [`Fold::index_inside`] off, to be kept beside it; how the index was decided; and the squeeze it
/// was folded with.
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

/// `input`, folded. The text squeeze folds into an `.ori` (see [`ori`]), its unit inside it, as
/// [`Squeeze::Auto`] does when it chooses the text squeeze; with [`Fold::index_inside`] off, into
/// the first format, as every squeeze does.
pub fn fold(input: &[u8], how: &Fold) -> Result<Folded, FoldError> {
    let FoldedAll { mut texts, indexing, judged, squeeze } = fold_all(&[input], how)?;
    Ok(Folded { text: texts.remove(0), indexing, judged, squeeze })
}

/// `inputs`, each folded on its own but with one shared index: every value any of them lists,
/// sorted. Each input still decides by itself what is worth listing, and one that lists nothing
/// doesn't need the index at all. Each text that needs it keeps it inside; with
/// [`Fold::index_inside`] off, it is kept once for them all, beside them, as the first format kept
/// it. [`Squeeze::Auto`] chooses one squeeze for them all, the one that folds them smallest
/// together. One input folds as [`fold`] folds it; several share an index only as folded files
/// of the first two formats can, the text squeeze's too. Texts that share their code as well,
/// kept once, are [`parts::fold_together`]'s and [`parts::fold_grouped`]'s, and [`files`] writes
/// them as the command line does.
pub fn fold_all(inputs: &[&[u8]], how: &Fold) -> Result<FoldedAll, FoldError> {
    let folder = how.buffer_folder.clone().unwrap_or_else(std::env::temp_dir);
    fold_each(inputs, how, &folder).map_err(|(_, error)| error)
}

/// A fold made: its squeeze, its texts, its indexing, the bytes it costs the caller, its index's
/// text included, and whether it weighed its index itself, folding both ways, as an `.ori` does.
struct Made {
    squeeze: Squeeze,
    texts: Buffered,
    indexing: Indexing,
    size: usize,
    weighed: bool,
}

/// `input` folded alone into an `.ori` by the text squeeze, its unit inside it.
fn fold_ori(input: &[u8], how: &Fold) -> Result<Made, FoldError> {
    let text = std::str::from_utf8(input)
        .map_err(|error| FoldError::NotText { at: error.valid_up_to() })?;
    let folded = parts::fold(text, how.alphabet, how.index);
    let whole = ori::whole(&folded.lines, &folded.unit, how.alphabet);
    let indexing = match (how.index, folded.unit.index) {
        (_, Some(index)) => Indexing::Index(index),
        (true, None) => Indexing::Unneeded,
        (false, None) => Indexing::Off,
    };
    let size = whole.len();
    let texts = Buffered::new(vec![whole]);
    Ok(Made { squeeze: Squeeze::Text, texts, indexing, size, weighed: how.index })
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
    // One input folds by the text squeeze into an `.ori`, whose unit goes with its lines; several
    // share an index, as only the first two formats can.
    let ori = inputs.len() == 1 && how.index_inside;
    for squeeze in how.squeeze.candidates() {
        if let Some(waiting) = &mut smallest {
            waiting.texts.buffer(how.buffer_past, folder);
        }
        let made = match (&squeeze, ori) {
            (Squeeze::Text, true) => fold_ori(inputs[0], how).map_err(|error| (0, error)),
            _ => fold_unmeasured(inputs, &squeeze, how, how.index, Deadline::NONE),
        };
        match made {
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
    let Some(Made { squeeze, mut texts, indexing, size, weighed }) = smallest else {
        return Err(on_input(failed.expect("a squeeze was tried")));
    };
    // The measurement: the same squeeze without the index, while the estimates' fold waits.
    let judged = match (how.measure_up_to, &indexing) {
        _ if weighed => Judged::Weighed,
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
                how.index_inside,
                deadline,
            )
            .map_err(|error| (at, error))
        })
        .collect::<Result<_, _>>()?;
    let texts = Buffered::new(texts);
    // An index kept inside is in the texts already; one kept beside them costs its own text.
    let beside = indexing.index().filter(|_| !how.index_inside);
    let size = texts.size() + beside.map_or(0, |index| index.to_text().len());
    Ok(Made { squeeze: squeeze.clone(), texts, indexing, size, weighed: false })
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
    if ori::is_ori(previous) {
        return keeping_ori(input, previous, how);
    }
    let folded = container::parse(previous).map_err(FoldError::Previous)?;
    let squeeze = match &how.squeeze {
        Squeeze::Auto(table) => {
            Squeeze::named_in(&folded.header, table).map_err(FoldError::Previous)?
        }
        squeeze if squeeze.name() == folded.header.squeeze => squeeze.clone(),
        _ => return Err(FoldError::PreviousSqueeze(folded.header.squeeze)),
    };
    let its_index = squeeze::needed(&folded, index).map_err(FoldError::Previous)?;
    let kept = Some((&folded, its_index.as_ref()));
    let squeezed = squeeze::squeeze(input, &squeeze, kept, how.index, Deadline::NONE)?;
    let (name, alphabet) = (squeeze.name(), how.alphabet);
    let (wrap, readable, inside) = (how.wrap, how.readable, how.index_inside);
    Ok(squeeze::render(name, &squeezed, alphabet, wrap, readable, its_index.as_ref(), inside))
}

/// [`fold_keeping`] of an `.ori`: `input` folded with the unit of `previous`, in its alphabet,
/// into an `.ori` holding it. The unit of an `.ori` that names an `.orinv` is in that file, which
/// [`files::fold_files_like`] reads.
fn keeping_ori(input: &[u8], previous: &str, how: &Fold) -> Result<String, FoldError> {
    if !matches!(how.squeeze, Squeeze::Auto(_) | Squeeze::Text) {
        return Err(FoldError::PreviousSqueeze(ori::SQUEEZE.to_string()));
    }
    let text = std::str::from_utf8(input)
        .map_err(|error| FoldError::NotText { at: error.valid_up_to() })?;
    let (unit, alphabet) = match ori::read(previous).map_err(FoldError::Previous)? {
        ori::OriFile::Whole { unit, alphabet, .. } | ori::OriFile::Unit { unit, alphabet } => {
            (unit, alphabet)
        }
        ori::OriFile::Apart { orinv, .. } => {
            return Err(FoldError::Previous(UnfoldError::NeedsOrinv(orinv)));
        }
    };
    let lines = parts::fold_keeping(text, &unit, alphabet)?;
    Ok(ori::whole(&lines, &unit, alphabet))
}

/// What a folded text holds, given the index it was folded with, if it needs one and doesn't keep
/// it inside. An `.ori` of the text squeeze unfolds here too, when it holds its unit; one whose
/// lines were folded together with other files' unfolds with [`ori::unfold`], given their unit.
pub fn unfold(folded: &str, index: Option<&Index>) -> Result<Vec<u8>, UnfoldError> {
    if ori::is_ori(folded) {
        return ori::unfold(folded, None, usize::MAX).map(String::into_bytes);
    }
    squeeze::unsqueeze(&container::parse(folded)?, index)
}

/// Whether `folded` needs an index given to unfold, and so whether one has to be kept with it:
/// whether it was folded with one and keeps it beside it, as format 1 did, rather than inside.
/// Only its first line is read.
pub fn needs_index(folded: &str) -> Result<bool, UnfoldError> {
    let header = container::header_of(folded)?;
    Ok(header.version == 1 && header.has("index"))
}

/// The squeeze `folded` was folded with, as named in its header, and its alphabet. Only its first
/// line is read.
pub fn folded_with(folded: &str) -> Result<(String, Alphabet), UnfoldError> {
    let header = container::header_of(folded)?;
    Ok((header.squeeze, header.alphabet))
}
