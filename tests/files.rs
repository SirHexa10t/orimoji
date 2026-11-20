//! Files folded and unfolded beside themselves and over themselves.

use orimoji::files::{self, FilesError, Output};
use orimoji::{Alphabet, Fold, Judged, Squeeze, TableOptions, UnfoldError};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A folder of this test's own, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orimoji-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

/// The example at `examples/data/<name>`.
fn example(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR")))
        .expect("an example")
}

/// Chapter `n` of Genesis.
fn chapter(n: usize) -> String {
    String::from_utf8(example(&format!("biblical_text_multiple/chapter_{n:02}.txt"))).unwrap()
}

/// `bytes` written to `dir/name`.
fn put(dir: &Path, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("a copy");
    path
}

/// Two of the example drawings, copied in as `a.txt` and `b.txt`.
fn drawings(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    ["small_filled_blocks_miku.txt", "long_ascii_anime_postergirl.txt"]
        .iter()
        .zip(["a.txt", "b.txt"])
        .map(|(example_name, name)| {
            let bytes = example(example_name);
            (put(dir, name, &bytes), bytes)
        })
        .collect()
}

/// The first `count` chapters of Genesis, copied in as `a.txt`, `b.txt` and on.
fn chapters(dir: &Path, count: usize) -> Vec<(PathBuf, Vec<u8>)> {
    (0..count)
        .map(|at| {
            let bytes = chapter(at + 1).into_bytes();
            let name = format!("{}.txt", (b'a' + at as u8) as char);
            (put(dir, &name, &bytes), bytes)
        })
        .collect()
}

