//! Several texts folded three ways, as `.ori` files: all together, sharing one unit kept once in
//! an `.orinv`; each alone, with a unit of its own; and grouped, as orimoji folds files given
//! together: those that take less sharing share, and each of the rest folds alone. By default,
//! Genesis a chapter a file, whose chapters spell much the same, and two drawings among them,
//! which don't.
//!
//! ```sh
//! cargo run --release --example folded_together
//! cargo run --release --example folded_together -- a.txt b.txt c.txt
//! ```

use orimoji::parts::{self, Member};
use orimoji::{Alphabet, ori};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        let data = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/data");
        paths =
            (1..=50).map(|n| format!("{data}/biblical_text_multiple/chapter_{n:02}.txt")).collect();
        paths.insert(20, format!("{data}/small_braille_kirino.txt"));
        paths.push(format!("{data}/long_ascii_anime_postergirl.txt"));
    }
    let texts: Vec<String> = paths.iter().map(std::fs::read_to_string).collect::<Result<_, _>>()?;
    let alphabet = Alphabet::Base64Url;
    // Named as the command line names it, after the first file's folder, beside the files.
    let first = Path::new(&paths[0]).canonicalize()?;
    let folder = first.parent().and_then(Path::file_name).unwrap_or_default().to_string_lossy();
    let name = format!("{folder}{}", ori::UNIT_SUFFIX);

    let start = std::time::Instant::now();
    let together = parts::fold_together(&texts, alphabet, true);
    let together_took = start.elapsed();
    let together_size = ori::unit_file(&together.unit, alphabet).len()
        + together
            .lines
            .iter()
            .map(|lines| ori::apart(lines, &name, &together.unit, alphabet).len())
            .sum::<usize>();
    let alone: Vec<usize> = texts
        .iter()
        .map(|text| {
            let folded = parts::fold(text, alphabet, true);
            ori::whole(&folded.lines, &folded.unit, alphabet).len()
        })
        .collect();

    let start = std::time::Instant::now();
    let overheads = orimoji::parts::Overheads {
        alone: ori::whole(&[], &together.unit, alphabet).len() - together.unit.size(),
        sharing: ori::apart(&[], &name, &together.unit, alphabet).len(),
        shared_unit: ori::unit_file(&together.unit, alphabet).len() - together.unit.size(),
    };
    let grouped = parts::fold_grouped(&texts, alphabet, true, overheads);
    let took = start.elapsed();
    let mut grouped_size =
        grouped.shared.as_ref().map_or(0, |unit| ori::unit_file(unit, alphabet).len());
    for ((path, member), alone) in paths.iter().zip(&grouped.texts).zip(&alone) {
        let file = path.rsplit('/').next().unwrap_or(path);
        let (way, size) = match member {
            Member::Sharing(lines) => {
                let unit = grouped.shared.as_ref().expect("a unit to share");
                ("shares", ori::apart(lines, &name, unit, alphabet).len())
            }
            Member::Alone(folded) => {
                ("alone ", ori::whole(&folded.lines, &folded.unit, alphabet).len())
            }
        };
        grouped_size += size;
        println!("{file}: {way} {size} bytes; alone, {alone}");
    }
    let sharing =
        grouped.texts.iter().filter(|member| matches!(member, Member::Sharing(_))).count();
    println!(
        "\n{} texts: together, {together_size} bytes; each alone, {}; grouped, {grouped_size}, \
         {sharing} of them sharing {name} ({took:.0?} to group, {together_took:.0?} together)",
        texts.len(),
        alone.iter().sum::<usize>(),
    );
    Ok(())
}
