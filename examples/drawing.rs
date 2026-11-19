//! An arterminal drawing folded a line at a time: smaller, and still a line for every row, so a
//! repository diffs the folded file the way it would diff the drawing.
//!
//! ```sh
//! cargo run --example drawing
//! cargo run --example drawing -- path/to/drawing.txt
//! ```

use orimoji::{Alphabet, Fold, Squeeze};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/small_filled_blocks_miku.txt").into()
    });
    let drawing = std::fs::read_to_string(&path)?;
    println!("{path}: {} bytes, {} rows\n", drawing.len(), drawing.lines().count());

    for alphabet in [Alphabet::Base64Url, Alphabet::TwoByte, Alphabet::Braille] {
        let folded = orimoji::fold(drawing.as_bytes(), &Fold::new(Squeeze::Text, alphabet))?;
        assert_eq!(orimoji::unfold(&folded)?, drawing.as_bytes());
        println!(
            "text squeeze into {alphabet:<9}: {:>6} bytes ({:>3.0}%), {:>6} cells wide in all",
            folded.len(),
            100.0 * folded.len() as f64 / drawing.len() as f64,
            orimoji::measure::cells(&folded),
        );
    }

    // Repaint a row: with the tables kept, only that row's line changes.
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url);
    let before = orimoji::fold(drawing.as_bytes(), &how)?;
    let mut rows: Vec<String> = drawing.split('\n').map(str::to_string).collect();
    let row = rows.len() / 2;
    rows[row] = rows[row].replacen(' ', "▒", 1);
    let repainted = rows.join("\n");
    let after = orimoji::fold_keeping(repainted.as_bytes(), &before, &how)?;
    assert_eq!(orimoji::unfold(&after)?, repainted.as_bytes());
    let changed: Vec<usize> = before
        .lines()
        .zip(after.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(at, _)| at + 1)
        .collect();
    println!(
        "\nrepainting row {} changed the folded file's line {changed:?} and no other",
        row + 1
    );

    println!("\nthe folded file, from its body on:\n");
    let body = after.split("=== body ===\n").nth(1).unwrap_or_default();
    for line in body.lines().take(6) {
        println!("  {line}");
    }
    Ok(())
}
