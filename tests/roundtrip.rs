//! Every squeeze into every alphabet gives back exactly what it folded, over the examples and the
//! inputs most likely to break a line-based format.

use orimoji::{
    Alphabet, Fold, FoldError, Folded, Index, Indexing, Squeeze, TableOptions, UnfoldError,
};

fn example(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR")))
        .expect("an example")
}

fn squeezes() -> Vec<Squeeze> {
    vec![
        Squeeze::None { whole: false },
        Squeeze::None { whole: true },
        Squeeze::Pack,
        Squeeze::Text,
        Squeeze::Table(TableOptions::default()),
        Squeeze::Table(TableOptions { separator: ',', keep: None }),
        Squeeze::Deflate { lean: Some(0) },
        Squeeze::Deflate { lean: Some(3) },
        Squeeze::Deflate { lean: None },
        Squeeze::Zstd { level: 3 },
    ]
}

/// Text that has broken line-based formats before.
fn awkward() -> Vec<Vec<u8>> {
    let texts = [
        "",
        "\n",
        "\n\n\n",
        "a",
        "a\n",
        "no newline at the end",
        "trailing spaces   \n  and leading\n",
        "tabs\there\tand\tthere\n\tleading tab\ntrailing tab\t\n",
        "CRLF lines\r\nas Windows writes\r\n",
        "=== body ===\n=== orimoji 1 none hex ===\n=== deflated tables ===\n",
        " starts with a space\n# a comment\n#\n",
        "<end>\t<esc>\n\\\\ \\u{20} \\0\n",
        "白猫骑士物语 / White Cat Knight\nעברית מימין לשמאל\ne\u{301}\u{302} combining\n🎮👾\u{200d}\n",
        "\u{1b}[38;2;226;57;57m⣿⣿⣿⣿⣿⣿\u{1b}[0m\u{1b}[38;2;226;57;57m⡇\u{1b}[0m\n\u{1b}[1m\u{1b}[",
        "0123456789\n00000000000000000000000000000000000000000000\n9876543210\n",
        "a,b,,c\n,\n,,\n;;\na;b,c\n",
        "?\t\t?\n\t\t\n1\t2\t3\n",
    ];
    let mut out: Vec<Vec<u8>> = texts.iter().map(|text| text.as_bytes().to_vec()).collect();
    out.push("x".repeat(3000).into_bytes());
    out.push(
        (0..2000).map(|i| char::from(b'a' + (i * 7 % 26) as u8)).collect::<String>().into_bytes(),
    );
    out.push(
        (0..300)
            .map(|i| format!("{}\t{}\t{}", i * 10, i % 7, ["Valve", "?", ""][i % 3]))
            .collect::<Vec<_>>()
            .join("\n")
            .into_bytes(),
    );
    out
}

fn roundtrip(input: &[u8], squeeze: &Squeeze, alphabet: Alphabet) {
    roundtrip_as(input, &Fold::new(squeeze.clone(), alphabet));
    if squeeze.has_tables() {
        roundtrip_as(input, &Fold::new(squeeze.clone(), alphabet).indexed());
    }
}

fn roundtrip_as(input: &[u8], how: &Fold) {
    let (squeeze, alphabet, how) = (&how.squeeze, how.alphabet, how.clone());
    let folded = match orimoji::fold(input, &how) {
        Ok(folded) => folded,
        Err(FoldError::NotATable) if matches!(squeeze, Squeeze::Table(_)) => return,
        Err(FoldError::NotText { .. }) if squeeze.reads_text() => return,
        Err(error) => panic!("{} into {alphabet}: {error}", squeeze.describe()),
    };
    let (text, index) = (&folded.text, folded.indexing.index());
    assert_eq!(orimoji::needs_index(text).unwrap(), index.is_some(), "the text says what it needs");
    if !how.index {
        assert_eq!(folded.indexing, Indexing::Off);
    }
    let back = orimoji::unfold(text, index)
        .unwrap_or_else(|error| panic!("{} into {alphabet}: {error}\n{text}", squeeze.describe()));
    assert_eq!(back, input, "{} into {alphabet}:\n{text}", squeeze.describe());
    assert_eq!(orimoji::fold(input, &how).unwrap(), folded, "a fold is the same every time");
    let readable = orimoji::fold(input, &Fold { readable: true, ..how.clone() }).unwrap();
    let unfolded = orimoji::unfold(&readable.text, readable.indexing.index()).unwrap();
    assert_eq!(unfolded, input, "readable tables read back too");
    if squeeze.by_line() {
        let lines = |text: &[u8]| text.split(|&byte| byte == b'\n').count();
        let body = text.split("\n=== body ===\n").nth(1).expect("a body");
        assert_eq!(
            lines(body.strip_suffix('\n').unwrap_or(body).as_bytes()),
            lines(input),
            "a line for each line"
        );
    }
}

#[test]
fn every_squeeze_into_every_alphabet_gives_back_the_awkward_inputs() {
    for input in awkward() {
        for squeeze in squeezes() {
            for alphabet in Alphabet::ALL {
                roundtrip(&input, &squeeze, alphabet);
            }
        }
    }
}

