//! Which texts of a set folded together would fold smaller alone: an estimate of what each one
//! saves the whole by leaving, for [`crate::parts::fold_grouped`] to choose which to try folding
//! apart. What it then keeps is decided by folding, not by this.
//!
//! A text that shares a code pays twice and saves once. Its own lines are spelled in a code learnt
//! over everyone's, which suits them worse than its own would; and its symbols take room in that
//! code, so everyone else's lines are spelled longer. It saves the unit it would keep alone, less
//! what it adds to the shared one: the characters and words only it uses, and the words that pay
//! for their place in the index only with it.
//!
//! Its own lines, shared and alone, and its unit alone are measured, folded; what it does to the
//! others is estimated, from counts. The set's counts less its own are what the others spell
//! without it, so each text is weighed against the others and never against a set it is part of:
//! two texts with nothing in common don't each find the set half like them.

use crate::alphabet::Alphabet;
use crate::escape::escape;
use crate::squeeze::text::{Tally, worth_a_symbol};
use std::collections::{HashMap, HashSet};

/// A set of texts folded together, as the estimate needs it.
pub(crate) struct Set<'a> {
    /// The texts.
    pub(crate) texts: Vec<&'a str>,
    /// The words and escapes their code spells as one symbol each: their index.
    pub(crate) known: HashSet<&'a str>,
    /// The bits their code's table takes.
    pub(crate) table_bits: f64,
    /// The bytes each text's lines take, spelled in their code.
    pub(crate) lines: Vec<usize>,
}