/// Text files that spell alike, folded together, share one unit in an `.orinv` named after their
/// folder, and unfold back beside themselves.
#[test]
fn files_folded_beside_themselves_unfold_back_beside_those() {
    let dir = scratch("beside");
    let originals = chapters(&dir, 3);
    let paths: Vec<PathBuf> = originals.iter().map(|(path, _)| path.clone()).collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    let ori = [dir.join("a.txt.ori"), dir.join("b.txt.ori"), dir.join("c.txt.ori")];
    assert_eq!(folded.written, ori);
    assert_eq!(folded.sharing, ori, "chapters of one book share their unit");
    let name = dir.file_name().unwrap().to_string_lossy();
    assert_eq!(folded.orinv, Some(dir.join(format!("{name}.orinv"))), "named after their folder");
    assert_eq!(folded.judged, Judged::Weighed);
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "the originals are left alone");
        std::fs::remove_file(path).unwrap();
    }
    assert!(folded.indexing.index().is_some(), "their common words are worth an index");
    let back = files::unfold_files(&folded.written, None, None, &Output::default()).unwrap();
    assert_eq!(back, paths);
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Files unlike each other take less each alone, as if each were given alone: there is no
/// `.orinv`, each `.ori` holds its own unit, and together they would take more.
#[test]
fn files_unlike_each_other_fold_each_alone() {
    let dir = scratch("unlike");
    let originals = drawings(&dir);
    let paths: Vec<PathBuf> = originals.iter().map(|(path, _)| path.clone()).collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!((&folded.orinv, folded.sharing.len()), (&None, 0));
    let mut size = 0;
    for ((path, bytes), written) in originals.iter().zip(&folded.written) {
        let text = std::fs::read_to_string(written).unwrap();
        size += text.len();
        let alone = std::str::from_utf8(bytes).unwrap();
        let by_itself = orimoji::fold(bytes, &how).unwrap();
        assert_eq!(text, by_itself.text, "{}: as if given alone", path.display());
        assert_eq!(orimoji::unfold(&text, None).unwrap(), alone.as_bytes());
    }
    let texts: Vec<&str> =
        originals.iter().map(|(_, bytes)| std::str::from_utf8(bytes).unwrap()).collect();
    let together = orimoji::parts::fold_together(&texts, Alphabet::Base64Url, true);
    assert!(size < together.size(), "{size} alone, {} together without headers", together.size());
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file unlike the rest folds alone, as if given alone, and the rest share their unit: the
/// `.orinv` goes beside the first of those that share it, named after its folder, even where the
/// file folded alone was given first.
#[test]
fn a_file_unlike_the_rest_folds_alone_and_the_rest_share() {
    let dir = scratch("one-unlike");
    let (book, art) = (dir.join("book"), dir.join("art"));
    std::fs::create_dir_all(&book).unwrap();
    std::fs::create_dir_all(&art).unwrap();
    let drawing = put(&art, "drawing.txt", example("long_ascii_anime_postergirl.txt"));
    let mut paths = vec![drawing.clone()];
    paths.extend(chapters(&book, 4).into_iter().map(|(path, _)| path));
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!(folded.orinv, Some(book.join("book.orinv")));
    assert_eq!(folded.sharing, folded.written[1..], "the chapters share");
    let alone = std::fs::read_to_string(&folded.written[0]).unwrap();
    assert!(alone.starts_with("=== orimoji 3 text base64url ===\n=== unit ==="), "{alone}");
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    files::unfold_files(&folded.written, None, None, &Output::default()).unwrap();
    assert_eq!(std::fs::read(&drawing).unwrap(), example("long_ascii_anime_postergirl.txt"));
    assert_eq!(std::fs::read_to_string(&paths[4]).unwrap(), chapter(4));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn files_folded_in_place_unfold_in_place_and_keep_their_permissions() {
    let dir = scratch("in-place");
    let originals = chapters(&dir, 2);
    let paths: Vec<PathBuf> = originals.iter().map(|(path, _)| path.clone()).collect();
    std::fs::set_permissions(&paths[0], std::fs::Permissions::from_mode(0o640)).unwrap();
    let how = Fold::new(Squeeze::Text, Alphabet::TwoByte).indexed();
    let folded = files::fold_files(&paths, &how, &Output::InPlace).unwrap();
    assert_eq!(folded.written, paths);
    let text = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(text.starts_with("=== orimoji 3 text two-byte orinv="), "{text}");
    files::unfold_files(&paths, None, None, &Output::InPlace).unwrap();
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    let mode = std::fs::metadata(&paths[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    let leftovers = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(leftovers, 3, "the two, and their .orinv: no temporary file is left behind");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_file_that_fails_leaves_every_file_as_it_was() {
    let dir = scratch("fails");
    let good = dir.join("good.txt");
    let bad = dir.join("bad.txt");
    std::fs::write(&good, "some text\n").unwrap();
    std::fs::write(&bad, [0xFF, 0xFE, b'\n']).unwrap();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url);
    match files::fold_files(&[&good, &bad], &how, &Output::InPlace) {
        Err(FilesError::Fold { path, .. }) => assert_eq!(path, bad),
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&good).unwrap(), "some text\n", "nothing was written");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A table's rows, numbered from `offset`, of two studios.
fn rows(offset: usize) -> String {
    (0..300).map(|i| format!("{}\t{}\n", i + offset, ["Valve", "Bethesda"][i % 2])).collect()
}

#[test]
fn folds_waiting_to_be_weighed_are_buffered_beside_the_files_and_leave_nothing_behind() {
    let dir = scratch("buffered");
    let paths = [put(&dir, "a.tsv", rows(0)), put(&dir, "b.tsv", rows(1000))];
    let how = Fold {
        buffer_past: 0,
        ..Fold::new(Squeeze::Auto(TableOptions::default()), Alphabet::Base64Url)
            .indexed()
            .measured_up_to(std::time::Duration::from_secs(3600))
    };
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert!(matches!(folded.judged, Judged::Measured { .. }), "{:?}", folded.judged);
    let mut left: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["a.tsv", "a.tsv.folded", "b.tsv", "b.tsv.folded"]);
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    files::unfold_files(&folded.written, None, None, &Output::default()).unwrap();
    assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), rows(1000));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Auto folds each file the way that takes least alone, and the text files among them that take
/// less sharing one unit together: chapters into `.ori` files and an `.orinv`, a table as a table,
/// and a drawing into an `.ori` of its own.
#[test]
fn auto_folds_together_the_text_files_that_share_and_the_rest_as_it_chose() {
    let dir = scratch("auto");
    let mut paths: Vec<PathBuf> = chapters(&dir, 3).into_iter().map(|(path, _)| path).collect();
    paths.push(put(&dir, "listing.tsv", rows(0)));
    paths.push(put(&dir, "drawing.txt", example("small_braille_kirino.txt")));
    let how = Fold::new(Squeeze::Auto(TableOptions::default()), Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    let names: Vec<String> = folded
        .written
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        ["a.txt.ori", "b.txt.ori", "c.txt.ori", "listing.tsv.folded", "drawing.txt.ori"]
    );
    assert_eq!(folded.sharing, folded.written[..3]);
    let name = dir.file_name().unwrap().to_string_lossy();
    assert_eq!(folded.orinv, Some(dir.join(format!("{name}.orinv"))));
    // Told where, the .orinv goes there.
    let units = dir.join("units");
    let elsewhere = Fold { orinv_dir: Some(units.clone()), ..how.clone() };
    let again = files::fold_files(&paths, &elsewhere, &Output::Suffix(".again".into())).unwrap();
    assert_eq!(again.orinv, Some(units.join(format!("{name}.orinv"))));
    let table = std::fs::read_to_string(&folded.written[3]).unwrap();
    assert!(table.starts_with("=== orimoji 2 table base64url"), "{table}");
    let drawing = std::fs::read_to_string(&folded.written[4]).unwrap();
    assert!(drawing.starts_with("=== orimoji 3 text base64url ===\n"), "{drawing}");
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    files::unfold_files(&folded.written, None, None, &Output::default()).unwrap();
    assert_eq!(std::fs::read_to_string(&paths[2]).unwrap(), chapter(3));
    assert_eq!(std::fs::read_to_string(&paths[3]).unwrap(), rows(0));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The text squeeze's files of one folder, folded together, share one unit, kept in an `.orinv`
/// named after their folder; each `.ori` names it, and every file unfolds with it, given or found.
#[test]
fn text_files_folded_together_share_an_orinv_named_after_their_folder() {
    let dir = scratch("together");
    let chapters = dir.join("chapters");
    std::fs::create_dir_all(&chapters).unwrap();
    let texts = [chapter(1), chapter(2)];
    let paths: Vec<PathBuf> = texts
        .iter()
        .enumerate()
        .map(|(at, text)| put(&chapters, &format!("chapter_{at}.txt"), text))
        .collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    let orinv = chapters.join("chapters.orinv");
    assert_eq!(folded.orinv.as_ref(), Some(&orinv));
    for written in &folded.written {
        let first = std::fs::read_to_string(written).unwrap();
        assert!(first.starts_with("=== orimoji 3 text base64url orinv=chapters.orinv unit="));
    }
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    // Moved away, the .orinv is given instead.
    let moved = dir.join("kept.orinv");
    std::fs::rename(&orinv, &moved).unwrap();
    match files::unfold_files(&folded.written, None, None, &Output::default()) {
        Err(FilesError::Unfold { path, error: UnfoldError::NeedsOrinv(named) }) => {
            assert_eq!(path, folded.written[0]);
            assert_eq!(named, chapters.join("chapters.orinv").display().to_string());
        }
        other => panic!("{other:?}"),
    }
    // Another set, folded there meanwhile, put its own unit in that place: refused too.
    let pair = [put(&chapters, "x.txt", chapter(10)), put(&chapters, "y.txt", chapter(11))];
    let other = files::fold_files(&pair, &how, &Output::Suffix(".else".into())).unwrap();
    assert_eq!(other.orinv.as_ref(), Some(&orinv));
    match files::unfold_files(&folded.written, None, None, &Output::default()) {
        Err(FilesError::Unfold { error: UnfoldError::WrongOrinv, .. }) => {}
        other => panic!("{other:?}"),
    }
    std::fs::remove_file(&orinv).unwrap();
    files::unfold_files(&folded.written, None, Some(&moved), &Output::default()).unwrap();
    for (path, text) in paths.iter().zip(texts) {
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// Files in different folders keep their `.orinv` beside the first of them, named after its
/// folder; the others name it by a path from their own folder. `orinv_dir` puts it elsewhere.
#[test]
fn scattered_files_keep_their_orinv_beside_the_first() {
    let dir = scratch("scattered");
    let (a, b) = (dir.join("a"), dir.join("b"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let paths = [put(&a, "x.txt", chapter(1)), put(&b, "y.txt", chapter(2))];
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!(folded.orinv, Some(a.join("a.orinv")));
    let second = std::fs::read_to_string(&folded.written[1]).unwrap();
    assert!(
        second.starts_with("=== orimoji 3 text base64url orinv=../a/a.orinv unit="),
        "{second}"
    );
    let elsewhere = dir.join("units");
    let how = Fold { orinv_dir: Some(elsewhere.clone()), ..how };
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!(folded.orinv, Some(elsewhere.join("a.orinv")), "there, still named after a");
    let back = files::unfold_files(&folded.written, None, None, &Output::Suffix(".ori".into()));
    assert_eq!(back.unwrap(), paths);
    std::fs::remove_dir_all(dir).unwrap();
}

/// An `.orinv` other files still unfold with is never replaced by another unit: folding a
/// different set into the same folder is refused, naming them; folding the whole set again is
/// not; and a file joins a set by keeping its unit, which leaves the `.orinv` as it was.
#[test]
fn an_orinv_other_files_need_is_kept_and_a_file_can_join_its_set() {
    let dir = scratch("orphans");
    let write = |name: &str, text: &str| {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let first = [write("a.txt", &chapter(1)), write("b.txt", &chapter(2))];
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&first, &how, &Output::default()).unwrap();
    let orinv = folded.orinv.clone().unwrap();
    let unit = std::fs::read_to_string(&orinv).unwrap();
    let others = [write("c.txt", &chapter(3)), write("d.txt", &chapter(4))];
    match files::fold_files(&others, &how, &Output::default()) {
        Err(FilesError::WouldOrphan { orinv: refused, files }) => {
            assert_eq!(refused, orinv);
            assert_eq!(files, folded.written, "a.txt.ori and b.txt.ori still need it");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&orinv).unwrap(), unit, "left as it was");
    assert!(!dir.join("c.txt.ori").exists(), "nothing was written");
    // All four together: every file that needs the old unit is folded again, so it is replaced.
    let all = [first[0].clone(), first[1].clone(), others[0].clone(), others[1].clone()];
    files::fold_files(&all, &how, &Output::default()).unwrap();
    let unit = std::fs::read_to_string(&orinv).unwrap();
    // A fifth joins them, keeping their unit: the .orinv is untouched.
    let fifth = write("e.txt", &chapter(5));
    let joined = files::fold_files_like(&[&fifth], &orinv, &how, &Output::default()).unwrap();
    assert_eq!(joined.orinv.as_ref(), Some(&orinv));
    assert_eq!(std::fs::read_to_string(&orinv).unwrap(), unit);
    std::fs::remove_file(&fifth).unwrap();
    files::unfold_files(&joined.written, None, None, &Output::default()).unwrap();
    assert_eq!(std::fs::read_to_string(&fifth).unwrap(), chapter(5));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Files of any other squeeze fold each on its own, their index inside them: no index file, and
/// every one unfolds alone.
#[test]
fn files_of_other_squeezes_keep_each_its_own_index_inside() {
    let dir = scratch("table");
    let paths = [dir.join("a.tsv"), dir.join("b.tsv")];
    std::fs::write(&paths[0], rows(0)).unwrap();
    std::fs::write(&paths[1], rows(1000)).unwrap();
    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!(folded.orinv, None);
    for written in &folded.written {
        let text = std::fs::read_to_string(written).unwrap();
        assert!(text.starts_with("=== orimoji 2 table base64url index="), "{text}");
    }
    for path in &paths {
        std::fs::remove_file(path).unwrap();
    }
    files::unfold_files(&folded.written, None, None, &Output::default()).unwrap();
    assert_eq!(std::fs::read_to_string(&paths[1]).unwrap(), rows(1000));
    std::fs::remove_dir_all(dir).unwrap();
}
