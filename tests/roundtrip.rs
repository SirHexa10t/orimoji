//! Every squeeze into every alphabet gives back exactly what it folded, over the examples and the
//! inputs most likely to break a line-based format.

use orimoji::{Alphabet, Fold, FoldError, Squeeze, TableOptions, UnfoldError};

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
    let how = Fold::new(squeeze.clone(), alphabet);
    let folded = match orimoji::fold(input, &how) {
        Ok(folded) => folded,
        Err(FoldError::NotATable) if matches!(squeeze, Squeeze::Table(_)) => return,
        Err(FoldError::NotText { .. }) if squeeze.reads_text() => return,
        Err(error) => panic!("{} into {alphabet}: {error}", squeeze.describe()),
    };
    let back = orimoji::unfold(&folded).unwrap_or_else(|error| {
        panic!("{} into {alphabet}: {error}\n{folded}", squeeze.describe())
    });
    assert_eq!(back, input, "{} into {alphabet}:\n{folded}", squeeze.describe());
    assert_eq!(orimoji::fold(input, &how).unwrap(), folded, "a fold is the same every time");
    let readable = orimoji::fold(input, &Fold { readable: true, ..how }).unwrap();
    assert_eq!(orimoji::unfold(&readable).unwrap(), input, "readable tables read back too");
    if squeeze.by_line() {
        let lines = |text: &[u8]| text.split(|&byte| byte == b'\n').count();
        let body = folded.split("\n=== body ===\n").nth(1).expect("a body");
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
            false => assert_eq!(orimoji::unfold(&folded.unwrap()).unwrap(), bytes),
        }
    }
}

#[test]
fn the_listing_is_folded_as_a_table_and_smaller_than_by_any_other_line_squeeze() {
    let listing = example("steam-games-excerpt.tsv");
    assert_eq!(
        orimoji::pick(&listing, Alphabet::Base64Url),
        Squeeze::Table(TableOptions::default())
    );
    let size = |squeeze: Squeeze| {
        orimoji::fold(&listing, &Fold::new(squeeze, Alphabet::Base64Url)).unwrap().len()
    };
    let table = orimoji::fold(
        &listing,
        &Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url),
    )
    .unwrap();
    for other in [
        Squeeze::Text,
        Squeeze::Pack,
        Squeeze::None { whole: false },
        Squeeze::Deflate { lean: Some(1) },
    ] {
        assert!(table.len() < size(other.clone()), "{} beats the table", other.describe());
    }
    // Its comment lines stay readable.
    assert!(table.lines().any(|line| line.starts_with(" # app_id")));
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
        let how = Fold::new(squeeze, Alphabet::Base64Url);
        let before = orimoji::fold(listing.as_bytes(), &how).unwrap();
        let after = orimoji::fold_keeping(changed.as_bytes(), &before, &how).unwrap();
        assert_eq!(orimoji::unfold(&after).unwrap(), changed.as_bytes());
        let (old, new): (Vec<&str>, Vec<&str>) =
            (before.lines().collect(), after.lines().collect());
        let differing = old.iter().zip(&new).filter(|(a, b)| a != b).count();
        // The changed row (and, for a sorted column coded by distance, the row below it).
        assert!((1..=2).contains(&differing), "{differing} lines changed");
        assert_eq!(new.len(), old.len() + 1, "and one line was added");
    }
}

#[test]
fn keeping_tables_from_another_squeeze_is_refused() {
    let text = b"a\nb\n";
    let before = orimoji::fold(text, &Fold::new(Squeeze::Text, Alphabet::Hex)).unwrap();
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Hex);
    assert!(matches!(
        orimoji::fold_keeping(text, &before, &how),
        Err(FoldError::PreviousSqueeze(_))
    ));
}

#[test]
fn a_damaged_folded_file_is_refused_or_read_but_never_panics() {
    let listing = example("steam-games-excerpt.tsv");
    for squeeze in squeezes() {
        let Ok(folded) = orimoji::fold(&listing, &Fold::new(squeeze, Alphabet::Base64Url)) else {
            continue;
        };
        let chars: Vec<char> = folded.chars().collect();
        let start = folded.find("=== body ===").unwrap();
        for at in (start + 20..chars.len()).step_by(chars.len() / 50 + 1) {
            let mut damaged = chars.clone();
            damaged[at] = match damaged[at] {
                'A' => 'B',
                '\n' => 'Q',
                _ => 'A',
            };
            let _ = orimoji::unfold(&damaged.into_iter().collect::<String>());
        }
        let cut = &folded[..folded.len() * 2 / 3];
        let _ = orimoji::unfold(cut);
    }
}

#[test]
fn a_damaged_body_line_is_reported_with_its_line_number() {
    let folded =
        orimoji::fold(b"one\ntwo\nthree", &Fold::new(Squeeze::Text, Alphabet::Hex)).unwrap();
    let mut lines: Vec<&str> = folded.lines().collect();
    let last = lines.len() - 1;
    lines[last] = "zz";
    match orimoji::unfold(&lines.join("\n")) {
        Err(UnfoldError::Alphabet { line, .. }) => assert_eq!(line, last + 1),
        other => panic!("{other:?}"),
    }
}
