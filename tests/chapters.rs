//! Genesis, a chapter a file (`examples/data/biblical_text_multiple`): texts folded together into
//! one index, as files share one.

use orimoji::{Alphabet, Fold, Squeeze, parts};

fn folder() -> String {
    format!("{}/examples/data/biblical_text_multiple", env!("CARGO_MANIFEST_DIR"))
}

fn chapters() -> Vec<Vec<u8>> {
    (1..=50)
        .map(|n| std::fs::read(format!("{}/chapter_{n:02}.txt", folder())).expect("a chapter"))
        .collect()
}

/// The whole book, from its chapters: its first line empty, a blank line between chapters, and
/// no newline after its last line, where each chapter ends in one. Kept as chapters alone, since
/// the book is them joined.
fn book() -> Vec<u8> {
    let mut book = b"\n".to_vec();
    for (at, chapter) in chapters().iter().enumerate() {
        if at > 0 {
            book.extend_from_slice(b"\n\n");
        }
        book.extend_from_slice(chapter.strip_suffix(b"\n").expect("a chapter ends in a newline"));
    }
    book
}

/// FNV-1a, 64 bits.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ byte as u64).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The chapters joined are the book as it was given, before it was split: its size and its
/// fingerprint, taken from it then.
#[test]
fn the_chapters_join_into_the_whole_book() {
    assert_eq!(chapters().len(), 50);
    let book = book();
    assert_eq!((book.len(), fingerprint(&book)), (199_295, 0xdab2_50fc_2315_972f));
}

/// Folded together the old way, the index kept beside the texts (`index_inside` off), the chapters
/// share one index, which every chapter unfolds with and none unfolds without, and which is
/// smaller than the indexes they keep folded apart. Kept inside, as it is by default, each
/// chapter unfolds alone.
#[test]
fn chapters_folded_together_share_one_index() {
    let chapters = chapters();
    let inputs: Vec<&[u8]> = chapters.iter().map(Vec::as_slice).collect();
    let how =
        Fold { index_inside: false, ..Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed() };
    let together = orimoji::fold_all(&inputs, &how).expect("text folds");
    let index = together.indexing.index().expect("words worth listing");
    for (chapter, folded) in chapters.iter().zip(&together.texts) {
        assert!(orimoji::unfold(folded, Some(index)).expect("unfolds") == *chapter);
        assert!(orimoji::unfold(folded, None).is_err(), "not without the index");
    }
    let apart: usize = inputs
        .iter()
        .map(|input| {
            let folded = orimoji::fold(input, &how).expect("text folds");
            folded.indexing.index().map_or(0, |index| index.to_text().len())
        })
        .sum();
    let shared = index.to_text().len();
    assert!(shared < apart, "one index of {shared} B, where apart they take {apart} B");
    let inside = orimoji::fold(inputs[6], &Fold { index_inside: true, ..how }).expect("folds");
    assert!(orimoji::unfold(&inside.text, None).expect("alone") == chapters[6]);
}

/// Folded into parts together, the chapters share one unit, and take exactly what the whole book
/// takes folded alone: the same lines, and the same unit, learnt from the same lines. Folded
/// apart, each with a unit of its own, they take far more.
#[test]
fn chapters_folded_together_take_what_the_book_takes() {
    let chapters: Vec<String> = chapters()
        .into_iter()
        .map(|chapter| String::from_utf8(chapter).expect("the chapters are text"))
        .collect();
    let book = String::from_utf8(book()).expect("the book is text");
    let together = parts::fold_together(&chapters, Alphabet::Base64Url, true);
    let whole = parts::fold(&book, Alphabet::Base64Url, true);
    assert_eq!(together.unit, whole.unit, "one unit, learnt from the same lines");
    assert_eq!(together.size(), whole.size());
    for (lines, chapter) in together.lines.iter().zip(&chapters) {
        let back = parts::unfold(lines, &together.unit, Alphabet::Base64Url, 1 << 24);
        assert!(back.ok().as_ref() == Some(chapter));
    }
    let apart: usize =
        chapters.iter().map(|chapter| parts::fold(chapter, Alphabet::Base64Url, true).size()).sum();
    assert!(together.size() * 5 < apart * 4, "{} together, {apart} apart", together.size());
}

/// The chapters and a drawing given with them: the drawing folds alone, as if given alone, and
/// the chapters share their unit, as they do without it. Grouped so, they take less than all of
/// them together, and than all of them alone.
#[test]
fn a_drawing_among_the_chapters_folds_alone_and_the_chapters_share() {
    let mut texts: Vec<String> = chapters()
        .into_iter()
        .map(|chapter| String::from_utf8(chapter).expect("the chapters are text"))
        .collect();
    let drawing = format!("{}/examples/data/small_braille_kirino.txt", env!("CARGO_MANIFEST_DIR"));
    texts.push(std::fs::read_to_string(drawing).expect("a drawing"));
    let alphabet = Alphabet::Base64Url;
    let none = parts::Overheads::default();
    let grouped = parts::fold_grouped(&texts, alphabet, true, none);
    let shares: Vec<bool> =
        grouped.texts.iter().map(|member| matches!(member, parts::Member::Sharing(_))).collect();
    assert_eq!(shares, [vec![true; 50], vec![false]].concat());
    let without = parts::fold_together(&texts[..50], alphabet, true);
    assert_eq!(grouped.shared.as_ref(), Some(&without.unit), "the chapters' own unit");
    let together = parts::fold_together(&texts, alphabet, true).size();
    let alone: usize = texts.iter().map(|text| parts::fold(text, alphabet, true).size()).sum();
    let size = grouped.size(none);
    assert!(size < together && size < alone, "{size}: {together} together, {alone} alone");
}
