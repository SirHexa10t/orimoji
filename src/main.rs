//! The `orimoji` command: a thin shell over the crate.

use clap::{Args, Parser, Subcommand};
use orimoji::files::{self, Output};
use orimoji::measure::{self, Measurement};
use orimoji::{Alphabet, Fold, Index, Indexing, Squeeze, TableOptions, UnfoldError};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
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
    /// Fold standard input onto standard output, or files beside themselves (`FILE.folded`) or,
    /// with --in-place, over themselves.
    Fold(FoldArgs),
    /// Unfold standard input onto standard output, or folded files beside themselves (taking
    /// `.folded` off) or, with --in-place, over themselves.
    Unfold(UnfoldArgs),
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

/// What to read, and where to write.
#[derive(Args)]
struct Files {
    /// The files; standard input when there are none.
    files: Vec<PathBuf>,
    /// Write here instead: for standard input, or a single file.
    #[arg(short, long, conflicts_with = "in_place")]
    output: Option<PathBuf>,
    /// Write over the files themselves, not beside them.
    #[arg(long)]
    in_place: bool,
    /// The suffix of the files written beside them: a fold adds it, an unfold takes it off.
    #[arg(long, default_value = files::SUFFIX)]
    suffix: String,
}

impl Files {
    fn output(&self) -> Output {
        match self.in_place {
            true => Output::InPlace,
            false => Output::Suffix(self.suffix.clone()),
        }
    }

    /// Whether what comes in and goes out is one stream: standard input, or a single file to
    /// `--output`.
    fn one_stream(&self) -> Result<bool, String> {
        match (self.files.len(), &self.output) {
            (0, _) => Ok(true),
            (1, Some(_)) => Ok(true),
            (_, Some(_)) => {
                Err("--output takes one input; several files are written beside or over themselves"
                    .into())
            }
            (_, None) => Ok(false),
        }
    }
}