/// What each text of `set` saves the whole, in bytes, by leaving it to fold alone in `alone[i]`
/// bytes, its unit and what keeping it costs besides included, where keeping a text in the set
/// costs `sharing` bytes besides its lines: positive where leaving saves.
pub(crate) fn savings(
    set: &Set<'_>,
    alone: &[usize],
    sharing: usize,
    alphabet: Alphabet,
) -> Vec<f64> {
    let tallies: Vec<Tally<'_>> =
        set.texts.iter().map(|text| Tally::of(text, &set.known)).collect();
    let mut all: HashMap<&str, u64> = HashMap::new();
    let (mut runs, mut ends) = (0u64, 0u64);
    for tally in &tallies {
        for (&unit, &count) in &tally.units {
            *all.entry(unit).or_default() += count;
        }
        runs += tally.runs;
        ends += tally.ends;
    }
    let symbols = all.values().sum::<u64>() + runs + ends;
    // A bit of a line or of the table, in bytes of the alphabet.
    let byte = alphabet.bytes_per_char() / alphabet.bits_per_char();
    // A table entry's bits, on average: `<end>`, `<esc>` and `<run>` have entries too.
    let entry = set.table_bits / (all.len() + 3) as f64;
    // The ideal code length of a symbol that is `count` of `total`.
    let length = |count: u64, total: u64| (total as f64 / count as f64).log2();
    // What a character of a line costs on average, as a word spelt out costs each of its letters.
    let (letters, letter_bits) = all.iter().filter(|(unit, _)| unit.chars().nth(1).is_none()).fold(
        (0u64, 0.0),
        |(letters, bits), (_, &count)| {
            (letters + count, bits + count as f64 * length(count, symbols))
        },
    );
    let letter = if letters > 0 { letter_bits / letters as f64 } else { 8.0 };
    // How much shorter the others spell `own` of a symbol's `count` without them: the share of
    // the code the others gain back, less what the symbol's own code length was.
    let back = |own: u64, count: u64| match count - own {
        0 => 0.0,
        rest => rest as f64 * length(rest, count),
    };
    tallies
        .iter()
        .zip(&set.lines)
        .zip(alone)
        .map(|((tally, &lines), &alone)| {
            let rest = symbols - tally.symbols();
            // The others' lines, spelled in the set's code rather than one of their own: every
            // symbol of theirs is a smaller part of all, but for those this text shares.
            let mut crowding = match rest {
                0 => 0.0,
                rest => rest as f64 * length(rest, symbols),
            };
            for (unit, &own) in &tally.units {
                crowding -= back(own, all[unit]);
            }
            crowding -= back(tally.runs, runs) + back(tally.ends, ends);
            // What it adds to the shared unit: an entry for each unit only it uses, and each word
            // that pays for its place in the index only with it, its line in the index too. The
            // others spell such a word a letter at a time without it.
            let (mut entries, mut index, mut spelt) = (0.0, 0.0, 0.0);
            for (&unit, &own) in &tally.units {
                let others = all[unit] - own;
                let leaves = unit.chars().nth(1).is_some() && !worth_a_symbol(unit, others);
                if others == 0 || leaves {
                    entries += 1.0;
                }
                if leaves {
                    index += escape(unit).len() as f64 + 1.0;
                    let word = unit.chars().count() as f64 * letter;
                    spelt += others as f64 * (word - length(all[unit], symbols)).max(0.0);
                }
            }
            let bits = entries * entry + crowding - spelt;
            lines as f64 + sharing as f64 + bits * byte + index - alone as f64
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set<'a>(texts: &[&'a str], lines: &[usize]) -> Set<'a> {
        Set { texts: texts.to_vec(), known: HashSet::new(), table_bits: 0.0, lines: lines.to_vec() }
    }

    /// A text the others spell nothing of crowds their code: leaving saves what it took from them,
    /// where a text just like the others takes nothing.
    #[test]
    fn a_text_unlike_the_others_crowds_their_code_and_one_like_them_does_not() {
        let alike = "abababab\nabab\n";
        let unlike = "xyzxyzxy\nxyzx\n";
        let costs = |texts: &[&str]| {
            let lines = vec![10; texts.len()];
            savings(&set(texts, &lines), &vec![10; texts.len()], 0, Alphabet::Base64Url)
        };
        let same = costs(&[alike, alike, alike]);
        assert!(same.iter().all(|&saving| saving.abs() < 0.5), "{same:?}");
        let mixed = costs(&[alike, alike, unlike]);
        assert!(mixed[2] > 1.0, "the unlike text crowds the others: {mixed:?}");
        assert!(mixed[2] > mixed[0], "{mixed:?}");
    }

    /// The estimate against folding: for each text of a set, what the set takes, less what the
    /// rest take together and it alone, as folded. Chapters of Genesis, which share a code well,
    /// and two drawings, which don't: the estimate says which way each goes, and within a little
    /// of how much.
    #[test]
    fn what_leaving_saves_is_estimated_close_to_what_folding_says() {
        use crate::parts;
        let data = |name: &str| {
            let path = format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR"));
            std::fs::read_to_string(path).expect("an example")
        };
        let mut texts: Vec<String> =
            (1..=4).map(|n| data(&format!("biblical_text_multiple/chapter_{n:02}.txt"))).collect();
        texts.push(data("long_ascii_anime_postergirl.txt"));
        texts.push(data("small_filled_blocks_miku.txt"));
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        let alphabet = Alphabet::Base64Url;
        let together = parts::fold_together(&texts, alphabet, true);
        let alone: Vec<usize> =
            texts.iter().map(|text| parts::fold(text, alphabet, true).size()).collect();
        let set = Set {
            texts: texts.clone(),
            known: together
                .unit
                .index
                .iter()
                .flat_map(|index| index.values())
                .map(String::as_str)
                .collect(),
            table_bits: together.unit.table.chars().count() as f64 * alphabet.bits_per_char(),
            lines: together
                .lines
                .iter()
                .map(|lines| lines.iter().map(|line| line.len() + 1).sum())
                .collect(),
        };
        let estimated = savings(&set, &alone, 0, alphabet);
        for (at, &estimate) in estimated.iter().enumerate() {
            let rest: Vec<&str> =
                texts.iter().enumerate().filter(|&(i, _)| i != at).map(|(_, t)| *t).collect();
            let without = parts::fold_together(&rest, alphabet, true).size();
            let saved = together.size() as f64 - (without + alone[at]) as f64;
            assert_eq!(estimate > 0.0, saved > 0.0, "text {at}: {estimate:.0} against {saved:.0}");
            let off = (estimate - saved).abs();
            assert!(
                off <= 40.0_f64.max(saved.abs() / 6.0),
                "text {at}: {estimate:.0} against {saved:.0}"
            );
        }
        assert!(estimated[..4].iter().all(|&saving| saving < 0.0), "the chapters stay");
        assert!(estimated[4..].iter().all(|&saving| saving > 0.0), "the drawings leave");
    }
}
