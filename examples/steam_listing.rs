//! catalogames' listing of Steam's catalogue, folded as a table: each column in a model chosen for
//! it, each row still a line of its own, and a new sweep folded with the old tables so only the
//! rows that changed change.
//!
//! ```sh
//! cargo run --release --example steam_listing
//! cargo run --release --example steam_listing -- ../catalogames/data/steam/steam-games.tsv
//! ```

use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data/steam-games-excerpt.tsv").into()
    });
    let listing = std::fs::read_to_string(&path)?;
    println!("{path}: {} bytes, {} lines\n", listing.len(), listing.lines().count());

    let how = Fold::new(Squeeze::Table(TableOptions::default()), Alphabet::Base64Url);
    let started = Instant::now();
    let folded = orimoji::fold(listing.as_bytes(), &how)?;
    let folding = started.elapsed();
    let started = Instant::now();
    assert_eq!(orimoji::unfold(&folded)?, listing.as_bytes());
    println!(
        "table squeeze into base64url: {} bytes ({:.0}%), folded in {folding:.2?}, unfolded in {:.2?}",
        folded.len(),
        100.0 * folded.len() as f64 / listing.len() as f64,
        started.elapsed(),
    );

    // The model each column got, from the folded file's own tables.
    let readable = orimoji::fold(listing.as_bytes(), &Fold { readable: true, ..how.clone() })?;
    let names = listing.lines().next().unwrap_or_default().trim_start_matches("# ").split('\t');
    let models = readable.lines().filter_map(|line| line.strip_prefix("column\t"));
    println!("\ncolumn models:");
    for (name, model) in names.zip(models) {
        let model = model.split_once('\t').map_or(model, |(_, model)| model).replace('\t', " ");
        println!("  {name:<16} {model}");
    }

    // A later sweep: one game got a review. Folded with the old tables, one line changes.
    let mut rows: Vec<String> = listing.lines().map(str::to_string).collect();
    let row = rows.iter().position(|row| !row.starts_with('#')).expect("a row") + 3;
    let mut cells: Vec<String> = rows[row].split('\t').map(str::to_string).collect();
    cells[4] = (cells[4].parse::<u64>().unwrap_or(0) + 1).to_string();
    rows[row] = cells.join("\t");
    let swept = rows.join("\n") + "\n";
    let after = orimoji::fold_keeping(swept.as_bytes(), &folded, &how)?;
    assert_eq!(orimoji::unfold(&after)?, swept.as_bytes());
    let changed = folded.lines().zip(after.lines()).filter(|(a, b)| a != b).count();
    println!("\none more review for app {}: {changed} folded line changed", cells[0]);
    Ok(())
}
