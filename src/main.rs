//! The `orimoji` command: a thin shell over the crate.

use clap::{Args, Parser, Subcommand};
use orimoji::measure::{self, Measurement};
use orimoji::{Alphabet, Fold, Squeeze, TableOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

/// Fold text and data into fewer characters, fewer bytes or narrower lines, and unfold it back
/// exactly.
#[derive(Parser)]
#[command(name = "orimoji", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Fold a file, or standard input, onto standard output (or --output).
    Fold(FoldArgs),
    /// Unfold a folded file, or standard input, back into what was folded.
    Unfold {
        /// The folded file; standard input when left out.
        input: Option<PathBuf>,
        /// Where to write what it holds; standard output when left out.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Fold a file every way and compare: characters, bytes, screen width, and what git stores.
    Measure {
        /// The file; standard input when left out.
        input: Option<PathBuf>,
        /// The alphabets to try, by name, or `all`.
        #[arg(short, long, value_delimiter = ',', default_value = "base64url,two-byte,cjk")]
        alphabets: Vec<String>,
    },
    /// List the alphabets, and what a character of each holds and costs.
    Alphabets,
}

#[derive(Args)]
struct FoldArgs {
    /// The file to fold; standard input when left out.
    input: Option<PathBuf>,
    /// Where to write the folded file; standard output when left out.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// How to make it smaller first: auto (the smallest line by line), none, pack, text, table,
    /// deflate or zstd.
    #[arg(short, long, default_value = "auto")]
    squeeze: String,
    /// The alphabet to fold into; see `orimoji alphabets`.
    #[arg(short, long, default_value = "base64url")]
    alphabet: String,
    /// Squeeze the whole file as one stream (none, deflate; zstd always does): smaller, but any
    /// edit changes the folded file from there on.
    #[arg(long)]
    whole: bool,
    /// For deflate a line at a time: how many lines above each line leans on.
    #[arg(long, default_value_t = 1)]
    lean: usize,
    /// For a table: what separates its cells (`tab`, or the character).
    #[arg(long, default_value = "tab")]
    separator: String,
    /// For a table: keep lines starting with this as they are (empty for none).
    #[arg(long, default_value = "#")]
    keep: String,
    /// Keep the tables of this earlier fold, so lines that didn't change fold the same.
    #[arg(long)]
    like: Option<PathBuf>,
    /// The screen cells a body line takes when the body is one stream.
    #[arg(long, default_value_t = 100)]
    wrap: usize,
    /// The zstd level, 1 to 22.
    #[arg(long, default_value_t = 19)]
    level: i32,
    /// Keep the tables readable text instead of deflating them.
    #[arg(long)]
    readable: bool,
    /// Say nothing on standard error about how it went.
    #[arg(short, long)]
    quiet: bool,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("orimoji: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Command::Fold(args) => fold(args),
        Command::Unfold { input, output } => {
            let folded = String::from_utf8(read(input.as_ref())?)
                .map_err(|_| "a folded file is UTF-8, and this isn't")?;
            Ok(write(output.as_ref(), &orimoji::unfold(&folded)?)?)
        }
        Command::Measure { input, alphabets } => {
            let alphabets = alphabet_list(&alphabets)?;
            print_measurements(&measure::measure(&read(input.as_ref())?, &alphabets));
            Ok(())
        }
        Command::Alphabets => {
            print_alphabets();
            Ok(())
        }
    }
}

fn fold(args: FoldArgs) -> Result<(), Box<dyn std::error::Error>> {
    let input = read(args.input.as_ref())?;
    let alphabet = Alphabet::from_name(&args.alphabet)
        .ok_or_else(|| format!("`{}` is no alphabet", args.alphabet))?;
    let previous = match &args.like {
        Some(path) => Some(
            String::from_utf8(std::fs::read(path)?)
                .map_err(|_| "the fold to keep tables from isn't UTF-8")?,
        ),
        None => None,
    };
    let name = match (args.squeeze.as_str(), &previous) {
        // Keeping tables means keeping the squeeze they belong to.
        ("auto", Some(previous)) => orimoji::folded_with(previous)?.0,
        (name, _) => name.to_string(),
    };
    let table = TableOptions {
        separator: match args.separator.as_str() {
            "tab" | "\\t" => '\t',
            other => {
                let mut chars = other.chars();
                chars
                    .next()
                    .filter(|_| chars.next().is_none())
                    .ok_or("a separator is one character, or `tab`")?
            }
        },
        keep: (!args.keep.is_empty()).then(|| args.keep.clone()),
    };
    let squeeze = match name.as_str() {
        "auto" => orimoji::pick(&input, alphabet),
        "none" => Squeeze::None { whole: args.whole },
        "pack" => Squeeze::Pack,
        "text" => Squeeze::Text,
        "table" => Squeeze::Table(table),
        "deflate" => Squeeze::Deflate { lean: (!args.whole).then_some(args.lean) },
        "zstd" => Squeeze::Zstd { level: args.level },
        other => {
            return Err(format!(
                "`{other}` is no squeeze: auto, none, pack, text, table, deflate, zstd"
            )
            .into());
        }
    };
    let how = Fold { squeeze, alphabet, wrap: args.wrap, readable: args.readable };
    let folded = match &previous {
        Some(previous) => orimoji::fold_keeping(&input, previous, &how)?,
        None => orimoji::fold(&input, &how)?,
    };
    write(args.output.as_ref(), folded.as_bytes())?;
    if !args.quiet {
        eprintln!(
            "orimoji: {} bytes folded into {} ({:.0}%), with {} into {}{}",
            input.len(),
            folded.len(),
            100.0 * folded.len() as f64 / input.len().max(1) as f64,
            how.squeeze.describe(),
            alphabet,
            if how.squeeze.by_line() { ", a line for each line" } else { "" },
        );
    }
    Ok(())
}

fn read(path: Option<&PathBuf>) -> std::io::Result<Vec<u8>> {
    match path {
        Some(path) if path.as_os_str() != "-" => std::fs::read(path),
        _ => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes)?;
            Ok(bytes)
        }
    }
}

