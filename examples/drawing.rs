//! An arterminal drawing folded a line at a time: smaller, and still a line for every row, so a
//! repository diffs the folded file the way it would diff the drawing. The fold may index the
//! drawing's colours; the index comes back beside the folded text, for the caller to keep.
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
        let folded =
            orimoji::fold(drawing.as_bytes(), &Fold::new(Squeeze::Text, alphabet).indexed())?;
        let index = folded.indexing.index();
        assert_eq!(orimoji::unfold(&folded.text, index)?, drawing.as_bytes());
        let kept = index.map_or(0, |index| index.to_text().len());
        println!(
            "text squeeze into {alphabet:<9}: {:>6} bytes and an index of {kept:>4} ({:>3.0}%), {:>6} cells wide",
            folded.text.len(),
            100.0 * (folded.text.len() + kept) as f64 / drawing.len() as f64,
            orimoji::measure::cells(&folded.text),
        );
    }

    // Repaint a row: with the tables and the index kept, only that row's line changes.
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
    let before = orimoji::fold(drawing.as_bytes(), &how)?;
    let index = before.indexing.index();
    let mut rows: Vec<String> = drawing.split('\n').map(str::to_string).collect();
    let row = rows.len() / 2;
    rows[row] = rows[row].replacen(' ', "▒", 1);
    let repainted = rows.join("\n");
    let after = orimoji::fold_keeping(repainted.as_bytes(), &before.text, index, &how)?;
    assert_eq!(orimoji::unfold(&after, index)?, repainted.as_bytes());
    let changed: Vec<usize> = before
        .text
        .lines()
        .zip(after.lines())
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(at, _)| at + 1)
        .collect();
    println!(
        "\nrepainting row {} changed the folded text's line {changed:?} and no other",
        row + 1
    );

    if let Some(index) = index {
        println!("\nthe index, as the caller would keep it ({} values):\n", index.len());
        for line in index.to_text().lines().take(4) {
            println!("  {line}");
        }
    }
    println!("\nthe folded text, from its body on:\n");
    let body = after.split("=== body ===\n").nth(1).unwrap_or_default();
    for line in body.lines().take(6) {
        println!("  {line}");
    }
    Ok(())
}
