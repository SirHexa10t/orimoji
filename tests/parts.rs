//! Folds into parts give back exactly what they folded, keep an older fold's code when asked to,
//! and refuse parts that don't belong together, damaged lines, and texts past their limit.

use orimoji::parts::{self, Parts, Unit};
use orimoji::{Alphabet, Index, UnfoldError};

/// Enough room for any text here.
const ROOM: usize = 1 << 26;

fn example(name: &str) -> String {
    std::fs::read_to_string(format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR")))
        .expect("an example")
}

fn drawings() -> Vec<String> {
    ["long_ascii_anime_postergirl.txt", "small_braille_kirino.txt", "small_filled_blocks_miku.txt"]
        .iter()
        .map(|name| example(name))
        .collect()
}

/// Text that has broken line-based formats before, and the drawings.
fn texts() -> Vec<String> {
    let awkward = [
        "",
        "\n",
        "\n\n\n",
        "a",
        "a\n",
        "trailing spaces   \n  and leading\n",
        "tabs\there\tand\tthere\n",
        "CRLF lines\r\nas Windows writes\r\n",
        "<end>\t<esc>\n\\\\ \\u{20} \\0 @3\n",
        "白猫骑士物语 / White Cat Knight\nעברית\ne\u{301}\u{302}\n🎮👾\u{200d}\n",
        "\u{1b}[38;2;226;57;57m⣿⣿⣿⣿⣿⣿\u{1b}[0m\u{1b}[38;2;226;57;57m⡇\u{1b}[0m\n\u{1b}[1m\u{1b}[",
        "0123456789\n00000000000000000000000000000000000000000000\n9876543210\n",
        "\u{10ffff}\u{0}\u{7f}",
    ];
    let mut texts: Vec<String> = awkward.iter().map(|text| text.to_string()).collect();
    texts.push("x".repeat(10_000));
    texts.push((0..2000).map(|i| char::from(b'a' + (i * 7 % 26) as u8)).collect());
    texts.extend(drawings());
    texts
}

/// An unfold's text, or what it refused, said.
fn said(unfolded: Result<String, UnfoldError>) -> Result<String, String> {
    unfolded.map_err(|error| error.to_string())
}

fn back(folded: &parts::Parts, alphabet: Alphabet) -> Result<String, UnfoldError> {
    parts::unfold(&folded.lines, &folded.unit, alphabet, ROOM)
}

#[test]
fn parts_give_back_exactly_what_they_folded() {
    let alphabets = [
        Alphabet::Base64Url,
        Alphabet::Hex,
        Alphabet::Base58,
        Alphabet::Base94,
        Alphabet::Braille,
        Alphabet::Cjk,
    ];
    for text in texts() {
        for alphabet in alphabets {
            for index in [false, true] {
                let folded = parts::fold(&text, alphabet, index);
                assert_eq!(said(back(&folded, alphabet)), Ok(text.clone()), "{alphabet} {text:?}");
                assert_eq!(
                    parts::needs_index(&folded.unit.choices, alphabet).unwrap(),
                    folded.unit.index.is_some()
                );
            }
        }
    }
}

/// A line for each line of the input, empty exactly where it is.
#[test]
fn a_line_for_each_line_and_an_empty_line_for_an_empty_one() {
    for text in texts() {
        let folded = parts::fold(&text, Alphabet::Base64Url, false);
        let lines: Vec<&str> = text.split('\n').collect();
        assert_eq!(folded.lines.len(), lines.len(), "{text:?}");
        for (line, folded) in lines.iter().zip(&folded.lines) {
            assert_eq!(line.is_empty(), folded.is_empty(), "{line:?}");
        }
    }
}

/// Asked to index, a fold keeps an index only where its parts come out smaller with it, the
/// index's text counted: the coloured drawing's escapes pay, the braille drawing has none.
#[test]
fn an_index_is_kept_only_where_it_pays() {
    let drawings = drawings();
    let coloured = parts::fold(&drawings[2], Alphabet::Base64Url, true);
    let plain = parts::fold(&drawings[2], Alphabet::Base64Url, false);
    let index = coloured.unit.index.as_ref().expect("the escapes pay for an index");
    assert!(index.values().iter().any(|value| value.starts_with("\u{1b}[")));
    assert!(coloured.size() < plain.size());
    assert_eq!(parts::fold(&drawings[1], Alphabet::Base64Url, true).unit.index, None, "none pays");
    assert_eq!(plain.unit.index, None, "none was asked for");
}

/// Folded with the unit of the parts before, every line the two texts share folds the same, and
/// a character the code lacks is spelled out.
#[test]
fn kept_parts_fold_the_lines_that_did_not_change_the_same() {
    for text in drawings() {
        for index in [false, true] {
            let before = parts::fold(&text, Alphabet::Base64Url, index);
            let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            lines[3].push_str("白猫 and more");
            lines.insert(5, "a new line".to_string());
            let changed = lines.join("\n");
            let lines_kept = parts::fold_keeping(&changed, &before.unit, Alphabet::Base64Url);
            let kept = Parts { lines: lines_kept.unwrap(), unit: before.unit.clone() };
            let differ: Vec<usize> = (0..lines.len())
                .filter(|&at| {
                    let was =
                        if at < 5 { Some(at) } else { at.checked_sub(1).filter(|&at| at != 4) };
                    was.is_none_or(|was| before.lines[was] != kept.lines[at])
                })
                .collect();
            assert_eq!(differ, [3, 5], "only the changed line and the new one");
            assert_eq!(said(back(&kept, Alphabet::Base64Url)), Ok(changed));
        }
    }
}

/// Parts that don't belong together are refused, never read wrong: a table or choices from
/// another fold, either of them damaged, no index or another one where the lines need one.
#[test]
fn parts_that_dont_belong_together_are_refused() {
    let drawings = drawings();
    let a = parts::fold(&drawings[2], Alphabet::Base64Url, true);
    let b = parts::fold(&drawings[1], Alphabet::Base64Url, false);
    let index = a.unit.index.as_ref().expect("an index");
    let other = Index::new(index.values().iter().rev().cloned()).unwrap();
    let mut table = a.unit.table.clone();
    table.replace_range(3..4, if &table[3..4] == "A" { "B" } else { "A" });
    let mut choices = a.unit.choices.clone();
    choices.replace_range(6..7, if &choices[6..7] == "A" { "B" } else { "A" });
    let unfold = |table: &str, choices: &str, index: Option<&Index>| {
        let unit =
            Unit { choices: choices.to_string(), table: table.to_string(), index: index.cloned() };
        parts::unfold(&a.lines, &unit, Alphabet::Base64Url, ROOM)
    };
    assert!(matches!(
        unfold(&b.unit.table, &a.unit.choices, Some(index)),
        Err(UnfoldError::PartsDontFit)
    ));
    assert!(matches!(unfold(&a.unit.table, &b.unit.choices, None), Err(UnfoldError::PartsDontFit)));
    assert!(matches!(unfold(&table, &a.unit.choices, Some(index)), Err(UnfoldError::PartsDontFit)));
    assert!(matches!(
        unfold(&a.unit.table, &choices, Some(index)),
        Err(UnfoldError::PartsDontFit | UnfoldError::Choices(_))
    ));
    assert!(matches!(unfold(&a.unit.table, &a.unit.choices, None), Err(UnfoldError::NeedsIndex)));
    assert!(matches!(
        unfold(&a.unit.table, &a.unit.choices, Some(&other)),
        Err(UnfoldError::PartsDontFit)
    ));
    let shorter = Index::new(index.values()[..1].to_vec()).unwrap();
    assert!(matches!(
        unfold(&a.unit.table, &a.unit.choices, Some(&shorter)),
        Err(UnfoldError::WrongIndex)
    ));
    let grown = Index::new(index.values().iter().cloned().chain(["more".to_string()])).unwrap();
    let read = said(unfold(&a.unit.table, &a.unit.choices, Some(&grown)));
    assert_eq!(read, Ok(drawings[2].clone()), "grown at its end");
    // And keeping the code of parts that don't fit is refused the same way.
    assert!(
        parts::fold_keeping(
            "x",
            &Unit { table: b.unit.table.clone(), ..a.unit.clone() },
            Alphabet::Base64Url
        )
        .is_err()
    );
}

/// Choices of a newer format are refused by its number, and choices cut short or with more after
/// them as damage.
#[test]
fn choices_of_a_newer_format_or_damaged_are_refused() {
    let folded = parts::fold("abc\nabd", Alphabet::Hex, false);
    // Hex is four bits a character, so the first character is the format.
    let newer = format!("3{}", &folded.unit.choices[1..]);
    let with = |choices: &str| Unit { choices: choices.to_string(), ..folded.unit.clone() };
    let refused = parts::unfold(&folded.lines, &with(&newer), Alphabet::Hex, ROOM);
    match refused {
        Err(UnfoldError::Choices(why)) => assert!(why.contains("format 3, newer"), "{why}"),
        other => panic!("{other:?}"),
    }
    for damaged in [&folded.unit.choices[..3], &format!("{}f", folded.unit.choices), "", "zz"] {
        let refused = parts::unfold(&folded.lines, &with(damaged), Alphabet::Hex, ROOM);
        assert!(matches!(refused, Err(UnfoldError::Choices(_))), "{damaged:?}: {refused:?}");
    }
}

/// A damaged line is refused by its number, counting from 1: a character changed, padding that
/// isn't zeros, a character no alphabet has, bits that spell nothing.
#[test]
fn a_damaged_line_is_refused_by_its_number() {
    let text = drawings().remove(1);
    let folded = parts::fold(&text, Alphabet::Base64Url, false);
    let at = 6;
    let mut damaged_lines = vec![folded.lines.clone(); 5];
    damaged_lines[0][at].push('A');
    damaged_lines[4][at].push_str("AA");
    damaged_lines[1][at].push('!');
    damaged_lines[2][at] = "A".to_string();
    damaged_lines[3][at] = format!("{}_", folded.lines[at]);
    for lines in damaged_lines {
        match parts::unfold(&lines, &folded.unit, Alphabet::Base64Url, ROOM) {
            Err(UnfoldError::Corrupt { line, .. } | UnfoldError::Alphabet { line, .. }) => {
                assert_eq!(line, at + 1)
            }
            other => panic!("{other:?}"),
        }
    }
}

/// A fold that would come to more than the limit is refused before the text is made: a long run
/// is a few bits, and asks for more than its line is worth.
#[test]
fn a_text_past_the_limit_is_refused_before_it_is_made() {
    let text = format!("ab\n{}\ncd", "x".repeat(100_000));
    let folded = parts::fold(&text, Alphabet::Base64Url, false);
    let unfold = |limit| parts::unfold(&folded.lines, &folded.unit, Alphabet::Base64Url, limit);
    assert_eq!(said(unfold(text.len())), Ok(text.clone()), "exactly the limit is within it");
    assert!(matches!(unfold(text.len() - 1), Err(UnfoldError::TooLarge { line: 3, .. })));
    assert!(matches!(unfold(50_000), Err(UnfoldError::TooLarge { line: 2, .. })));
    assert!(matches!(unfold(1), Err(UnfoldError::TooLarge { line: 1, .. })));
}

/// Damage of any kind, to any part, is refused or read, and never panics.
#[test]
fn damaged_parts_never_panic() {
    let noise = ['A', '_', '-', '9', 'f', '!', 'é', '\n', ' '];
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let mut below = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for text in drawings() {
        let folded = parts::fold(&text, Alphabet::Base64Url, true);
        for _ in 0..400 {
            let (mut lines, mut table, mut choices) =
                (folded.lines.clone(), folded.unit.table.clone(), folded.unit.choices.clone());
            let part = match below(3) {
                0 => &mut table,
                1 => &mut choices,
                _ => {
                    let at = below(lines.len());
                    &mut lines[at]
                }
            };
            let mut chars: Vec<char> = part.chars().collect();
            let at = below(chars.len() + 1);
            match below(3) {
                0 if at < chars.len() => chars[at] = noise[below(noise.len())],
                1 if at < chars.len() => {
                    chars.remove(at);
                }
                _ => chars.insert(at, noise[below(noise.len())]),
            }
            *part = chars.into_iter().collect();
            let unit = Unit { choices, table, index: folded.unit.index.clone() };
            let _ = parts::unfold(&lines, &unit, Alphabet::Base64Url, ROOM);
        }
    }
}

/// On arterminal's drawings, parts take fewer bytes than a folded file of the same, of the first
/// format, its index included either way: the compact table, the weighed runs and the counts'
/// code.
#[test]
fn parts_are_smaller_than_a_folded_file() {
    let how = orimoji::Fold::new(orimoji::Squeeze::Text, Alphabet::Base64Url).indexed();
    let how = orimoji::Fold { index_inside: false, ..how };
    for text in drawings() {
        let folded = orimoji::fold(text.as_bytes(), &how).unwrap();
        let file =
            folded.text.len() + folded.indexing.index().map_or(0, |index| index.to_text().len());
        let parts = parts::fold(&text, Alphabet::Base64Url, true);
        assert!(parts.size() < file, "{} bytes in parts, {file} in a folded file", parts.size());
    }
}

/// The choices record what the fold decided, each decision as this text suits it: a code of
/// their own for the lengths of a table as long as a braille drawing's and five bits each for a
/// short one, low bits as they are for the steps between scattered scripts, a code for the counts
/// of runs that are long and varied, and how many values of the index the lines need.
#[test]
fn the_choices_record_what_the_fold_decided() {
    let decided = |text: &str, index: bool| {
        let folded = parts::fold(text, Alphabet::Base64Url, index);
        let decisions = folded.unit.decisions(Alphabet::Base64Url).unwrap();
        assert_eq!(decisions.format, parts::FORMAT);
        (decisions, folded)
    };
    let drawings = drawings();
    assert_eq!(
        decided(&drawings[1], false).0.lengths,
        "huffman",
        "a braille drawing's 78 characters"
    );
    assert_eq!(decided("ab\nab", false).0.lengths, "fixed", "a short table");
    let scripts = "白猫骑士物语 / White Cat Knight\nעברית מימין לשמאל\nΕλληνικά русский текст 日本語のテキスト 한국어";
    assert!(decided(scripts, false).0.step_low_bits > 0, "scattered scripts");
    assert_eq!(decided(&drawings[1], false).0.step_low_bits, 0, "braille, all close together");
    let runs: String = (0..60).map(|i| format!("{}.", "x".repeat(3 + i * 37 % 900))).collect();
    let (counts, _) = decided(&runs, false);
    assert!(counts.counts_coded || counts.count_low_bits > 0, "long, varied runs: {counts:?}");
    assert_eq!(decided("a\nb", false).0.count_low_bits, 0, "no runs at all");
    let (indexed, folded) = decided(&drawings[2], true);
    let values = folded.unit.index.as_ref().map(|index| index.len() as u64);
    assert_eq!((indexed.index_values, values.is_some()), (values, true));
}

/// Parts in format 1, written before the choices recorded how the table is written, as their
/// fold wrote them: they unfold to what was folded, and say they are format 1.
#[test]
fn parts_in_format_1_still_unfold() {
    const DRAWING_CHOICES: &str = "EgxTWL-";
    const DRAWING_TABLE: &str = "NGQQISAAJ-AgEcgHEYEcQ";
    const DRAWING_LINES: &[&str] = &["Rf2rA", "X9Fqc", "", "4dY"];
    const INDEXED_CHOICES: &str = "EKibSd5A";
    const INDEXED_TABLE: &str = "EshiBwhRhAc0oKSBIgACUbHHG";
    const INDEXED_LINES: &[&str] = &[
        "azTwgNA", "aWaeEH9A", "avmnhB6g", "atZp4QGg", "azTwg_o", "aWaeEHq", "avmnhAa", "atZp4Qf0",
        "azTwg9Q", "aWaeEBo", "avmnhB_Q", "atZp4Qeo",
    ];
    let drawing = "⣿⣿⣿⣿⣿⣿⡇ ⢸⣿⣿\n⣿⣿⡇      ⢸⣿⣿⣿⣿\n\n⠀⠀⠀⠀⠀⠀⠀⠀⣿⣿";
    let indexed: String = (0..12)
        .map(|i| {
            format!("\u{1b}[38;2;226;57;57m{}\u{1b}[0m Simulator {}", "█".repeat(1 + i % 4), i % 3)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let index = Index::new(["\u{1b}[38;2;226;57;57m", "Simulator"]).unwrap();
    let unfold = |lines: &[&str], table: &str, choices: &str, index: Option<&Index>| {
        let unit = Unit { choices: choices.into(), table: table.into(), index: index.cloned() };
        said(parts::unfold(lines, &unit, Alphabet::Base64Url, ROOM))
    };
    assert_eq!(
        unfold(DRAWING_LINES, DRAWING_TABLE, DRAWING_CHOICES, None),
        Ok(drawing.to_string())
    );
    assert_eq!(unfold(INDEXED_LINES, INDEXED_TABLE, INDEXED_CHOICES, Some(&index)), Ok(indexed));
    let decided = parts::decisions(INDEXED_CHOICES, Alphabet::Base64Url).unwrap();
    assert_eq!((decided.format, decided.lengths, decided.step_low_bits), (1, "fixed", 0));
    assert_eq!(decided.index_values, Some(2));
}

/// A unit's text reads back as the unit, with an index and without, through the carriage returns
/// an editor may add and without its last newline; cut short, or with an empty line among its
/// index's values, it is refused.
#[test]
fn a_unit_reads_back_from_its_text() {
    let drawings = drawings();
    for unit in [
        parts::fold(&drawings[1], Alphabet::Base64Url, false).unit,
        parts::fold(&drawings[2], Alphabet::Base64Url, true).unit,
    ] {
        let text = unit.to_text();
        assert_eq!(Unit::from_text(&text).ok().as_ref(), Some(&unit));
        assert_eq!(Unit::from_text(text.trim_end()).ok().as_ref(), Some(&unit));
        assert_eq!(Unit::from_text(&text.replace('\n', "\r\n")).ok().as_ref(), Some(&unit));
        assert_eq!(unit.size(), text.len());
    }
    let indexed = parts::fold(&drawings[2], Alphabet::Base64Url, true).unit;
    let text = indexed.to_text();
    for damaged in ["", "only the choices\n", &text.replacen('\n', "\n\n", 3)] {
        let refused = Unit::from_text(damaged);
        assert!(matches!(refused, Err(UnfoldError::Unit(_))), "{damaged:?}: {refused:?}");
    }
    // A unit with an index its lines don't use is refused, as no fold makes one.
    let plain = parts::fold(&drawings[1], Alphabet::Base64Url, false);
    let padded = Unit { index: indexed.index.clone(), ..plain.unit.clone() };
    let refused = parts::unfold(&plain.lines, &padded, Alphabet::Base64Url, ROOM);
    assert!(matches!(refused, Err(UnfoldError::Unit(_))), "{refused:?}");
}

/// Texts folded together share one unit, which every one of them unfolds with. (Whether together
/// is smaller depends on how alike they are: these three drawings, nothing alike, take more
/// together than apart; a book's chapters take less, see `tests/chapters.rs`.)
#[test]
fn texts_folded_together_share_one_unit() {
    let drawings = drawings();
    let texts: Vec<&str> = drawings.iter().map(String::as_str).collect();
    for index in [false, true] {
        let together = parts::fold_together(&texts, Alphabet::Base64Url, index);
        assert_eq!(together.lines.len(), texts.len());
        for (lines, text) in together.lines.iter().zip(&texts) {
            let back = parts::unfold(lines, &together.unit, Alphabet::Base64Url, ROOM);
            assert_eq!(said(back), Ok(text.to_string()));
        }
    }
}

/// Chapter `n` of Genesis.
fn chapter(n: usize) -> String {
    example(&format!("biblical_text_multiple/chapter_{n:02}.txt"))
}

/// Each text a grouped fold folded, back: those that share, with the shared unit; the rest with
/// their own.
fn grouped_back(grouped: &parts::Grouped, alphabet: Alphabet) -> Vec<String> {
    grouped
        .texts
        .iter()
        .map(|member| match member {
            parts::Member::Sharing(lines) => {
                let unit = grouped.shared.as_ref().expect("a unit to share");
                parts::unfold(lines, unit, alphabet, ROOM).expect("unfolds")
            }
            parts::Member::Alone(folded) => back(folded, alphabet).expect("unfolds"),
        })
        .collect()
}

/// Which texts of a grouped fold share.
fn sharing(grouped: &parts::Grouped) -> Vec<bool> {
    grouped.texts.iter().map(|member| matches!(member, parts::Member::Sharing(_))).collect()
}

/// Texts that spell alike share one unit; texts unlike each other fold each alone, as `fold`
/// folds them; and among both, those unlike the rest fold alone and the rest share. Every text
/// comes back, and the whole is never larger than all of them together, or all of them alone.
#[test]
fn texts_fold_together_where_that_takes_less_and_alone_where_it_does_not() {
    let alphabet = Alphabet::Base64Url;
    let none = parts::Overheads::default();
    let book: Vec<String> = (1..=6).map(chapter).collect();
    let art = drawings();
    let mixed: Vec<String> =
        [art[0].clone()].into_iter().chain(book.iter().cloned()).chain([art[2].clone()]).collect();
    for (texts, expected) in [
        (&book, vec![true; 6]),
        (&art, vec![false; 3]),
        (&mixed, [vec![false], vec![true; 6], vec![false]].concat()),
    ] {
        let grouped = parts::fold_grouped(texts, alphabet, true, none);
        assert_eq!(sharing(&grouped), expected);
        assert_eq!(grouped.shared.is_some(), expected.contains(&true));
        assert_eq!(&grouped_back(&grouped, alphabet), texts);
        let together = parts::fold_together(texts, alphabet, true).size();
        let alone: usize = texts.iter().map(|text| parts::fold(text, alphabet, true).size()).sum();
        let size = grouped.size(none);
        assert!(size <= together && size <= alone, "{size}: {together} together, {alone} alone");
        for (member, text) in grouped.texts.iter().zip(texts) {
            if let parts::Member::Alone(folded) = member {
                assert_eq!(folded, &parts::fold(text, alphabet, true), "as if folded by itself");
            }
        }
    }
}

/// What keeping each text costs besides weighs too: two texts that share well enough on their
/// own fold alone once each that shares pays a header that eats what sharing saved.
#[test]
fn what_keeping_a_text_costs_is_weighed_too() {
    let alphabet = Alphabet::Base64Url;
    let texts = [chapter(1), chapter(2)];
    let free = parts::fold_grouped(&texts, alphabet, true, parts::Overheads::default());
    assert!(free.shared.is_some(), "with nothing else to pay, two chapters share");
    let saved = parts::fold(&texts[0], alphabet, true).size()
        + parts::fold(&texts[1], alphabet, true).size()
        - free.size(parts::Overheads::default());
    let costly = parts::Overheads { alone: 0, sharing: saved, shared_unit: 0 };
    let grouped = parts::fold_grouped(&texts, alphabet, true, costly);
    assert_eq!(grouped.shared, None, "a header of {saved} bytes a text eats it");
}

/// No text folds into nothing, and one text alone.
#[test]
fn no_text_and_one_text_are_grouped_as_nothing_and_alone() {
    let none: [&str; 0] = [];
    let grouped = parts::fold_grouped(&none, Alphabet::Base64Url, true, Default::default());
    assert_eq!((grouped.shared, grouped.texts.len()), (None, 0));
    let one = parts::fold_grouped(&[chapter(1)], Alphabet::Base64Url, true, Default::default());
    let alone = parts::fold(&chapter(1), Alphabet::Base64Url, true);
    assert_eq!(one.texts, [parts::Member::Alone(alone)]);
}
