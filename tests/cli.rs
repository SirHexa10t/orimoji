//! The `orimoji` binary, run the way a person or a script would.

use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Runs orimoji with `args` and `stdin`: whether it succeeded, its output, and what it said.
fn orimoji(args: &[&str], stdin: &[u8]) -> (bool, Vec<u8>, String) {
    orimoji_in(Path::new("."), args, stdin)
}

/// Runs orimoji in the folder `dir`, as [`orimoji`] does.
fn orimoji_in(dir: &Path, args: &[&str], stdin: &[u8]) -> (bool, Vec<u8>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_orimoji"))
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    child.stdin.take().expect("piped").write_all(stdin).expect("stdin takes it");
    let out = child.wait_with_output().expect("it finishes");
    (out.status.success(), out.stdout, String::from_utf8_lossy(&out.stderr).into_owned())
}

/// A folder of this test's own, empty.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orimoji-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a temporary folder");
    dir
}

/// A path as an argument.
fn arg(path: &Path) -> &str {
    path.to_str().expect("a UTF-8 path")
}

const LISTING: &[u8] = b"# app_id\tname\tdevs\n10\tCounter-Strike\tValve\n20\tTeam Fortress Classic\tValve\n50\tHalf-Life: Opposing Force\tGearbox Software\n";

/// Two short texts, much alike.
const NOTES: [&str; 2] = [
    "the cat sat on the mat\nthe dog sat on the log\n",
    "the cat ran to the mat\nthe dog ran to the log\n",
];