fn write(path: Option<&PathBuf>, bytes: &[u8]) -> std::io::Result<()> {
    match path {
        Some(path) if path.as_os_str() != "-" => std::fs::write(path, bytes),
        _ => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(bytes)?;
            stdout.flush()
        }
    }
}

fn alphabet_list(names: &[String]) -> Result<Vec<Alphabet>, String> {
    if names.iter().any(|name| name == "all") {
        return Ok(Alphabet::ALL.to_vec());
    }
    names
        .iter()
        .map(|name| Alphabet::from_name(name).ok_or_else(|| format!("`{name}` is no alphabet")))
        .collect()
}

fn print_alphabets() {
    println!(
        "{:<10} {:>6} {:>5} {:>6} {:>6} {:>6}  characters",
        "alphabet", "count", "bits", "bytes", "wasted", "cells"
    );
    for alphabet in Alphabet::ALL {
        let wasted = 1.0 - alphabet.bits_per_char() / (8.0 * alphabet.bytes_per_char());
        println!(
            "{:<10} {:>6} {:>5.2} {:>6.2} {:>5.0}% {:>6}  {}",
            alphabet.name(),
            alphabet.size(),
            alphabet.bits_per_char(),
            alphabet.bytes_per_char(),
            wasted * 100.0,
            alphabet.cells_per_char(),
            alphabet.characters()
        );
    }
}

fn print_measurements(measurements: &[Measurement]) {
    let input = &measurements[0];
    let percent =
        |part: usize, whole: usize| format!("{:.0}%", 100.0 * part as f64 / whole.max(1) as f64);
    println!(
        "{:<36} {:<10} {:>4} {:>10} {:>6} {:>10} {:>6} {:>10} {:>6}",
        "squeeze", "alphabet", "diff", "bytes", "", "width", "", "in git", ""
    );
    for m in measurements {
        let squeeze =
            m.squeeze.as_ref().map_or("(the input itself)".to_string(), Squeeze::describe);
        let alphabet = m.alphabet.map_or("", Alphabet::name);
        let diffs = m.squeeze.as_ref().is_none_or(Squeeze::by_line);
        println!(
            "{:<36} {:<10} {:>4} {:>10} {:>6} {:>10} {:>6} {:>10} {:>6}",
            squeeze,
            alphabet,
            if diffs { "yes" } else { "no" },
            m.bytes,
            percent(m.bytes, input.bytes),
            m.cells,
            percent(m.cells, input.cells),
            m.in_git,
            percent(m.in_git, input.in_git),
        );
    }
}
