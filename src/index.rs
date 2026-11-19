//! An index: the values a fold writes as their places in a list, handed back to the caller to keep
//! wherever suits it (beside the folded text, inside a file of its own format, or once for many
//! folded files) and to give back when unfolding.
//!
//! A folded text that needs an index says so in its header, with how many of the index's values
//! it was folded with and a fingerprint of them. Unfolding without the index, or with another one,
//! is refused; an index that only had values added at its end still fits.
//!
//! As text, an index is a header line and then a value a line, escaped:
//!
//! ```text
//! === orimoji index 1 ===
//! Bethesda
//! Gearbox Software
//! Valve
//! ```

use crate::error::UnfoldError;
use crate::escape::{escape, unescape};
use std::collections::HashMap;

const FIRST_LINE: &str = "=== orimoji index 1 ===";

/// The values a fold wrote as their places, in their order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    values: Vec<String>,
    places: HashMap<String, usize>,
}

impl Index {
    /// The index of these values, in this order. A value may be in it only once.
    pub fn new<I, S>(values: I) -> Result<Self, UnfoldError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let values: Vec<String> = values.into_iter().map(Into::into).collect();
        let mut places = HashMap::with_capacity(values.len());
        for (place, value) in values.iter().enumerate() {
            if places.insert(value.clone(), place).is_some() {
                return Err(UnfoldError::Index(format!("`{value}` is in it twice")));
            }
        }
        Ok(Self { values, places })
    }

    /// Its values, in their order.
    pub fn values(&self) -> &[String] {
        &self.values
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Where `value` is in it.
    pub fn place(&self, value: &str) -> Option<usize> {
        self.places.get(value).copied()
    }

    /// The value at `place`.
    pub fn value(&self, place: usize) -> Option<&str> {
        self.values.get(place).map(String::as_str)
    }

    /// It as text, to keep: a header line, then a value a line.
    pub fn to_text(&self) -> String {
        let mut text = String::with_capacity(
            self.values.iter().map(|value| value.len() + 1).sum::<usize>() + 32,
        );
        text.push_str(FIRST_LINE);
        text.push('\n');
        for value in &self.values {
            text.push_str(&escape(value));
            text.push('\n');
        }
        text
    }

    /// The index [`Index::to_text`] wrote.
    pub fn from_text(text: &str) -> Result<Self, UnfoldError> {
        let text = text.strip_suffix('\n').unwrap_or(text);
        let mut lines = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line));
        if lines.next() != Some(FIRST_LINE) {
            return Err(UnfoldError::Index(format!("its first line isn't `{FIRST_LINE}`")));
        }
        let values: Result<Vec<String>, String> = lines.map(unescape).collect();
        Self::new(values.map_err(UnfoldError::Index)?)
    }

    /// What a folded text's header says of the index it needs: how many values, and their
    /// fingerprint.
    pub(crate) fn mark(&self) -> String {
        format!("{}.{:08x}", self.values.len(), fingerprint(&self.values))
    }

    /// The fingerprint of its first `count` values, which a fold into parts records with how many
    /// values it was folded with.
    pub(crate) fn fingerprint_of(&self, count: usize) -> u32 {
        fingerprint(&self.values[..count.min(self.values.len())])
    }

    /// Whether this is the index `mark` was written for, or that index with values added at its
    /// end.
    pub(crate) fn fits(&self, mark: &str) -> bool {
        let Some((count, print)) = mark.split_once('.') else { return false };
        match (count.parse::<usize>(), u32::from_str_radix(print, 16)) {
            (Ok(count), Ok(print)) => {
                count <= self.len() && fingerprint(&self.values[..count]) == print
            }
            _ => false,
        }
    }
}

/// How a table writes `value`: as its place in `index` (`@3`), if it is there, otherwise escaped.
pub(crate) fn write_entry(value: &str, index: Option<&Index>) -> String {
    match index.and_then(|index| index.place(value)) {
        Some(place) => format!("@{place}"),
        None => escape(value),
    }
}

/// What a table entry [`write_entry`] wrote stands for.
pub(crate) fn read_entry(entry: &str, index: Option<&Index>) -> Result<String, String> {
    let Some(place) = entry.strip_prefix('@') else { return unescape(entry) };
    let place: usize = place.parse().map_err(|_| format!("`{entry}` is no place in an index"))?;
    let index =
        index.ok_or_else(|| format!("`{entry}` is a place in an index, and there is none"))?;
    index
        .value(place)
        .map(str::to_string)
        .ok_or_else(|| format!("`{entry}` is past the index's end"))
}

/// FNV-1a over the values, each followed by a newline.
fn fingerprint(values: &[String]) -> u32 {
    let mut hash = 0x811c_9dc5u32;
    for byte in values.iter().flat_map(|value| value.as_bytes().iter().chain(b"\n").copied()) {
        hash = (hash ^ byte as u32).wrapping_mul(0x0100_0193);
    }
    hash
}

/// What became of the index a fold was asked for, and so what the caller has to keep.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Indexing {
    /// The fold wasn't asked to index: there is no index, and the folded text unfolds alone.
    Off,
    /// It was asked to, but no list paid for itself twice over: there is no index to keep, and the
    /// folded text unfolds alone.
    Unneeded,
    /// The index the folded text needs: keep it, and pass it to unfold.
    Index(Index),
}

impl Indexing {
    /// The index to keep, if there is one.
    pub fn index(&self) -> Option<&Index> {
        match self {
            Indexing::Index(index) => Some(index),
            _ => None,
        }
    }

    /// The index to keep, if there is one.
    pub fn into_index(self) -> Option<Index> {
        match self {
            Indexing::Index(index) => Some(index),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_index_reads_back_from_its_text() {
        let index = Index::new(["Valve", "", "tab\there", "@home", "<end>", "trailing "]).unwrap();
        let text = index.to_text();
        assert!(text.starts_with("=== orimoji index 1 ===\nValve\n"));
        assert_eq!(Index::from_text(&text).unwrap(), index);
        assert_eq!(index.place("tab\there"), Some(2));
        assert_eq!(index.value(4), Some("<end>"));
    }

    #[test]
    fn a_mark_fits_its_index_and_that_index_grown_at_its_end() {
        let index = Index::new(["a", "b"]).unwrap();
        let mark = index.mark();
        assert!(index.fits(&mark));
        assert!(Index::new(["a", "b", "c"]).unwrap().fits(&mark), "values added at the end");
        assert!(!Index::new(["b", "a"]).unwrap().fits(&mark), "reordered");
        assert!(!Index::new(["a"]).unwrap().fits(&mark), "shorter");
        assert!(!index.fits("garbage"));
    }

    #[test]
    fn a_value_twice_or_a_foreign_first_line_is_refused() {
        assert!(Index::new(["x", "x"]).is_err());
        assert!(Index::from_text("x\ny\n").is_err());
    }
}
