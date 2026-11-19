//! Files folded and unfolded beside themselves and over themselves.

use orimoji::files::{self, FilesError, Output};
use orimoji::{Alphabet, Fold, Squeeze};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A folder of this test's own, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orimoji-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

/// Two of the example drawings, copied in as `a.txt` and `b.txt`.
fn drawings(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    ["small_filled_blocks_miku.txt", "long_ascii_anime_postergirl.txt"]
        .iter()
        .zip(["a.txt", "b.txt"])
        .map(|(example, name)| {
            let bytes =
                std::fs::read(format!("{}/examples/data/{example}", env!("CARGO_MANIFEST_DIR")))
                    .expect("an example");
            let path = dir.join(name);
            std::fs::write(&path, &bytes).expect("a copy");
            (path, bytes)
        })
        .collect()
}

#[test]
fn files_folded_beside_themselves_unfold_back_beside_those() {
    let dir = scratch("beside");
    let originals = drawings(&dir);
    let paths: Vec<PathBuf> = originals.iter().map(|(path, _)| path.clone()).collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let folded = files::fold_files(&paths, &how, &Output::default()).unwrap();
    assert_eq!(folded.written, [dir.join("a.txt.folded"), dir.join("b.txt.folded")]);
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes, "the originals are left alone");
        std::fs::remove_file(path).unwrap();
    }
    let index = folded.indexing.index();
    assert!(index.is_some(), "the coloured drawing's escapes are worth an index");
    let back = files::unfold_files(&folded.written, index, &Output::default()).unwrap();
    assert_eq!(back, paths);
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn files_folded_in_place_unfold_in_place_and_keep_their_permissions() {
    let dir = scratch("in-place");
    let originals = drawings(&dir);
    let paths: Vec<PathBuf> = originals.iter().map(|(path, _)| path.clone()).collect();
    std::fs::set_permissions(&paths[0], std::fs::Permissions::from_mode(0o640)).unwrap();
    let how = Fold::new(Squeeze::Text, Alphabet::TwoByte);
    let folded = files::fold_files(&paths, &how, &Output::InPlace).unwrap();
    assert_eq!(folded.written, paths);
    let text = std::fs::read_to_string(&paths[0]).unwrap();
    assert!(text.starts_with("=== orimoji 1 text two-byte ==="));
    files::unfold_files(&paths, None, &Output::InPlace).unwrap();
    for (path, bytes) in &originals {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    let mode = std::fs::metadata(&paths[0]).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
    let leftovers = std::fs::read_dir(&dir).unwrap().count();
    assert_eq!(leftovers, 2, "no temporary file is left behind");
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
