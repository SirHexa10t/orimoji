//! Folds into parts give back exactly what they folded, keep an older fold's code when asked to,
//! and refuse parts that don't belong together, damaged lines, and texts past their limit.

use orimoji::{Alphabet, Index, Indexing, UnfoldError, parts};

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
    let index = folded.indexing.index();
    parts::unfold(&folded.lines, &folded.table, &folded.choices, index, alphabet, ROOM)
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
                    parts::needs_index(&folded.choices, alphabet).unwrap(),
                    folded.indexing.index().is_some()
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
    let index = coloured.indexing.index().expect("the escapes pay for an index");
    assert!(index.values().iter().any(|value| value.starts_with("\u{1b}[")));
    assert!(coloured.size() < plain.size());
    assert_eq!(parts::fold(&drawings[1], Alphabet::Base64Url, true).indexing, Indexing::Unneeded);
    assert_eq!(plain.indexing, Indexing::Off);
}

/// Folded with the code of the parts before, every line the two texts share folds the same, the
/// table and the choices stay as they were, and a character the code lacks is spelled out.
#[test]
fn kept_parts_fold_the_lines_that_did_not_change_the_same() {
    for text in drawings() {
        for index in [false, true] {
            let before = parts::fold(&text, Alphabet::Base64Url, index);
            let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
            lines[3].push_str("白猫 and more");
            lines.insert(5, "a new line".to_string());
            let changed = lines.join("\n");
            let kept = parts::fold_keeping(
                &changed,
                &before.table,
                &before.choices,
                before.indexing.index(),
                Alphabet::Base64Url,
            )
            .unwrap();
            assert_eq!((&kept.table, &kept.choices), (&before.table, &before.choices));
            assert_eq!(kept.indexing.index(), before.indexing.index());
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
    let index = a.indexing.index().expect("an index");
    let other = Index::new(index.values().iter().rev().cloned()).unwrap();
    let mut table = a.table.clone();
    table.replace_range(3..4, if &table[3..4] == "A" { "B" } else { "A" });
    let mut choices = a.choices.clone();
    choices.replace_range(6..7, if &choices[6..7] == "A" { "B" } else { "A" });
    let unfold = |table: &str, choices: &str, index: Option<&Index>| {
        parts::unfold(&a.lines, table, choices, index, Alphabet::Base64Url, ROOM)
    };
    assert!(matches!(unfold(&b.table, &a.choices, Some(index)), Err(UnfoldError::PartsDontFit)));
    assert!(matches!(unfold(&a.table, &b.choices, None), Err(UnfoldError::PartsDontFit)));
    assert!(matches!(unfold(&table, &a.choices, Some(index)), Err(UnfoldError::PartsDontFit)));
    assert!(matches!(
        unfold(&a.table, &choices, Some(index)),
        Err(UnfoldError::PartsDontFit | UnfoldError::Choices(_))
    ));
    assert!(matches!(unfold(&a.table, &a.choices, None), Err(UnfoldError::NeedsIndex)));
    assert!(matches!(unfold(&a.table, &a.choices, Some(&other)), Err(UnfoldError::PartsDontFit)));
    let shorter = Index::new(index.values()[..1].to_vec()).unwrap();
    assert!(matches!(unfold(&a.table, &a.choices, Some(&shorter)), Err(UnfoldError::WrongIndex)));
    let grown = Index::new(index.values().iter().cloned().chain(["more".to_string()])).unwrap();
    let read = said(unfold(&a.table, &a.choices, Some(&grown)));
    assert_eq!(read, Ok(drawings[2].clone()), "grown at its end");
    // And keeping the code of parts that don't fit is refused the same way.
    assert!(
        parts::fold_keeping("x", &b.table, &a.choices, Some(index), Alphabet::Base64Url).is_err()
    );
}

/// Choices of a newer format are refused by its number, and choices cut short or with more after
/// them as damage.
#[test]
fn choices_of_a_newer_format_or_damaged_are_refused() {
    let folded = parts::fold("abc\nabd", Alphabet::Hex, false);
    // Hex is four bits a character, so the first character is the format.
    let newer = format!("2{}", &folded.choices[1..]);
    let refused = parts::unfold(&folded.lines, &folded.table, &newer, None, Alphabet::Hex, ROOM);
    match refused {
        Err(UnfoldError::Choices(why)) => assert!(why.contains("format 2, newer"), "{why}"),
        other => panic!("{other:?}"),
    }
    for damaged in [&folded.choices[..3], &format!("{}f", folded.choices), "", "zz"] {
        let refused =
            parts::unfold(&folded.lines, &folded.table, damaged, None, Alphabet::Hex, ROOM);
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
        match parts::unfold(&lines, &folded.table, &folded.choices, None, Alphabet::Base64Url, ROOM)
        {
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
    let unfold = |limit| {
        parts::unfold(
            &folded.lines,
            &folded.table,
            &folded.choices,
            None,
            Alphabet::Base64Url,
            limit,
        )
    };
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
                (folded.lines.clone(), folded.table.clone(), folded.choices.clone());
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
            let index = folded.indexing.index();
            let _ = parts::unfold(&lines, &table, &choices, index, Alphabet::Base64Url, ROOM);
        }
    }
}

/// On arterminal's drawings, parts take fewer bytes than a folded file of the same, its index
/// included either way: the compact table, the weighed runs and the counts' code.
#[test]
fn parts_are_smaller_than_a_folded_file() {
    let how = orimoji::Fold::new(orimoji::Squeeze::Text, Alphabet::Base64Url).indexed();
    for text in drawings() {
        let folded = orimoji::fold(text.as_bytes(), &how).unwrap();
        let file =
            folded.text.len() + folded.indexing.index().map_or(0, |index| index.to_text().len());
        let parts = parts::fold(&text, Alphabet::Base64Url, true);
        assert!(parts.size() < file, "{} bytes in parts, {file} in a folded file", parts.size());
    }
}
