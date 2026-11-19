//! Words from a fixed list, written as their places in it: the README's "words from a fixed list".
//!
//! Most spellings never occur (no weekday is spelled `xqzv`), so a word that can only be one of a
//! known list costs only the bits the list's length needs, however long it is. A [`Record`] of
//! several such fields is one number below the product of their lists' lengths: a weekday and a
//! month are one of 84 pairs, 6.39 bits, rather than 3 + 4 whole ones.
//!
//! The lists are the agreement between the writer and the reader, and they are not in the folded
//! text: they must keep their meaning. Words may be added at a list's end, never removed or
//! reordered, or old text unfolds into other words.
//!
//! ```
//! use orimoji::Alphabet;
//! use orimoji::words::{Record, WordList};
//!
//! let days = WordList::new(["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"])?;
//! let months = WordList::new(["January", "February", "March", "April", "May", "June", "July",
//!     "August", "September", "October", "November", "December"])?;
//! let record = Record::new(vec![&days, &months]);
//! assert_eq!(record.bits(), 7, "84 pairs fit 7 bits");
//! let folded = record.fold(&["Saturday", "November"], Alphabet::Base64Url)?;
//! assert_eq!(folded.len(), 2, "two base64 characters for both words");
//! assert_eq!(record.unfold(&folded, Alphabet::Base64Url)?, ["Saturday", "November"]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::alphabet::Alphabet;
use crate::bignum::Nat;
use crate::bits::BitWriter;
use std::collections::HashMap;
use std::fmt;

/// A fixed list of distinct words.
#[derive(Clone, Debug)]
pub struct WordList {
    words: Vec<String>,
    places: HashMap<String, usize>,
}

impl WordList {
    /// The list of these words, in this order, which is the order their places count in.
    pub fn new<I, S>(words: I) -> Result<Self, WordsError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let words: Vec<String> = words.into_iter().map(Into::into).collect();
        if words.is_empty() {
            return Err(WordsError::EmptyList);
        }
        let mut places = HashMap::with_capacity(words.len());
        for (place, word) in words.iter().enumerate() {
            if places.insert(word.clone(), place).is_some() {
                return Err(WordsError::Twice(word.clone()));
            }
        }
        Ok(Self { words, places })
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Where `word` is in the list.
    pub fn place(&self, word: &str) -> Option<usize> {
        self.places.get(word).copied()
    }

    /// The word at `place`.
    pub fn word(&self, place: usize) -> Option<&str> {
        self.words.get(place).map(String::as_str)
    }
}

/// Fields of a record, each a word from its own list.
#[derive(Clone, Debug)]
pub struct Record<'a> {
    lists: Vec<&'a WordList>,
}

impl<'a> Record<'a> {
    pub fn new(lists: Vec<&'a WordList>) -> Self {
        Self { lists }
    }

    /// The bits a record takes: enough for every combination of its fields.
    pub fn bits(&self) -> usize {
        let mut combinations = Nat::one();
        for list in &self.lists {
            combinations.mul_add(list.len() as u64, 0);
        }
        combinations.decrement();
        combinations.bit_len()
    }

    /// `words`, a word for each field, as one number in `alphabet`.
    pub fn fold(&self, words: &[&str], alphabet: Alphabet) -> Result<String, WordsError> {
        if words.len() != self.lists.len() {
            return Err(WordsError::Fields { expected: self.lists.len(), got: words.len() });
        }
        let mut number = Nat::zero();
        for (list, word) in self.lists.iter().zip(words).rev() {
            let place = list.place(word).ok_or_else(|| WordsError::NotListed(word.to_string()))?;
            number.mul_add(list.len() as u64, place as u64);
        }
        let mut writer = BitWriter::new();
        for index in (0..self.bits()).rev() {
            writer.bit(number.bit(index));
        }
        Ok(alphabet.fold_bits(&writer.finish()))
    }

    /// The words [`Record::fold`] folded into `text`.
    pub fn unfold(&self, text: &str, alphabet: Alphabet) -> Result<Vec<&'a str>, WordsError> {
        let bits =
            alphabet.unfold_bits(text).map_err(|error| WordsError::Unfold(error.to_string()))?;
        let mut reader = bits.reader();
        let mut number = Nat::zero();
        for _ in 0..self.bits() {
            number.mul_add(2, reader.bit().ok_or(WordsError::TooShort)? as u64);
        }
        let mut words = Vec::with_capacity(self.lists.len());
        for list in &self.lists {
            let place = number.div_rem(list.len() as u64) as usize;
            words.push(list.word(place).expect("a remainder is a place"));
        }
        match number.is_zero() {
            true => Ok(words),
            false => Err(WordsError::OutOfRange),
        }
    }
}

/// Why words couldn't be folded or unfolded.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WordsError {
    EmptyList,
    Twice(String),
    NotListed(String),
    Fields { expected: usize, got: usize },
    Unfold(String),
    TooShort,
    OutOfRange,
}

impl fmt::Display for WordsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WordsError::EmptyList => write!(f, "a word list needs words"),
            WordsError::Twice(word) => write!(f, "`{word}` is in the list twice"),
            WordsError::NotListed(word) => write!(f, "`{word}` is not in its list"),
            WordsError::Fields { expected, got } => {
                write!(f, "{got} words for a record of {expected}")
            }
            WordsError::Unfold(why) => write!(f, "{why}"),
            WordsError::TooShort => write!(f, "too few characters for a record"),
            WordsError::OutOfRange => write!(f, "a number past every combination of the lists"),
        }
    }
}

impl std::error::Error for WordsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_folds_and_unfolds() {
        let a = WordList::new(["x", "y", "z"]).unwrap();
        let b = WordList::new(["true", "false"]).unwrap();
        let record = Record::new(vec![&a, &b, &a]);
        assert_eq!(record.bits(), 5, "18 combinations");
        for first in ["x", "y", "z"] {
            for second in ["true", "false"] {
                for third in ["x", "y", "z"] {
                    let words = [first, second, third];
                    let folded = record.fold(&words, Alphabet::Crockford).unwrap();
                    assert_eq!(record.unfold(&folded, Alphabet::Crockford).unwrap(), words);
                }
            }
        }
    }

    #[test]
    fn a_word_off_the_list_or_a_list_twice_over_is_refused() {
        assert_eq!(WordList::new(["a", "a"]).unwrap_err(), WordsError::Twice("a".into()));
        let list = WordList::new(["a", "b"]).unwrap();
        let record = Record::new(vec![&list]);
        assert_eq!(
            record.fold(&["c"], Alphabet::Hex).unwrap_err(),
            WordsError::NotListed("c".into())
        );
        assert!(matches!(record.fold(&["a", "b"], Alphabet::Hex), Err(WordsError::Fields { .. })));
    }
}