#[test]
fn every_squeeze_into_every_alphabet_gives_back_the_examples() {
    for name in [
        "long_ascii_anime_postergirl.txt",
        "small_braille_kirino.txt",
        "small_filled_blocks_miku.txt",
        "steam-games-excerpt.tsv",
    ] {
        let input = example(name);
        for squeeze in squeezes() {
            for alphabet in [
                Alphabet::Base64Url,
                Alphabet::Base91,
                Alphabet::TwoByte,
                Alphabet::Cjk,
                Alphabet::Base58,
            ] {
                roundtrip(&input, &squeeze, alphabet);
            }
        }
    }
}

#[test]
fn bytes_that_are_no_text_fold_with_the_squeezes_that_take_bytes() {
    let bytes: Vec<u8> = (0..5000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
    for squeeze in squeezes() {
        let folded = orimoji::fold(&bytes, &Fold::new(squeeze.clone(), Alphabet::CjkExtB));
        match squeeze.reads_text() {
            true => assert!(matches!(folded, Err(FoldError::NotText { .. }))),
            false => assert_eq!(orimoji::unfold(&folded.unwrap().text, None).unwrap(), bytes),
        }
    }
}

/// What a fold costs the caller: its text, and its index if it made one.
fn kept(folded: &Folded) -> usize {
    folded.text.len() + folded.indexing.index().map_or(0, |index| index.to_text().len())
}

#[test]
fn the_listing_is_folded_as_a_table_and_smaller_than_by_any_other_line_squeeze() {
    let listing = example("steam-games-excerpt.tsv");
    assert_eq!(
        orimoji::pick(&listing, Alphabet::Base64Url, true),
        Squeeze::Table(TableOptions::default())
    );
    let size = |squeeze: Squeeze| {
        kept(&orimoji::fold(&listing, &Fold::new(squeeze, Alphabet::Base64Url).indexed()).unwrap())
    };
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url);
    let table = orimoji::fold(&listing, &how.clone().indexed()).unwrap();
    let unindexed = orimoji::fold(&listing, &how).unwrap();
    assert!(kept(&table) < kept(&unindexed), "indexing pays on a listing, index included");
    assert_eq!(unindexed.indexing, Indexing::Off, "no index unless asked");
    let index = table.indexing.index().expect("the listing is worth indexing");
    assert!(index.place("Valve").is_some(), "the studios are listed");
    for other in [
        Squeeze::Text,
        Squeeze::Pack,
        Squeeze::None { whole: false },
        Squeeze::Deflate { lean: Some(1) },
    ] {
        assert!(kept(&table) < size(other.clone()), "{} beats the table", other.describe());
    }
    // Its comment lines stay readable.
    assert!(table.text.lines().any(|line| line.starts_with(" # app_id")));
}

#[test]
fn a_fold_keeping_the_tables_changes_only_the_lines_that_changed() {
    let listing = String::from_utf8(example("steam-games-excerpt.tsv")).unwrap();
    let mut rows: Vec<String> = listing.lines().map(str::to_string).collect();
    let row = rows.iter().position(|row| !row.starts_with('#')).unwrap() + 10;
    rows[row] = rows[row].replacen("\t0\t", "\t11\t", 1);
    rows.push("9999990\t0\t9\t100\t1\t2030-01-01\t7\t1\t1\t3\tY\t1\t1\t1\tqqq,zzz\t\t1,2\tÑew Ståudio\tÑew Ståudio\tA Game Nobody Listed".into());
    let changed = rows.join("\n") + "\n";
    for squeeze in [Squeeze::Table(TableOptions::default()), Squeeze::Text] {
        let how = Fold::new(squeeze, Alphabet::Base64Url).indexed();
        let before = orimoji::fold(listing.as_bytes(), &how).unwrap();
        let index = before.indexing.index();
        let after = orimoji::fold_keeping(changed.as_bytes(), &before.text, index, &how).unwrap();
        assert_eq!(orimoji::unfold(&after, index).unwrap(), changed.as_bytes());
        let (old, new): (Vec<&str>, Vec<&str>) =
            (before.text.lines().collect(), after.lines().collect());
        let differing = old.iter().zip(&new).filter(|(a, b)| a != b).count();
        // The changed row (and, for a sorted column coded by distance, the row below it).
        assert!((1..=2).contains(&differing), "{differing} lines changed");
        assert_eq!(new.len(), old.len() + 1, "and one line was added");
    }
}

#[test]
fn an_index_is_kept_only_where_it_takes_at_most_half_what_it_saves() {
    // Every value once: no list of them would pay.
    let unique: String = (0..300).map(|i| format!("{i}\tstudio number {i}\n")).collect();
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
    let folded = orimoji::fold(unique.as_bytes(), &how).unwrap();
    let listed = |folded: &Folded, value: &str| {
        folded.indexing.index().is_some_and(|index| index.place(value).is_some())
    };
    assert!(!listed(&folded, "studio number 7"));
    // Three values over and over: the list is all but free.
    let repeated: String = (0..300)
        .map(|i| format!("{i}\t{}\n", ["Valve", "Gearbox Software", "Bethesda"][i % 3]))
        .collect();
    let folded = orimoji::fold(repeated.as_bytes(), &how).unwrap();
    assert_eq!(
        folded.indexing.index().unwrap().values(),
        ["Bethesda", "Gearbox Software", "Valve"]
    );
}

