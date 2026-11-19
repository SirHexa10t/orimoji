//! Fold text and data into fewer characters, fewer bytes or narrower lines, and unfold it back
//! exactly. *Ori* (折り) is the fold of origami; *moji* (文字), the character of emoji.
//!
//! A fold is two steps:
//!
//! - a [`Squeeze`] makes the data smaller: packing, a Huffman code, a table's column models,
//!   deflate or zstd, or nothing;
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

pub mod alphabet;
pub mod bits;
pub mod files;
pub mod huffman;
pub mod index;
pub mod measure;
pub mod squeeze;
pub mod words;

mod bignum;
mod container;
mod error;
mod escape;

pub use alphabet::Alphabet;
pub use error::{FoldError, UnfoldError};
pub use files::Output;
pub use index::{Index, Indexing};
pub use squeeze::{Squeeze, TableOptions};

use std::collections::BTreeSet;

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
}

impl Fold {
    pub fn new(squeeze: Squeeze, alphabet: Alphabet) -> Self {
        Self { squeeze, alphabet, wrap: 100, readable: false, index: false }
    }

    /// The same fold, allowed to index.
    pub fn indexed(self) -> Self {
        Self { index: true, ..self }
    }
}

/// A fold: the folded text, and the index to keep beside it, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folded {
    pub text: String,
    pub indexing: Indexing,
}

/// Inputs folded together: their folded texts, in their order, and the one index they share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedAll {
    pub texts: Vec<String>,
    pub indexing: Indexing,
}

/// `input`, folded.
pub fn fold(input: &[u8], how: &Fold) -> Result<Folded, FoldError> {
    let FoldedAll { mut texts, indexing } = fold_all(&[input], how)?;
    Ok(Folded { text: texts.remove(0), indexing })
}

/// `inputs`, each folded on its own but into one shared index, kept once for them all: every
/// value any of them lists, sorted. Each input still decides by itself what is worth listing, and
/// one that lists nothing doesn't need the index at all.
pub fn fold_all(inputs: &[&[u8]], how: &Fold) -> Result<FoldedAll, FoldError> {
    fold_each(inputs, how).map_err(|(_, error)| error)
}

/// [`fold_all`], saying which input a fold failed on.
pub(crate) fn fold_each(inputs: &[&[u8]], how: &Fold) -> Result<FoldedAll, (usize, FoldError)> {
    let squeezed: Vec<squeeze::Squeezed> = inputs
        .iter()
        .enumerate()
        .map(|(at, input)| {
            squeeze::squeeze(input, &how.squeeze, None, how.index).map_err(|error| (at, error))
        })
        .collect::<Result<_, _>>()?;
    let values: BTreeSet<String> =
        squeezed.iter().flat_map(|squeezed| squeezed.tables.index_values()).collect();
    let indexing = match (how.index, values.is_empty()) {
        (false, _) => Indexing::Off,
        (true, true) => Indexing::Unneeded,
        (true, false) => Indexing::Index(Index::new(values).expect("a set holds no value twice")),
    };
    let texts = squeezed
        .iter()
        .map(|squeezed| {
            let name = how.squeeze.name();
            squeeze::render(name, squeezed, how.alphabet, how.wrap, how.readable, indexing.index())
        })
        .collect();
    Ok(FoldedAll { texts, indexing })
}

/// `input`, folded with the tables of `previous`, an earlier fold of an earlier version of it, and
/// with `index`, the index that fold needs, if any. Every line the two have in common folds to the
/// same body line, and the index stays as it was: what the old tables and index don't hold is
/// spelled out instead, a little larger. Fold afresh now and then to bring them up to date. The
/// squeeze must be the one `previous` was folded with; the alphabet may differ.
pub fn fold_keeping(
    input: &[u8],
    previous: &str,
    index: Option<&Index>,
    how: &Fold,
) -> Result<String, FoldError> {
    let folded = container::parse(previous).map_err(FoldError::Previous)?;
    if folded.header.squeeze != how.squeeze.name() {
        return Err(FoldError::PreviousSqueeze(folded.header.squeeze));
    }
    let its_index = squeeze::needed(&folded, index).map_err(FoldError::Previous)?;
    let squeezed = squeeze::squeeze(input, &how.squeeze, Some((&folded, its_index)), how.index)?;
    let name = how.squeeze.name();
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

/// The line squeeze that folds `input` smallest into `alphabet`, index included, indexing or not:
/// of a table (where its lines are rows), text, pack and deflate leaning on the line above. Each
/// is tried, so this takes as long as the four folds.
pub fn pick(input: &[u8], alphabet: Alphabet, index: bool) -> Squeeze {
    let candidates = [
        Squeeze::Table(TableOptions::default()),
        Squeeze::Text,
        Squeeze::Pack,
        Squeeze::Deflate { lean: Some(1) },
    ];
    candidates
        .into_iter()
        .filter_map(|squeeze| {
            let how = Fold { index, ..Fold::new(squeeze.clone(), alphabet) };
            let folded = fold(input, &how).ok()?;
            let size = folded.text.len()
                + folded.indexing.index().map_or(0, |index| index.to_text().len());
            Some((size, squeeze))
        })
        .min_by_key(|(size, _)| *size)
        .map_or(Squeeze::None { whole: false }, |(_, squeeze)| squeeze)
}
