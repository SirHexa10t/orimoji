//! The `orimoji` command: a thin shell over the crate.

use clap::{Args, Parser, Subcommand};
use orimoji::files::{self, Output};
use orimoji::measure::{self, Measurement};
use orimoji::{Alphabet, Fold, Index, Indexing, Judged, Squeeze, TableOptions, UnfoldError};
use std::error::Error;
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
    /// Fold standard input onto standard output, or files beside themselves (`FILE.ori` for the
    /// text squeeze, `FILE.folded` for any other) or, with --in-place, over themselves. Text files
    /// given together fold together where that takes less, sharing one unit in an `.orinv`; each
    /// that takes less alone folds alone, its unit inside its `.ori`.
    Fold(FoldArgs),
    /// Unfold standard input onto standard output, or folded files beside themselves (taking
    /// `.ori` or `.folded` off) or, with --in-place, over themselves.
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
    /// The suffix of the files written beside them: a fold adds it, an unfold takes it off. By
    /// default `.ori` for the text squeeze and `.folded` for any other.
    #[arg(long, conflicts_with = "in_place")]
    suffix: Option<String>,
}

impl Files {
    fn output(&self) -> Output {
        match (self.in_place, &self.suffix) {
            (true, _) => Output::InPlace,
            (false, Some(suffix)) => Output::Suffix(suffix.clone()),
            (false, None) => Output::Beside,
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
    /// Index: write common words and escapes (text), or a column's common values (table), as
    /// their places in a list, where a list pays for itself. The index is kept with the code:
    /// inside the folded file, or in the .orinv of files folded together.
    #[arg(long)]
    index: bool,
    /// With --index: measure, not estimate, whether the index pays, by folding without it too and
    /// keeping the smaller fold, as it is; but spend TIME at most on it (30ms, 5s, 15m, 1h, 2d),
    /// and past it let the estimates decide. A fold waiting to be weighed is buffered in a
    /// temporary file once it is past 8 MiB: beside the output, or in TMPDIR for standard output.
    /// (The text squeeze always weighs both.)
    #[arg(long, value_name = "TIME", requires = "index")]
    measure_up_to: Option<String>,
    /// Keep the tables of this earlier fold, so lines that didn't change fold the same. For the
    /// text squeeze, an .ori or an .orinv, whose unit the files are folded with: one naming an
    /// .orinv, or the .orinv itself, and the files join those that share it.
    #[arg(long, value_name = "FOLDED")]
    like: Option<PathBuf>,
    /// Where the .orinv of text files folded together goes (by the text squeeze, or by auto where
    /// it chooses it): by default beside the first of those that share it, named after its
    /// folder.
    #[arg(long, value_name = "DIR")]
    orinv_dir: Option<PathBuf>,
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
    /// The index a folded file was folded with, when it keeps it beside it (format 1).
    #[arg(long, value_name = "FILE")]
    index: Option<PathBuf>,
    /// The .orinv holding the unit of .ori files folded together, when it isn't where they name
    /// it.
    #[arg(long, value_name = "FILE")]
    orinv: Option<PathBuf>,
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

fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
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

fn fold(args: FoldArgs) -> Result<(), Box<dyn Error>> {
    let one_stream = args.files.one_stream()?;
    let alphabet = Alphabet::from_name(&args.alphabet)
        .ok_or_else(|| format!("`{}` is no alphabet", args.alphabet))?;
    let previous = args.like.as_ref().map(read_text).transpose()?;
    let name = match (args.squeeze.as_str(), &previous) {
        // Keeping tables means keeping the squeeze they belong to.
        ("auto", Some(previous)) if orimoji::ori::is_ori(previous) => "text".to_string(),
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
        "auto" => Squeeze::Auto(table),
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
    let shares = matches!(squeeze, Squeeze::Text | Squeeze::Auto(_));
    if args.orinv_dir.is_some() && (!shares || one_stream) {
        return Err("--orinv-dir places the .orinv of text files folded together, by the text \
                    squeeze or auto"
            .into());
    }
    let measure_up_to = args.measure_up_to.as_deref().map(orimoji::time_limit).transpose()?;
    // A stream folded into a file is buffered beside it, on its disk; files, beside themselves.
    let output = args.files.output.as_ref().filter(|path| path.as_os_str() != "-");
    let how = Fold {
        wrap: args.wrap,
        readable: args.readable,
        index: args.index,
        measure_up_to,
        buffer_folder: output.map(|path| folder_of(path).to_path_buf()),
        orinv_dir: args.orinv_dir.clone(),
        ..Fold::new(squeeze, alphabet)
    };
    let said = |input: usize, folded: usize, squeeze: &Squeeze, written: &str| {
        if !args.quiet {
            eprintln!(
                "orimoji: {input} bytes folded into {folded} ({:.0}%), with {} into {alphabet}{}{written}",
                100.0 * folded as f64 / input.max(1) as f64,
                squeeze.describe(),
                if squeeze.by_line() { ", a line for each line" } else { "" },
            );
        }
    };
    // What became of the index, how it was decided, and, of several files, how many hold their own
    // unit or index rather than share one, and how many there are.
    let (indexing, judged, own, count) = match one_stream {
        true => {
            let path = args.files.files.first().cloned().unwrap_or_else(|| PathBuf::from("-"));
            let input = read(args.files.files.first())?;
            let into = output.map_or_else(|| PathBuf::from("."), |path| folder_of(path).into());
            let like = args.like.as_deref();
            let folded = files::fold_stream(&input, &path, &how, like, &into).map_err(hint)?;
            write(args.files.output.as_ref(), folded.text.as_bytes())?;
            said(input.len(), folded.text.len(), &folded.squeeze, "");
            (folded.indexing, folded.judged, 0, 1)
        }
        false => {
            // The files are read by the fold itself; here, only their sizes, before any is
            // written over.
            let size_of = |path: &PathBuf| std::fs::metadata(path).map_or(0, |m| m.len() as usize);
            let input: usize = args.files.files.iter().map(size_of).sum();
            let (paths, place) = (&args.files.files, args.files.output());
            let folded = match &args.like {
                Some(like) => files::fold_files_like(paths, like, &how, &place),
                None => files::fold_files(paths, &how, &place),
            }
            .map_err(hint)?;
            let unit = folded.orinv.as_ref().map_or(0, size_of);
            let size = folded.written.iter().map(size_of).sum::<usize>() + unit;
            let count = folded.written.len();
            let shared = match (&folded.orinv, folded.sharing.len()) {
                (Some(orinv), sharing) if sharing == count => {
                    format!(", their unit in {}", orinv.display())
                }
                (Some(orinv), sharing) => format!(
                    ", {sharing} of them sharing their unit in {}, the rest each with its own: \
                     they take less alone",
                    orinv.display()
                ),
                (None, _) if count > 1 && folded.squeeze == Squeeze::Text => {
                    ", each with its own unit: folded together, they would take more".to_string()
                }
                (None, _) => String::new(),
            };
            let written =
                format!(", {count} file{} written{shared}", if count == 1 { "" } else { "s" });
            said(input, size, &folded.squeeze, &written);
            let own = if count > 1 { count - folded.sharing.len() } else { 0 };
            (folded.indexing, folded.judged, own, count)
        }
    };
    if !args.quiet {
        tell_judged(judged, args.measure_up_to.as_deref());
        // Files each with its own unit keep each its own index; files sharing one share its.
        let rest = match own {
            0 => "",
            _ => "; those folded alone keep their own, where a list paid for itself",
        };
        match (indexing, own) {
            (Indexing::Off, _) => {}
            (_, own) if own > 0 && own == count => eprintln!(
                "orimoji: each file keeps its own index with its code, where a list paid for itself"
            ),
            (Indexing::Index(index), _) => {
                let values = match index.len() {
                    1 => "1 value".to_string(),
                    count => format!("{count} values"),
                };
                eprintln!("orimoji: the index, {values}, is kept with the code{rest}");
            }
            (Indexing::Unneeded, _) => {
                eprintln!("orimoji: no list paid for itself, so there is no index{rest}")
            }
        }
    }
    Ok(())
}

/// What a fold or an unfold refused, with what to give it, where it needs more.
fn hint(error: files::FilesError) -> Box<dyn Error> {
    match error {
        files::FilesError::Unfold { path, error: UnfoldError::NeedsIndex } => {
            format!("{}: {}: give it with --index FILE", path.display(), UnfoldError::NeedsIndex)
                .into()
        }
        files::FilesError::Unfold { path, error: error @ UnfoldError::NeedsOrinv(_) } => {
            format!("{}: {error}: give it with --orinv FILE", path.display()).into()
        }
        files::FilesError::Unfold { path, error: UnfoldError::WrongOrinv } => format!(
            "{}: {}: give the one it was folded with, with --orinv FILE",
            path.display(),
            UnfoldError::WrongOrinv
        )
        .into(),
        files::FilesError::Fold {
            path,
            error: orimoji::FoldError::Previous(UnfoldError::NeedsIndex),
        } => format!(
            "{}: the earlier fold keeps its index beside it (format 1): unfold it with --index \
             FILE, and fold it afresh",
            path.display()
        )
        .into(),
        error => error.into(),
    }
}

/// Says how a measured fold decided its index.
fn tell_judged(judged: Judged, limit: Option<&str>) {
    match judged {
        Judged::Estimated | Judged::Weighed => {}
        Judged::Measured { indexed, unindexed } => eprintln!(
            "orimoji: measured {indexed} bytes indexed, index included, against {unindexed} without: {}",
            if unindexed <= indexed {
                "the index doesn't pay, so there is none"
            } else {
                "the index pays"
            }
        ),
        Judged::OutOfTime => eprintln!(
            "orimoji: measuring took longer than {}, so the estimates decided",
            limit.unwrap_or("its time")
        ),
    }
}

fn unfold(args: UnfoldArgs) -> Result<(), Box<dyn Error>> {
    let one_stream = args.files.one_stream()?;
    let index = args.index.as_ref().map(read_index).transpose()?;
    let orinv = args.orinv.as_deref();
    if one_stream {
        let path = args.files.files.first().cloned().unwrap_or_else(|| PathBuf::from("-"));
        let text = read_text(&path)?;
        let from = match args.files.files.first() {
            Some(path) if path.as_os_str() != "-" => folder_of(path).to_path_buf(),
            _ => PathBuf::from("."),
        };
        let unfolded =
            files::unfold_stream(&text, &path, index.as_ref(), orinv, &from).map_err(hint)?;
        return Ok(write(args.files.output.as_ref(), &unfolded)?);
    }
    files::unfold_files(&args.files.files, index.as_ref(), orinv, &args.files.output())
        .map_err(hint)?;
    Ok(())
}

/// The folder `path` is in: `.` for a bare name.
fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

fn read_text(path: impl AsRef<Path>) -> Result<String, Box<dyn Error>> {
    let path = path.as_ref();
    let bytes = read(Some(&path.to_path_buf()))?;
    String::from_utf8(bytes)
        .map_err(|_| format!("{}: a folded file is UTF-8, and this isn't", path.display()).into())
}

fn read_index(path: impl AsRef<Path>) -> Result<Index, Box<dyn Error>> {
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
