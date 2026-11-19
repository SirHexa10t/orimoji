//! Several drawings folded together into one index, kept once for them all, against each folded
//! with an index of its own.
//!
//! ```sh
//! cargo run --example shared_index
//! cargo run --example shared_index -- a.txt b.txt c.txt
//! ```

use orimoji::{Alphabet, Fold, Squeeze};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data");
        paths = [
            "long_ascii_anime_postergirl.txt",
            "small_braille_kirino.txt",
            "small_filled_blocks_miku.txt",
        ]
        .iter()
        .map(|name| format!("{examples}/{name}"))
        .collect();
    }
    let inputs: Vec<Vec<u8>> = paths.iter().map(std::fs::read).collect::<Result<_, _>>()?;
    let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
    let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();

    let together = orimoji::fold_all(&refs, &how)?;
    let index = together.indexing.index();
    let shared = index.map_or(0, |index| index.to_text().len());
    let (mut alone_total, mut together_total) = (0, shared);
    for ((path, input), text) in paths.iter().zip(&inputs).zip(&together.texts) {
        assert_eq!(&orimoji::unfold(text, index)?, input);
        let alone = orimoji::fold(input, &how)?;
        let own = alone.indexing.index().map_or(0, |index| index.to_text().len());
        alone_total += alone.text.len() + own;
        together_total += text.len();
        let needs = if orimoji::needs_index(text)? { "needs the index" } else { "needs no index" };
        println!(
            "{path}: {} bytes, folded into {} ({needs}); alone, {} and an index of {own}",
            input.len(),
            text.len(),
            alone.text.len()
        );
    }
    println!(
        "\none shared index of {shared} bytes: {together_total} bytes in all, against {alone_total} with an index each"
    );
    Ok(())
}