#[test]
fn an_index_asked_for_but_not_worth_it_is_said_to_be_unneeded() {
    let drawing = example("small_braille_kirino.txt");
    let folded =
        orimoji::fold(&drawing, &Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed()).unwrap();
    assert_eq!(folded.indexing, Indexing::Unneeded, "nothing in a braille drawing repeats enough");
    assert!(!orimoji::needs_index(&folded.text).unwrap());
    assert_eq!(orimoji::unfold(&folded.text, None).unwrap(), drawing);
}

#[test]
fn a_text_folded_with_an_index_refuses_to_unfold_without_it_or_with_another() {
    let listing = example("steam-games-excerpt.tsv");
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
    let folded = orimoji::fold(&listing, &how).unwrap();
    let index = folded.indexing.index().unwrap();
    assert!(matches!(orimoji::unfold(&folded.text, None), Err(UnfoldError::NeedsIndex)));
    let other = Index::new(index.values().iter().rev().cloned()).unwrap();
    assert!(matches!(orimoji::unfold(&folded.text, Some(&other)), Err(UnfoldError::WrongIndex)));
    // Values added at its end leave it fitting what was folded with it.
    let grown =
        Index::new(index.values().iter().cloned().chain(["a newcomer".to_string()])).unwrap();
    assert_eq!(orimoji::unfold(&folded.text, Some(&grown)).unwrap(), listing);
    // And it survives being kept as text.
    let kept = Index::from_text(&index.to_text()).unwrap();
    assert_eq!(orimoji::unfold(&folded.text, Some(&kept)).unwrap(), listing);
}

#[test]
fn inputs_folded_together_share_one_index() {
    let names = [
        "long_ascii_anime_postergirl.txt",
        "small_braille_kirino.txt",
        "small_filled_blocks_miku.txt",
    ];
    let drawings: Vec<Vec<u8>> = names.iter().map(|name| example(name)).collect();
    let inputs: Vec<&[u8]> = drawings.iter().map(Vec::as_slice).collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = orimoji::fold_all(&inputs, &how).unwrap();
    let index = folded.indexing.index().expect("the coloured drawing's escapes are worth indexing");
    for (text, drawing) in folded.texts.iter().zip(&drawings) {
        assert_eq!(&orimoji::unfold(text, Some(index)).unwrap(), drawing);
    }
    assert!(!orimoji::needs_index(&folded.texts[1]).unwrap(), "the braille drawing lists nothing");
    assert!(orimoji::needs_index(&folded.texts[2]).unwrap(), "the coloured one does");
    // One drawing on its own makes an index of its own values alone.
    let alone = orimoji::fold(&drawings[2], &how).unwrap();
    assert!(alone.indexing.index().unwrap().len() <= index.len());
}

#[test]
fn keeping_tables_from_another_squeeze_is_refused() {
    let text = b"a\nb\n";
    let before = orimoji::fold(text, &Fold::new(Squeeze::Text, Alphabet::Hex)).unwrap();
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Hex);
    assert!(matches!(
        orimoji::fold_keeping(text, &before.text, None, &how),
        Err(FoldError::PreviousSqueeze(_))
    ));
}

#[test]
fn a_damaged_folded_file_is_refused_or_read_but_never_panics() {
    let listing = example("steam-games-excerpt.tsv");
    for squeeze in squeezes() {
        let how = Fold::new(squeeze, Alphabet::Base64Url).indexed();
        let Ok(Folded { text: folded, indexing }) = orimoji::fold(&listing, &how) else {
            continue;
        };
        let index = indexing.index();
        let chars: Vec<char> = folded.chars().collect();
        let start = folded.find("=== body ===").unwrap();
        for at in (start + 20..chars.len()).step_by(chars.len() / 50 + 1) {
            let mut damaged = chars.clone();
            damaged[at] = match damaged[at] {
                'A' => 'B',
                '\n' => 'Q',
                _ => 'A',
            };
            let _ = orimoji::unfold(&damaged.into_iter().collect::<String>(), index);
        }
        let cut = &folded[..folded.len() * 2 / 3];
        let _ = orimoji::unfold(cut, index);
    }
}

#[test]
fn a_damaged_body_line_is_reported_with_its_line_number() {
    let folded =
        orimoji::fold(b"one\ntwo\nthree", &Fold::new(Squeeze::Text, Alphabet::Hex)).unwrap().text;
    let mut lines: Vec<&str> = folded.lines().collect();
    let last = lines.len() - 1;
    lines[last] = "zz";
    match orimoji::unfold(&lines.join("\n"), None) {
        Err(UnfoldError::Alphabet { line, .. }) => assert_eq!(line, last + 1),
        other => panic!("{other:?}"),
    }
}