#[derive(Args)]
struct FoldArgs {
    #[command(flatten)]
    files: Files,
    /// How to make it smaller first: auto (the smallest line by line), none, pack, text, table,
    /// deflate or zstd.
    #[arg(short, long, default_value = "auto")]
    squeeze: String,
    /// The alphabet to fold into; see `orimoji alphabets`.
    #[arg(short, long, default_value = "base64url")]
    alphabet: String,
    /// Index, and keep the index in FILE: written only if a list paid for itself. Files folded
    /// together share it. With --like, the earlier fold's index, which is kept as it is.
    #[arg(long, value_name = "FILE")]
    index: Option<PathBuf>,
    /// Keep the tables of this earlier fold, so lines that didn't change fold the same.
    #[arg(long, value_name = "FOLDED")]
    like: Option<PathBuf>,
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

#[derive(Args)]
struct UnfoldArgs {
    #[command(flatten)]
    files: Files,
    /// The index the input was folded with, if it needs one.
    #[arg(long, value_name = "FILE")]
    index: Option<PathBuf>,
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
        Command::Unfold(args) => unfold(args),
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
    let one_stream = args.files.one_stream()?;
    let alphabet = Alphabet::from_name(&args.alphabet)
        .ok_or_else(|| format!("`{}` is no alphabet", args.alphabet))?;
    let previous = args.like.as_ref().map(read_text).transpose()?;
    if previous.is_some() && args.files.files.len() > 1 {
        return Err("--like keeps the tables of one fold, for one input".into());
    }
    let inputs: Vec<Vec<u8>> = match args.files.files.is_empty() {
        true => vec![read(None)?],
        false => args.files.files.iter().map(std::fs::read).collect::<Result<_, _>>()?,
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
    let asked = args.index.is_some() && previous.is_none();
    let squeeze = match name.as_str() {
        "auto" => orimoji::pick(&inputs.concat(), alphabet, asked),
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
    let how = Fold { squeeze, alphabet, wrap: args.wrap, readable: args.readable, index: asked };
    let said = |input: usize, folded: usize, written: &str| {
        if !args.quiet {
            eprintln!(
                "orimoji: {input} bytes folded into {folded} ({:.0}%), with {} into {alphabet}{}{written}",
                100.0 * folded as f64 / input.max(1) as f64,
                how.squeeze.describe(),
                if how.squeeze.by_line() { ", a line for each line" } else { "" },
            );
        }
    };
    if let Some(previous) = &previous {
        let index = match orimoji::needs_index(previous)? {
            true => Some(read_index(
                args.index.as_ref().ok_or("the earlier fold needs its index: --index FILE")?,
            )?),
            false => None,
        };
        let text = orimoji::fold_keeping(&inputs[0], previous, index.as_ref(), &how)?;
        let target = match one_stream {
            true => args.files.output.clone(),
            false => Some(written_beside(&args.files.files[0], &args.files.output())),
        };
        write(target.as_ref(), text.as_bytes())?;
        said(inputs[0].len(), text.len(), "");
        return Ok(());
    }
    let indexing = match one_stream {
        true => {
            let folded = orimoji::fold(&inputs[0], &how)?;
            write(args.files.output.as_ref(), folded.text.as_bytes())?;
            said(inputs[0].len(), folded.text.len(), "");
            folded.indexing
        }
        false => {
            let folded = files::fold_files(&args.files.files, &how, &args.files.output())?;
            let size: usize = folded
                .written
                .iter()
                .map(|path| std::fs::metadata(path).map_or(0, |m| m.len() as usize))
                .sum();
            let input: usize = inputs.iter().map(Vec::len).sum();
            let files = folded.written.len();
            said(
                input,
                size,
                &format!(", {files} file{} written", if files == 1 { "" } else { "s" }),
            );
            folded.indexing
        }
    };
    keep_index(&indexing, args.index.as_deref(), args.quiet)?;
    Ok(())
}

/// Writes the index a fold made where the caller asked, or says why there is none.
fn keep_index(indexing: &Indexing, path: Option<&Path>, quiet: bool) -> std::io::Result<()> {
    match (indexing, path) {
        (Indexing::Index(index), Some(path)) => {
            std::fs::write(path, index.to_text())?;
            if !quiet {
                eprintln!("orimoji: the index, {} values, is in {}", index.len(), path.display());
            }
        }
        (Indexing::Unneeded, Some(path)) if !quiet => eprintln!(
            "orimoji: no list paid for itself, so there is no index: {} wasn't written, and the folded text unfolds without one",
            path.display()
        ),
        _ => {}
    }
    Ok(())
}

fn unfold(args: UnfoldArgs) -> Result<(), Box<dyn std::error::Error>> {
    let one_stream = args.files.one_stream()?;
    let index = args.index.as_ref().map(read_index).transpose()?;
    let hint = |error: UnfoldError| -> Box<dyn std::error::Error> {
        match error {
            UnfoldError::NeedsIndex => format!("{error}: give it with --index FILE").into(),
            error => error.into(),
        }
    };
    if one_stream {
        let path = args.files.files.first();
        let text = read_text(path.cloned().unwrap_or_else(|| PathBuf::from("-")))?;
        let unfolded = orimoji::unfold(&text, index.as_ref()).map_err(hint)?;
        return Ok(write(args.files.output.as_ref(), &unfolded)?);
    }
    files::unfold_files(&args.files.files, index.as_ref(), &args.files.output()).map_err(
        |error| -> Box<dyn std::error::Error> {
            match error {
                files::FilesError::Unfold { path, error: UnfoldError::NeedsIndex } => format!(
                    "{}: {}: give it with --index FILE",
                    path.display(),
                    UnfoldError::NeedsIndex
                )
                .into(),
                error => error.into(),
            }
        },
    )?;
    Ok(())
}

/// Where a fold of `path` is written, as `output` says.
fn written_beside(path: &Path, output: &Output) -> PathBuf {
    match output {
        Output::InPlace => path.to_path_buf(),
        Output::Suffix(suffix) => {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            path.with_file_name(format!("{name}{suffix}"))
        }
    }
}

fn read_text(path: impl AsRef<Path>) -> Result<String, Box<dyn std::error::Error>> {
    let path = path.as_ref();
    let bytes = read(Some(&path.to_path_buf()))?;
    String::from_utf8(bytes)
        .map_err(|_| format!("{}: a folded file is UTF-8, and this isn't", path.display()).into())
}

fn read_index(path: impl AsRef<Path>) -> Result<Index, Box<dyn std::error::Error>> {
    let path = path.as_ref();
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Index::from_text(&text).map_err(|error| format!("{}: {error}", path.display()))?)
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
        let squeeze = match &m.squeeze {
            None => "(the input itself)".to_string(),
            Some(squeeze) if m.index => format!("{}, indexed", squeeze.describe()),
            Some(squeeze) => squeeze.describe(),
        };
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
