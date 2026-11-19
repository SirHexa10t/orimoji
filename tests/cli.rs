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
    let (ok, back, _) = orimoji(&["unfold", after.to_str().unwrap()], b"");
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
