//! The `orimoji` binary, run the way a person or a script would.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Runs orimoji with `args` and `stdin`: whether it succeeded, its output, and what it said.
fn orimoji(args: &[&str], stdin: &[u8]) -> (bool, Vec<u8>, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_orimoji"))
        .args(args)
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

const LISTING: &[u8] = b"# app_id\tname\tdevs\n10\tCounter-Strike\tValve\n20\tTeam Fortress Classic\tValve\n50\tHalf-Life: Opposing Force\tGearbox Software\n";

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

#[test]
fn folding_like_an_earlier_fold_changes_only_the_lines_that_changed() {
    let dir = scratch("like");
    let (before, after) = (dir.join("before.folded"), dir.join("after.folded"));
    let (ok, _, said) =
        orimoji(&["fold", "-q", "-s", "table", "-o", before.to_str().unwrap()], LISTING);
    assert!(ok, "{said}");
    let changed = String::from_utf8_lossy(LISTING).replace("Gearbox Software", "Gearbox");
    let (ok, _, said) = orimoji(
        &["fold", "-q", "--like", before.to_str().unwrap(), "-o", after.to_str().unwrap()],
        changed.as_bytes(),
    );
    assert!(ok, "{said}");
    let (old, new) =
        (std::fs::read_to_string(&before).unwrap(), std::fs::read_to_string(&after).unwrap());
    let differing = old.lines().zip(new.lines()).filter(|(a, b)| a != b).count();
    assert_eq!(differing, 1, "only the changed row's line differs:\n{old}\n{new}");
    let (ok, back, _) = orimoji(&["unfold", after.to_str().unwrap(), "-o", "-"], b"");
    assert!(ok);
    assert_eq!(back, changed.as_bytes());
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

#[test]
fn files_and_the_index_they_share_fold_and_unfold_on_the_command_line() {
    let dir = scratch("files");
    let rows = |offset: usize| -> String {
        (0..300)
            .map(|i| {
                format!("{}\t{}\n", i + offset, ["Valve", "Gearbox Software", "Bethesda"][i % 3])
            })
            .collect()
    };
    let (a, b, index) = (dir.join("a.tsv"), dir.join("b.tsv"), dir.join("studios.index"));
    std::fs::write(&a, rows(0)).unwrap();
    std::fs::write(&b, rows(1000)).unwrap();
    let (ok, _, said) = orimoji(
        &[
            "fold",
            "-s",
            "table",
            "--index",
            index.to_str().unwrap(),
            a.to_str().unwrap(),
            b.to_str().unwrap(),
        ],
        b"",
    );
    assert!(ok, "{said}");
    assert!(said.contains("2 files written") && said.contains("the index, 3 values"), "{said}");
    let kept = std::fs::read_to_string(&index).unwrap();
    assert_eq!(kept, "=== orimoji index 1 ===\nBethesda\nGearbox Software\nValve\n");
    std::fs::remove_file(&a).unwrap();
    std::fs::remove_file(&b).unwrap();
    let (a_folded, b_folded) = (dir.join("a.tsv.folded"), dir.join("b.tsv.folded"));
    let folded = [a_folded.to_str().unwrap(), b_folded.to_str().unwrap()];
    let (ok, _, said) = orimoji(&["unfold", folded[0], folded[1]], b"");
    assert!(!ok && said.contains("needs that index") && said.contains("--index"), "{said}");
    let (ok, _, said) =
        orimoji(&["unfold", "--index", index.to_str().unwrap(), folded[0], folded[1]], b"");
    assert!(ok, "{said}");
    assert_eq!(std::fs::read_to_string(&a).unwrap(), rows(0));
    assert_eq!(std::fs::read_to_string(&b).unwrap(), rows(1000));
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn an_index_asked_for_but_not_worth_it_is_not_written() {
    let dir = scratch("no-index");
    let index = dir.join("unused.index");
    let (ok, _, said) =
        orimoji(&["fold", "-s", "text", "--index", index.to_str().unwrap()], b"one line\n");
    assert!(ok && said.contains("no list paid for itself"), "{said}");
    assert!(!index.exists());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_measured_fold_says_what_it_found_or_that_time_ran_out() {
    let dir = scratch("measure");
    let art = std::fs::read(format!(
        "{}/examples/data/long_ascii_anime_postergirl.txt",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let index = dir.join("art.index");
    let index = index.to_str().unwrap();
    let (ok, _, said) =
        orimoji(&["fold", "-s", "text", "--index", index, "--measure-up-to", "1h"], &art);
    assert!(ok && said.contains("measured") && said.contains("doesn't pay"), "{said}");
    let (ok, _, said) =
        orimoji(&["fold", "-s", "text", "--index", index, "--measure-up-to", "0ms"], &art);
    assert!(ok && said.contains("longer than 0ms") && said.contains("estimates decided"), "{said}");
    let (ok, _, said) =
        orimoji(&["fold", "-s", "text", "--index", index, "--measure-up-to", "5x"], &art);
    assert!(!ok && said.contains("no time"), "{said}");
    let (ok, _, said) = orimoji(&["fold", "-s", "text", "--measure-up-to", "5s"], &art);
    assert!(!ok && said.contains("--index"), "measuring needs an index to weigh: {said}");
    // Auto, the default, measures the squeeze it chose.
    let (ok, _, said) = orimoji(&["fold", "--index", index, "--measure-up-to", "1h"], &art);
    assert!(ok && said.contains("with text into") && said.contains("doesn't pay"), "{said}");
    std::fs::remove_dir_all(dir).unwrap();
}