/// The example at `examples/data/<name>`.
fn example(name: &str) -> String {
    let path = format!("{}/examples/data/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).expect("an example")
}

/// Chapter `n` of Genesis.
fn chapter(n: usize) -> String {
    example(&format!("biblical_text_multiple/chapter_{n:02}.txt"))
}

/// A table of `300` rows, numbered from `offset`, of three studios.
fn rows(offset: usize) -> String {
    (0..300)
        .map(|i| format!("{}\t{}\n", i + offset, ["Valve", "Gearbox Software", "Bethesda"][i % 3]))
        .collect()
}

#[test]
fn whatever_is_folded_on_the_command_line_unfolds_there() {
    for squeeze in ["auto", "none", "pack", "text", "table", "deflate", "zstd"] {
        for alphabet in ["base64url", "two-byte", "cjk"] {
            let (ok, folded, said) =
                orimoji(&["fold", "-q", "-s", squeeze, "-a", alphabet], LISTING);
            assert!(ok, "{squeeze} {alphabet}: {said}");
            assert!(said.is_empty(), "--quiet says nothing");
            let (ok, back, said) = orimoji(&["unfold"], &folded);
            assert!(ok, "{squeeze} {alphabet}: {said}");
            assert_eq!(back, LISTING, "{squeeze} {alphabet}");
        }
    }
}

#[test]
fn a_fold_says_on_standard_error_how_it_went() {
    let (ok, _, said) = orimoji(&["fold", "-s", "table"], LISTING);
    assert!(ok);
    assert!(said.contains("folded into") && said.contains("table into base64url"), "{said}");
}

/// Folded like an earlier fold, keeping its tables (or, for the text squeeze, its unit), a text
/// changed in one line folds into a file changed in one line.
#[test]
fn folding_like_an_earlier_fold_changes_only_the_lines_that_changed() {
    let dir = scratch("like");
    for (squeeze, suffix) in [("table", "folded"), ("text", "ori")] {
        let before = dir.join(format!("before.{suffix}"));
        let after = dir.join(format!("after.{suffix}"));
        let (ok, _, said) = orimoji(&["fold", "-q", "-s", squeeze, "-o", arg(&before)], LISTING);
        assert!(ok, "{said}");
        let changed = String::from_utf8_lossy(LISTING).replace("Gearbox Software", "Gearbox");
        let (ok, _, said) =
            orimoji(&["fold", "-q", "--like", arg(&before), "-o", arg(&after)], changed.as_bytes());
        assert!(ok, "{said}");
        let (old, new) =
            (std::fs::read_to_string(&before).unwrap(), std::fs::read_to_string(&after).unwrap());
        assert_eq!(old.lines().count(), new.lines().count());
        let differing = old.lines().zip(new.lines()).filter(|(a, b)| a != b).count();
        assert_eq!(differing, 1, "{squeeze}: only the changed row's line differs:\n{old}\n{new}");
        let (ok, back, _) = orimoji(&["unfold", arg(&after), "-o", "-"], b"");
        assert!(ok);
        assert_eq!(back, changed.as_bytes());
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn what_cant_be_done_says_why_and_fails() {
    let (ok, _, said) = orimoji(&["unfold"], b"just some text\n");
    assert!(!ok && said.contains("isn't a folded file"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "-a", "klingon"], b"x");
    assert!(!ok && said.contains("klingon"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "-s", "text"], &[0xFF, 0xFE, b'\n']);
    assert!(!ok && said.contains("UTF-8"), "{said}");
}

#[test]
fn the_alphabets_and_a_measurement_are_printed_as_tables() {
    let (ok, out, _) = orimoji(&["alphabets"], b"");
    let out = String::from_utf8(out).unwrap();
    assert!(ok && out.lines().count() == 14, "{out}");
    assert!(out.contains("two-byte") && out.contains("braille"));
    let (ok, out, said) = orimoji(&["measure", "-a", "base64url"], LISTING);
    let out = String::from_utf8(out).unwrap();
    assert!(ok, "{said}");
    assert!(
        out.contains("(the input itself)") && out.contains("table") && out.contains("zstd"),
        "{out}"
    );
}

/// Text files given together fold together: an `.ori` for each, and their unit in an `.orinv`
/// named after their folder, which they unfold with where they name it, or given with --orinv.
#[test]
fn text_files_fold_together_and_unfold_on_the_command_line() {
    let dir = scratch("together");
    let notes = dir.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    let (a, b) = (notes.join("a.txt"), notes.join("b.txt"));
    std::fs::write(&a, chapter(1)).unwrap();
    std::fs::write(&b, chapter(2)).unwrap();
    let (ok, _, said) = orimoji(&["fold", "-s", "text", "--index", arg(&a), arg(&b)], b"");
    assert!(ok, "{said}");
    let orinv = notes.join("notes.orinv");
    let written = format!("2 files written, their unit in {}", orinv.display());
    assert!(said.contains(&written), "{said}");
    std::fs::remove_file(&a).unwrap();
    std::fs::remove_file(&b).unwrap();
    let (a_ori, b_ori) = (notes.join("a.txt.ori"), notes.join("b.txt.ori"));
    // A file at a time, a stream, from its folder or from another.
    let (ok, back, said) = orimoji(&["unfold", arg(&a_ori), "-o", "-"], b"");
    assert!(ok && back == chapter(1).as_bytes(), "{said}");
    let b_text = std::fs::read(&b_ori).unwrap();
    let (ok, back, said) = orimoji_in(&notes, &["unfold"], &b_text);
    assert!(ok && back == chapter(2).as_bytes(), "{said}");
    let (ok, _, said) = orimoji_in(&dir, &["unfold"], &b_text);
    assert!(!ok && said.contains("which is missing") && said.contains("--orinv FILE"), "{said}");
    // Moved away, the .orinv is missed, and given.
    let moved = dir.join("moved.orinv");
    std::fs::rename(&orinv, &moved).unwrap();
    let (ok, _, said) = orimoji(&["unfold", arg(&a_ori), arg(&b_ori)], b"");
    assert!(!ok && said.contains("which is missing") && said.contains("--orinv FILE"), "{said}");
    let (ok, _, said) = orimoji(&["unfold", "--orinv", arg(&moved), arg(&a_ori), arg(&b_ori)], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), chapter(1));
    assert_eq!(std::fs::read_to_string(&b).unwrap(), chapter(2));
    std::fs::remove_dir_all(dir).unwrap();
}

/// The .orinv of files folded together goes in --orinv-dir when given; it is refused for anything
/// else, which has no .orinv.
#[test]
fn the_orinv_goes_where_it_is_told_and_only_text_files_folded_together_have_one() {
    let dir = scratch("orinv-dir");
    let (a, b, c, units) =
        (dir.join("a.txt"), dir.join("b.txt"), dir.join("c.txt"), dir.join("units"));
    std::fs::write(&a, chapter(1)).unwrap();
    std::fs::write(&b, chapter(2)).unwrap();
    std::fs::write(&c, chapter(3)).unwrap();
    let mut args = vec!["fold", "-q", "-s", "text", "--index", "--orinv-dir", arg(&units)];
    args.extend([arg(&a), arg(&b), arg(&c)]);
    let (ok, _, said) = orimoji(&args, b"");
    assert!(ok, "{said}");
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(units.join(format!("{name}.orinv")).exists(), "named after the files' folder");
    let ori = std::fs::read_to_string(dir.join("a.txt.ori")).unwrap();
    assert!(ori.starts_with(&format!("=== orimoji 3 text base64url orinv=units/{name}.orinv")));
    for refused in [
        vec!["fold", "-s", "text", "--orinv-dir", arg(&units)],
        vec!["fold", "-s", "table", "--orinv-dir", arg(&units), arg(&a), arg(&b)],
    ] {
        let (ok, _, said) = orimoji(&refused, NOTES[0].as_bytes());
        assert!(!ok && said.contains("--orinv-dir places the .orinv"), "{said}");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A set folded into a folder whose .orinv another set still unfolds with is refused, naming
/// them, and nothing is written; a file joins that set by folding --like its .orinv.
#[test]
fn an_orinv_in_use_is_not_replaced_and_a_file_joins_its_set_like_it() {
    let dir = scratch("in-use");
    let write = |name: &str, text: &str| {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let fold = |paths: &[PathBuf]| {
        let mut args = vec!["fold", "-s", "text", "--index"];
        args.extend(paths.iter().map(|path| arg(path)));
        orimoji(&args, b"")
    };
    let first: Vec<PathBuf> = (1..=3).map(|n| write(&format!("{n}.txt"), &chapter(n))).collect();
    let (ok, _, said) = fold(&first);
    assert!(ok && said.contains("their unit in"), "{said}");
    let name = dir.file_name().unwrap().to_str().unwrap();
    let orinv = dir.join(format!("{name}.orinv"));
    let others: Vec<PathBuf> = (4..=6).map(|n| write(&format!("{n}.txt"), &chapter(n))).collect();
    let (ok, _, said) = fold(&others);
    assert!(!ok && said.contains("so it isn't replaced") && said.contains("1.txt.ori"), "{said}");
    assert!(!dir.join("4.txt.ori").exists());
    let seventh = write("7.txt", &chapter(7));
    let (ok, _, said) = orimoji(&["fold", "--like", arg(&orinv), arg(&seventh)], b"");
    assert!(ok && said.contains(&format!("their unit in {}", orinv.display())), "{said}");
    std::fs::remove_file(&seventh).unwrap();
    let (ok, _, said) = orimoji(&["unfold", arg(&dir.join("7.txt.ori"))], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(&seventh).unwrap(), chapter(7));
    std::fs::remove_dir_all(dir).unwrap();
}

/// Files of any other squeeze fold each on its own, the index inside each, and unfold alone.
#[test]
fn files_of_other_squeezes_keep_their_index_inside() {
    let dir = scratch("files");
    let (a, b) = (dir.join("a.tsv"), dir.join("b.tsv"));
    std::fs::write(&a, rows(0)).unwrap();
    std::fs::write(&b, rows(1000)).unwrap();
    let (ok, _, said) = orimoji(&["fold", "-s", "table", "--index", arg(&a), arg(&b)], b"");
    assert!(ok, "{said}");
    assert!(said.contains("2 files written") && !said.contains("unit"), "{said}");
    assert!(said.contains("each file keeps its own index with its code"), "{said}");
    std::fs::remove_file(&a).unwrap();
    std::fs::remove_file(&b).unwrap();
    let (a_folded, b_folded) = (dir.join("a.tsv.folded"), dir.join("b.tsv.folded"));
    let (ok, _, said) = orimoji(&["unfold", arg(&a_folded), arg(&b_folded)], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), rows(0));
    assert_eq!(std::fs::read_to_string(&b).unwrap(), rows(1000));
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file folded the old way, its index beside it (format 1), unfolds with --index FILE, and
/// says so when it isn't given; folding like it says to fold it afresh.
#[test]
fn a_file_with_its_index_beside_it_unfolds_with_index_file() {
    let dir = scratch("beside");
    let squeeze = Squeeze::Table(TableOptions::default());
    let how = Fold { index_inside: false, ..Fold::new(squeeze, Alphabet::Base64Url).indexed() };
    let folded = orimoji::fold(rows(0).as_bytes(), &how).unwrap();
    let (file, index) = (dir.join("a.tsv.folded"), dir.join("studios.index"));
    std::fs::write(&file, &folded.text).unwrap();
    std::fs::write(&index, folded.indexing.index().unwrap().to_text()).unwrap();
    let (ok, _, said) = orimoji(&["unfold", arg(&file)], b"");
    assert!(!ok && said.contains("needs that index") && said.contains("--index FILE"), "{said}");
    let (ok, _, said) = orimoji(&["unfold", "--index", arg(&index), arg(&file)], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(dir.join("a.tsv")).unwrap(), rows(0));
    let (ok, _, said) = orimoji(&["fold", "--like", arg(&file)], rows(5).as_bytes());
    assert!(!ok && said.contains("fold it afresh"), "{said}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_index_asked_for_but_not_worth_it_is_not_kept() {
    let (ok, _, said) = orimoji(&["fold", "-s", "text", "--index"], b"one line\n");
    assert!(ok && said.contains("no list paid for itself"), "{said}");
}

#[test]
fn a_measured_fold_says_what_it_found_or_that_time_ran_out() {
    let art = std::fs::read(format!(
        "{}/examples/data/long_ascii_anime_postergirl.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let listing = example("steam-games-excerpt.tsv");
    let listing = listing.as_bytes();
    // Auto, the default, measures the squeeze it chose: on the listing, the table.
    let (ok, _, said) = orimoji(&["fold", "--index", "--measure-up-to", "1h"], listing);
    assert!(ok && said.contains("with table into") && said.contains("the index pays"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "--index", "--measure-up-to", "0ms"], listing);
    assert!(ok && said.contains("longer than 0ms") && said.contains("estimates decided"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "--index", "--measure-up-to", "5x"], &art);
    assert!(!ok && said.contains("no time"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "-s", "text", "--measure-up-to", "5s"], &art);
    assert!(!ok && said.contains("--index"), "measuring needs an index to weigh: {said}");
    // The text squeeze weighs both ways always, so it has nothing to measure; nor has auto, where
    // it chooses the text squeeze, as on the art.
    for squeeze in ["text", "auto"] {
        let (ok, _, said) =
            orimoji(&["fold", "-s", squeeze, "--index", "--measure-up-to", "1h"], &art);
        assert!(ok && said.contains("with text into"), "{said}");
        assert!(!said.contains("measur") && said.contains("no list paid"), "{said}");
    }
}

/// Files unlike each other fold each alone, as if given alone, and the fold says so; where some
/// share and some don't, it says how many share.
#[test]
fn files_that_take_less_alone_fold_alone_and_the_fold_says_so() {
    let dir = scratch("alone");
    let write = |name: &str, text: &str| {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let art = write("art.txt", &example("long_ascii_anime_postergirl.txt"));
    let miku = write("miku.txt", &example("small_filled_blocks_miku.txt"));
    let (ok, _, said) = orimoji(&["fold", "-s", "text", "--index", arg(&art), arg(&miku)], b"");
    assert!(
        ok && said.contains("each with its own unit: folded together, they would take"),
        "{said}"
    );
    assert!(said.contains("each file keeps its own index"), "{said}");
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(!dir.join(format!("{name}.orinv")).exists());
    let chapters: Vec<PathBuf> = (1..=4).map(|n| write(&format!("{n}.txt"), &chapter(n))).collect();
    let mut args = vec!["fold", "-s", "text", "--index", arg(&art)];
    args.extend(chapters.iter().map(|path| arg(path)));
    args.push(arg(&miku));
    let (ok, _, said) = orimoji(&args, b"");
    assert!(ok, "{said}");
    let orinv = dir.join(format!("{name}.orinv"));
    let shared = format!("4 of them sharing their unit in {}", orinv.display());
    assert!(said.contains(&shared) && said.contains("the rest each with its own"), "{said}");
    assert!(said.contains("those folded alone keep their own"), "{said}");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Auto folds a drawing into an `.ori`, where the text squeeze takes least, and it unfolds back.
#[test]
fn auto_folds_a_drawing_into_an_ori() {
    let dir = scratch("auto-ori");
    let drawing = dir.join("drawing.txt");
    std::fs::write(&drawing, example("small_braille_kirino.txt")).unwrap();
    let (ok, _, said) = orimoji(&["fold", arg(&drawing)], b"");
    assert!(ok && said.contains("with text into base64url"), "{said}");
    let ori = dir.join("drawing.txt.ori");
    assert!(std::fs::read_to_string(&ori).unwrap().starts_with("=== orimoji 3 text base64url ==="));
    std::fs::remove_file(&drawing).unwrap();
    let (ok, _, said) = orimoji(&["unfold", arg(&ori)], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(&drawing).unwrap(), example("small_braille_kirino.txt"));
    std::fs::remove_dir_all(dir).unwrap();
}
