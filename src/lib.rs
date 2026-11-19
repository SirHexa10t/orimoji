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
//! The folded file names both in its first line, so [`unfold`] needs nothing else:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Squeeze};
//!
//! let listing = "app_id\tname\n10\tCounter-Strike\n20\tTeam Fortress Classic\n";
//! let folded = orimoji::fold(listing.as_bytes(), &Fold::new(Squeeze::Text, Alphabet::Base64Url))?;
//! assert!(folded.starts_with("=== orimoji 1 text base64url ==="));
//! assert_eq!(orimoji::unfold(&folded)?, listing.as_bytes());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! # Folds that diff
//!
//! The line squeezes ([`Squeeze::by_line`]) fold every line into a line of its own, so a folded
//! file kept in git diffs like the original. Their tables come from the whole file, though, and a
//! rebuilt table can change every line. [`fold_keeping`] folds a new version with an older fold's
//! tables instead, so only the lines that changed change:
//!
//! ```
//! use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
//!
//! let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url);
//! let before = orimoji::fold(b"10\tValve\n20\tValve\n30\tGearbox", &how)?;
//! let after = orimoji::fold_keeping(b"10\tValve\n20\tValve\n30\tGearbox\n40\tNew Studio", &before, &how)?;
//! let (old, new): (Vec<_>, Vec<_>) = (before.lines().collect(), after.lines().collect());
//! assert_eq!(old[old.len() - 3..], new[new.len() - 4..new.len() - 1], "the old rows fold the same");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod alphabet;
pub mod bits;
pub mod huffman;
pub mod measure;
pub mod squeeze;
pub mod words;

mod bignum;
mod container;
mod error;
mod escape;

pub use alphabet::Alphabet;
pub use error::{FoldError, UnfoldError};
pub use squeeze::{Squeeze, TableOptions};

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
}

impl Fold {
    pub fn new(squeeze: Squeeze, alphabet: Alphabet) -> Self {
        Self { squeeze, alphabet, wrap: 100, readable: false }
    }
}

/// `input`, folded.
pub fn fold(input: &[u8], how: &Fold) -> Result<String, FoldError> {
    let squeezed = squeeze::squeeze(input, &how.squeeze, None)?;
    Ok(squeeze::render(how.squeeze.name(), &squeezed, how.alphabet, how.wrap, how.readable))
}

/// `input`, folded with the tables of `previous`, an earlier fold of an earlier version of it, so
/// that every line the two have in common folds to the same body line. What the old tables don't
/// hold is spelled out instead, a little larger; fold afresh now and then to bring them up to
/// date. The squeeze must be the one `previous` was folded with; the alphabet may differ.
pub fn fold_keeping(input: &[u8], previous: &str, how: &Fold) -> Result<String, FoldError> {
    let folded = container::parse(previous).map_err(FoldError::Previous)?;
    if folded.header.squeeze != how.squeeze.name() {
        return Err(FoldError::PreviousSqueeze(folded.header.squeeze));
    }
    let squeezed = squeeze::squeeze(input, &how.squeeze, Some(&folded))?;
    Ok(squeeze::render(how.squeeze.name(), &squeezed, how.alphabet, how.wrap, how.readable))
}

/// What a folded file holds.
pub fn unfold(folded: &str) -> Result<Vec<u8>, UnfoldError> {
    squeeze::unsqueeze(&container::parse(folded)?)
}

/// The squeeze `folded` was folded with, as named in its header, and its alphabet.
pub fn folded_with(folded: &str) -> Result<(String, Alphabet), UnfoldError> {
    let parsed = container::parse(folded)?;
    Ok((parsed.header.squeeze, parsed.header.alphabet))
}

/// The line squeeze that folds `input` smallest into `alphabet`: of a table (where its lines are
/// rows), text, pack and deflate leaning on the line above. Each is tried, so this takes as long
/// as the four folds.
pub fn pick(input: &[u8], alphabet: Alphabet) -> Squeeze {
    let candidates = [
        Squeeze::Table(TableOptions::default()),
        Squeeze::Text,
        Squeeze::Pack,
        Squeeze::Deflate { lean: Some(1) },
    ];
    candidates
        .into_iter()
        .filter_map(|squeeze| {
            let size = fold(input, &Fold::new(squeeze.clone(), alphabet)).ok()?.len();
            Some((size, squeeze))
        })
        .min_by_key(|(size, _)| *size)
        .map_or(Squeeze::None { whole: false }, |(_, squeeze)| squeeze)
}
