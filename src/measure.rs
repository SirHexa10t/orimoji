//! What each squeeze and alphabet makes of an input: characters, bytes, screen width, and the bytes
//! git would store for it, since git keeps every object zlib-compressed. An indexed fold is
//! measured with its index, which the caller has to keep too. `orimoji measure` prints
//! it; the tables in docs/observations.md were made the same way.

use crate::alphabet::Alphabet;
use crate::deadline::Deadline;
use crate::index::Index;
use crate::squeeze::{self, Squeeze, TableOptions};
use std::collections::BTreeSet;

/// One fold of the input, measured.
#[derive(Clone, Debug)]
pub struct Measurement {
    /// The squeeze, or `None` for the input itself.
    pub squeeze: Option<Squeeze>,
    /// Whether the squeeze was allowed to index (see [`crate::Fold::index`]).
    pub index: bool,
    pub alphabet: Option<Alphabet>,
    pub chars: usize,
    pub bytes: usize,
    /// Screen cells, roughly: CJK and the like count two.
    pub cells: usize,
    /// What git would store: zlib at its default level.
    pub in_git: usize,
}

impl Measurement {
    fn of(text: &[u8], squeeze: Option<Squeeze>, index: bool, alphabet: Option<Alphabet>) -> Self {
        let as_text = String::from_utf8_lossy(text);
        Self {
            squeeze,
            index,
            alphabet,
            chars: as_text.chars().count(),
            bytes: text.len(),
            cells: cells(&as_text),
            in_git: squeeze::deflate::git_size(text),
        }
    }
}

/// The squeezes measured: every line squeeze that takes this input, then the whole-file ones.
pub fn squeezes(input: &[u8]) -> Vec<Squeeze> {
    let mut squeezes = Vec::new();
    if std::str::from_utf8(input).is_ok() {
        squeezes.extend([Squeeze::Table(TableOptions::default()), Squeeze::Text, Squeeze::Pack]);
    }
    squeezes.extend([
        Squeeze::None { whole: false },
        Squeeze::Deflate { lean: Some(1) },
        Squeeze::Deflate { lean: None },
    ]);
    if cfg!(feature = "zstd") {
        squeezes.push(Squeeze::Zstd { level: 19 });
    }
    squeezes
}

/// The input itself, then every squeeze that takes it in every one of `alphabets`, measured; the
/// squeezes that can index, both without and with.
pub fn measure(input: &[u8], alphabets: &[Alphabet]) -> Vec<Measurement> {
    let mut out = vec![Measurement::of(input, None, false, None)];
    for squeeze in squeezes(input) {
        let indexing: &[bool] = if squeeze.has_tables() { &[false, true] } else { &[false] };
        for &index in indexing {
            let Ok(squeezed) = squeeze::squeeze(input, &squeeze, None, index, Deadline::NONE)
            else {
                continue;
            };
            let values: BTreeSet<String> = squeezed.tables.index_values().into_iter().collect();
            let kept = (!values.is_empty())
                .then(|| Index::new(values).expect("a set holds no value twice"));
            for &alphabet in alphabets {
                // What the caller keeps: the folded text, and the index beside it.
                let mut folded =
                    squeeze::render(squeeze.name(), &squeezed, alphabet, 100, false, kept.as_ref());
                folded.extend(kept.as_ref().map(Index::to_text));
                let squeeze = Some(squeeze.clone());
                out.push(Measurement::of(folded.as_bytes(), squeeze, index, Some(alphabet)));
            }
        }
    }
    out
}

/// The cells a terminal draws `text` in, roughly: two for East Asian wide characters and emoji,
/// none for combining marks and zero-width ones, and none for a newline.
pub fn cells(text: &str) -> usize {
    text.chars()
        .map(|c| match c as u32 {
            0x0A | 0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
            0x1100..=0x115F
            | 0x2E80..=0x303E
            | 0x3041..=0x33FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xA000..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD => 2,
            _ => 1,
        })
        .sum()
}
