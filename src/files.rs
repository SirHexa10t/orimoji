//! Folding and unfolding files as the command line does: over themselves, or beside them under a
//! suffix. A program asks orimoji to write its files with these, and gets exactly what someone at
//! a terminal would; or it folds into values with [`crate::fold`] and [`crate::parts`], and keeps
//! them itself.
//!
//! Every file is folded (or unfolded) before any is written, so a file that fails leaves all of
//! them as they were. Each is then written whole to a temporary file beside it and renamed into
//! place, keeping the permissions of a file it replaces. What a folded file needs to unfold is
//! never written apart from it:
//!
//! - **The text squeeze** writes [`crate::ori`] files. One file folds alone into its `.ori`, its
//!   unit inside it. More than one fold together where that takes less (see
//!   [`parts::fold_grouped`]): those that share one unit, learnt over them, keep it in one
//!   `.orinv`, beside the first of them and named after its folder (`chapters/chapters.orinv`), or
//!   in [`Fold::orinv_dir`], and each holds its lines and names the `.orinv`; each of the rest is
//!   folded alone, as if given alone, its unit inside its `.ori`. An `.orinv` other files still
//!   need is never replaced by one they can't unfold with.
//! - **Every other squeeze** folds each file on its own, its index, when it keeps one, inside it.
//!
//! ```no_run
//! use orimoji::files::{self, Output};
//! use orimoji::{Alphabet, Fold, Squeeze};
//!
//! let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
//! let folded = files::fold_files(&["a.txt", "b.txt"], &how, &Output::default())?;
//! // a.txt.ori and b.txt.ori, and their unit in the folder's .orinv:
//! println!("{:?} share {:?}", folded.written, folded.orinv);
//! files::unfold_files(&folded.written, None, None, &Output::default())?; // a.txt, b.txt again
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::ori::{self, OriFile};
use crate::parts::{self, Member, Unit};
use crate::{Fold, FoldError, Index, Indexing, Judged, Squeeze, UnfoldError};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::io::{BufRead, Read};
use std::path::{Component, Path, PathBuf};

/// The suffix a folded file of any squeeze but text takes beside the file it was folded from.
pub const SUFFIX: &str = ".folded";

/// Where folded or unfolded files are written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Output {
    /// Beside them, under the suffix of what they are: a fold adds `.ori` for the text squeeze and
    /// `.folded` for any other; an unfold takes either off again, and adds `.unfolded` to a name
    /// that ends in neither.
    #[default]
    Beside,
    /// Over the files themselves.
    InPlace,
    /// Beside them, under this suffix: a fold adds it to each file's name; an unfold takes it off
    /// again, and adds `.unfolded` to a name that doesn't end in it.
    Suffix(String),
}

/// Files folded: where each was written, in their order; the `.orinv` those folded together
/// share, if any were, and which they are; what became of the index; how it was decided; and the
/// squeeze.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedFiles {
    pub written: Vec<PathBuf>,
    /// Where the unit of files folded together is, for the text squeeze; `None` when each was
    /// folded on its own, its unit or index inside it.
    pub orinv: Option<PathBuf>,
    /// Those of `written` that unfold with the `.orinv`, in their order; the rest hold their own.
    pub sharing: Vec<PathBuf>,
    /// For files folded together, their unit's; for files each on its own, the first one's.
    pub indexing: Indexing,
    pub judged: Judged,
    /// The squeeze asked for, or the one [`Squeeze::Auto`] chose, when it chose one for them all.
    pub squeeze: Squeeze,
}

/// Why files couldn't be folded or unfolded.
#[derive(Debug)]
#[non_exhaustive]
pub enum FilesError {
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
    Write {
        path: PathBuf,
        error: std::io::Error,
    },
    Fold {
        path: PathBuf,
        error: FoldError,
    },
    Unfold {
        path: PathBuf,
        error: UnfoldError,
    },
    /// A folded file is UTF-8 text, and this one isn't.
    NotText {
        path: PathBuf,
    },
    /// An `.orinv` is there holding another unit, which these files still unfold with: replaced,
    /// they couldn't.
    WouldOrphan {
        orinv: PathBuf,
        files: Vec<PathBuf>,
    },
    /// A file is where an `.orinv` is, or would go, and it is no `.orinv`.
    NotOrinv {
        path: PathBuf,
    },
}

impl fmt::Display for FilesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FilesError::Read { path, error } => write!(f, "{}: {error}", path.display()),
            FilesError::Write { path, error } => write!(f, "{}: {error}", path.display()),
            FilesError::Fold { path, error } => write!(f, "{}: {error}", path.display()),
            FilesError::Unfold { path, error } => write!(f, "{}: {error}", path.display()),
            FilesError::NotText { path } => {
                write!(f, "{}: a folded file is UTF-8, and this isn't", path.display())
            }
            FilesError::WouldOrphan { orinv, files } => {
                let files: Vec<String> =
                    files.iter().map(|file| file.display().to_string()).collect();
                write!(
                    f,
                    "{} holds the unit {} unfold with, so it isn't replaced: fold them too, keep \
                     the unit (--like), or put this unit elsewhere (--orinv-dir)",
                    orinv.display(),
                    files.join(", ")
                )
            }
            FilesError::NotOrinv { path } => {
                write!(f, "{}: this file holds no unit, so it is no .orinv", path.display())
            }
        }
    }
}

impl std::error::Error for FilesError {}

/// Folds the files at `paths`, writing each where `output` says, as the command line does: for
/// the text squeeze, into `.ori` files, together when there is more than one; for any other,
/// each on its own, its index inside it.
pub fn fold_files<P: AsRef<Path>>(
    paths: &[P],
    how: &Fold,
    output: &Output,
) -> Result<FoldedFiles, FilesError> {
    let paths: Vec<PathBuf> = paths.iter().map(|path| path.as_ref().to_path_buf()).collect();
    match how.squeeze {
        Squeeze::Text => fold_ori(&paths, how, output, None),
        _ => fold_each_alone(&paths, how, output, None),
    }
}

/// Folds the files at `paths` keeping the tables of the earlier fold at `previous`, so that the
/// lines they share with what it folded fold the same. For the text squeeze, `previous` is an
/// `.ori` or an `.orinv`, and the files are folded with its unit, in its alphabet: with an
/// `.orinv`'s, or with that of an `.ori` naming one, they name it too, and join the files that
/// share it; with the unit of a whole `.ori`, one file holds it inside, and more than one get an
/// `.orinv` of their own holding it. For any other squeeze, `previous` is a folded file holding
/// its index inside it, if it has one.
pub fn fold_files_like<P: AsRef<Path>>(
    paths: &[P],
    previous: &Path,
    how: &Fold,
    output: &Output,
) -> Result<FoldedFiles, FilesError> {
    let paths: Vec<PathBuf> = paths.iter().map(|path| path.as_ref().to_path_buf()).collect();
    let text = read_text(previous)?;
    match ori::is_ori(&text) {
        true => fold_ori(&paths, how, output, Some(kept(previous, &text)?)),
        false => fold_each_alone(&paths, how, output, Some(&text)),
    }
}

/// A unit to keep, its alphabet, and the `.orinv` it is in, when it is in one.
struct Kept {
    unit: Unit,
    alphabet: crate::Alphabet,
    orinv: Option<PathBuf>,
}

/// The unit of the `.ori` or `.orinv` at `path`, whose text is `text`.
fn kept(path: &Path, text: &str) -> Result<Kept, FilesError> {
    let refused =
        |error| FilesError::Fold { path: path.to_path_buf(), error: FoldError::Previous(error) };
    match ori::read(text).map_err(refused)? {
        OriFile::Unit { unit, alphabet } => {
            Ok(Kept { unit, alphabet, orinv: Some(path.to_path_buf()) })
        }
        OriFile::Whole { unit, alphabet, .. } => Ok(Kept { unit, alphabet, orinv: None }),
        OriFile::Apart { orinv, .. } => {
            let orinv = folder_of(path).join(orinv);
            let (unit, alphabet) = read_unit(&orinv)?;
            Ok(Kept { unit, alphabet, orinv: Some(orinv) })
        }
    }
}

/// The unit in the `.orinv` at `path`, and its alphabet.
fn read_unit(path: &Path) -> Result<(Unit, crate::Alphabet), FilesError> {
    let text = read_text(path)?;
    match ori::read(&text) {
        Ok(OriFile::Unit { unit, alphabet }) => Ok((unit, alphabet)),
        Ok(_) => Err(FilesError::NotOrinv { path: path.to_path_buf() }),
        Err(error) => Err(FilesError::Unfold { path: path.to_path_buf(), error }),
    }
}

/// The text squeeze's files: folded alone, or together into one unit, or with a kept one.
fn fold_ori(
    paths: &[PathBuf],
    how: &Fold,
    output: &Output,
    kept: Option<Kept>,
) -> Result<FoldedFiles, FilesError> {
    let texts: Vec<String> = paths
        .iter()
        .map(|path| {
            String::from_utf8(read(path)?).map_err(|error| FilesError::Fold {
                path: path.clone(),
                error: FoldError::NotText { at: error.utf8_error().valid_up_to() },
            })
        })
        .collect::<Result<_, _>>()?;
    let written: Vec<PathBuf> =
        paths.iter().map(|path| folded_name(path, output, ori::SUFFIX)).collect();
    let alphabet = kept.as_ref().map_or(how.alphabet, |kept| kept.alphabet);
    // A unit kept weighs nothing; a fold afresh weighs its index, folding both ways.
    let judged = match kept.is_none() && how.index {
        true => Judged::Weighed,
        false => Judged::Estimated,
    };
    // Each file, alone or sharing; the unit they share, if any do; and the .orinv kept, if the
    // unit is one kept in an .orinv, which is then left as it is.
    let (members, shared, kept_orinv) = match kept {
        None if texts.len() == 1 => {
            let folded = parts::fold(&texts[0], alphabet, how.index);
            (vec![Member::Alone(folded)], None, None)
        }
        None => {
            // Weighed as if the first of them shares, and so names the .orinv.
            let overheads = ori::overheads(alphabet, &orinv_name(&written[0]));
            let grouped = parts::fold_grouped(&texts, alphabet, how.index, overheads);
            (grouped.texts, grouped.shared, None)
        }
        Some(Kept { unit, orinv, .. }) => {
            let mut lines = Vec::with_capacity(texts.len());
            for (path, text) in paths.iter().zip(&texts) {
                let kept = parts::fold_keeping(text, &unit, alphabet);
                lines.push(kept.map_err(|error| FilesError::Fold { path: path.clone(), error })?);
            }
            match (orinv, texts.len()) {
                (None, 1) => {
                    let parts = parts::Parts { lines: lines.remove(0), unit };
                    (vec![Member::Alone(parts)], None, None)
                }
                (orinv, _) => (lines.into_iter().map(Member::Sharing).collect(), Some(unit), orinv),
            }
        }
    };
    // The .orinv: the one kept, or beside the first file that shares it.
    let first_sharing = written
        .iter()
        .zip(&members)
        .find(|(_, member)| matches!(member, Member::Sharing(_)))
        .map(|(target, _)| target);
    let orinv = match (&kept_orinv, first_sharing) {
        (Some(kept), _) => Some(kept.clone()),
        (None, Some(first)) => Some(orinv_path(first, how.orinv_dir.as_deref())?),
        (None, None) => None,
    };
    let mut sharing = Vec::new();
    let mut files = Vec::with_capacity(members.len());
    for (target, member) in written.iter().zip(&members) {
        files.push(match (member, &shared, &orinv) {
            (Member::Sharing(lines), Some(unit), Some(orinv)) => {
                sharing.push(target.clone());
                ori::apart(lines, &relative(folder_of(target), orinv), unit, alphabet)
            }
            (Member::Alone(parts), _, _) => ori::whole(&parts.lines, &parts.unit, alphabet),
            _ => unreachable!("lines that share a unit have it, and an .orinv to keep it in"),
        });
    }
    let files: Vec<(PathBuf, String)> = written.iter().cloned().zip(files).collect();
    let new_unit = match (&orinv, &shared, &kept_orinv) {
        (Some(orinv), Some(unit), None) => Some((orinv.as_path(), unit)),
        _ => None,
    };
    write_files(&files, new_unit, alphabet)?;
    // The shared unit's index, or, when none is shared, the first file's.
    let unit = shared.as_ref().or(match members.first() {
        Some(Member::Alone(parts)) => Some(&parts.unit),
        _ => None,
    });
    let indexing = unit.map_or(Indexing::Off, |unit| unit_indexing(how.index, unit));
    Ok(FoldedFiles { written, orinv, sharing, indexing, judged, squeeze: Squeeze::Text })
}

/// Whether the `.orinv` at `path` holds `unit_file` already, and so needn't be written. Another
/// unit there is replaced only if no file but those being written now unfolds with it.
fn already_there(path: &Path, unit_file: &str, writing: &[PathBuf]) -> Result<bool, FilesError> {
    let existing = match std::fs::read_to_string(path) {
        Ok(existing) => existing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(FilesError::Read { path: path.to_path_buf(), error }),
    };
    if existing == unit_file {
        return Ok(true);
    }
    let Ok(OriFile::Unit { unit, .. }) = ori::read(&existing) else {
        return Err(FilesError::NotOrinv { path: path.to_path_buf() });
    };
    let files = orphans(path, unit.fingerprint(), writing);
    match files.is_empty() {
        true => Ok(false),
        false => Err(FilesError::WouldOrphan { orinv: path.to_path_buf(), files }),
    }
}

/// The files that unfold with the unit of `fingerprint` in the `.orinv` at `orinv` and aren't
/// among `writing`: those in its folder and in the folders of `writing`. (Files sharing it from
/// further away aren't looked for.)
fn orphans(orinv: &Path, fingerprint: u32, writing: &[PathBuf]) -> Vec<PathBuf> {
    let Some(target) = canonical(orinv) else { return Vec::new() };
    let writing: BTreeSet<PathBuf> = writing.iter().filter_map(|path| canonical(path)).collect();
    let folders: BTreeSet<PathBuf> = std::iter::once(folder_of(orinv).to_path_buf())
        .chain(writing.iter().map(|path| folder_of(path).to_path_buf()))
        .filter_map(|folder| canonical(&folder))
        .collect();
    let mut found = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for path in entries.flatten().map(|entry| entry.path()) {
            let Some((named, print)) = first_line(&path).and_then(|line| ori::names(&line)) else {
                continue;
            };
            let same = canonical(&folder.join(named)).is_some_and(|named| named == target);
            let written_now = canonical(&path).is_some_and(|path| writing.contains(&path));
            if same && print == fingerprint && !written_now {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The first line of the file at `path`, if it is a file and its first line is a short one.
fn first_line(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok().filter(|_| path.is_file())?;
    let mut line = String::new();
    std::io::BufReader::new(file.take(4096)).read_line(&mut line).ok()?;
    Some(line.trim_end_matches(['\n', '\r']).to_string())
}

/// The name of the `.orinv` of files folded together, the first that shares it written to
/// `first`: that one's folder's name, or `folded` for a folder that has none, as the root has none.
fn orinv_name(first: &Path) -> String {
    let name = std::fs::canonicalize(folder_of(first))
        .ok()
        .and_then(|folder| folder.file_name().map(|name| name.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "folded".to_string());
    format!("{name}{}", ori::UNIT_SUFFIX)
}

/// Where the `.orinv` of files folded together goes, the first that shares it written to `first`:
/// in `dir`, or beside that one, named after its folder.
fn orinv_path(first: &Path, dir: Option<&Path>) -> Result<PathBuf, FilesError> {
    let dir = match dir {
        Some(dir) => {
            std::fs::create_dir_all(dir)
                .map_err(|error| FilesError::Write { path: dir.to_path_buf(), error })?;
            dir.to_path_buf()
        }
        None => folder_of(first).to_path_buf(),
    };
    Ok(dir.join(orinv_name(first)))
}

/// `to`, as a path from the folder `from`, `/` between its parts.
fn relative(from: &Path, to: &Path) -> String {
    let name = to.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let (Ok(from), Ok(folder)) =
        (std::fs::canonicalize(from), std::fs::canonicalize(folder_of(to)))
    else {
        return to.to_string_lossy().replace('\\', "/");
    };
    let parts = |path: &Path| -> Vec<String> {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
    };
    let (from, folder) = (parts(&from), parts(&folder));
    let common = from.iter().zip(&folder).take_while(|(a, b)| a == b).count();
    let mut path: Vec<String> = vec!["..".to_string(); from.len() - common];
    path.extend(folder[common..].iter().cloned());
    path.push(name);
    path.join("/")
}

/// `path` with every link and `..` resolved: itself if it is there, or the folder it would be in
/// and its name.
fn canonical(path: &Path) -> Option<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Some(path),
        Err(_) => Some(std::fs::canonicalize(folder_of(path)).ok()?.join(path.file_name()?)),
    }
}

/// Files of any squeeze but text, or of [`Squeeze::Auto`], each folded on its own, its index
/// inside it; or, with `previous`, each with its tables. Under auto, files given together that
/// take less sharing one unit, as the text squeeze's, fold together into `.ori` files and an
/// `.orinv`, each of the rest as auto folded it alone.
fn fold_each_alone(
    paths: &[PathBuf],
    how: &Fold,
    output: &Output,
    previous: Option<&str>,
) -> Result<FoldedFiles, FilesError> {
    // Under auto, files given together may fold together: their texts are kept for it, and only
    // theirs, since only text folds so.
    let may_group = matches!((&how.squeeze, previous), (Squeeze::Auto(_), None)) && paths.len() > 1;
    let mut inputs: Vec<Option<String>> = Vec::with_capacity(paths.len());
    let mut texts = Vec::with_capacity(paths.len());
    let mut first: Option<(Indexing, Judged)> = None;
    for path in paths {
        let input = read(path)?;
        // Buffered on the disk the folds go to, which a system's temporary folder may not be.
        let beside = folder_of(&folded_name(path, output, SUFFIX)).to_path_buf();
        let buffer_folder = how.buffer_folder.clone().or(Some(beside));
        let how = Fold { index_inside: true, buffer_folder, ..how.clone() };
        let failed = |error| FilesError::Fold { path: path.clone(), error };
        let (text, indexing, judged, squeeze) = match previous {
            Some(previous) => {
                let text = crate::fold_keeping(&input, previous, None, &how).map_err(failed)?;
                (text, Indexing::Off, Judged::Estimated, how.squeeze.clone())
            }
            None => {
                let folded = crate::fold(&input, &how).map_err(failed)?;
                (folded.text, folded.indexing, folded.judged, folded.squeeze)
            }
        };
        first.get_or_insert((indexing, judged));
        texts.push((text, squeeze));
        inputs.push(may_group.then(|| String::from_utf8(input).ok()).flatten());
    }
    let group = match may_group {
        true => grouped(paths, &inputs, &texts, how, output)?,
        false => None,
    };
    let (mut files, mut sharing, mut squeezes) = (Vec::new(), Vec::new(), BTreeSet::new());
    for (at, (path, (text, squeeze))) in paths.iter().zip(&texts).enumerate() {
        match &group {
            Some(group) if group.sharing.contains(&at) => {
                let target = folded_name(path, output, ori::SUFFIX);
                let place = group.sharing.iter().position(|&shared| shared == at);
                let lines = &group.together.lines[place.expect("a sharing file")];
                let orinv = relative(folder_of(&target), &group.orinv);
                let text = ori::apart(lines, &orinv, &group.together.unit, how.alphabet);
                sharing.push(target.clone());
                squeezes.insert(Squeeze::Text.name());
                files.push((target, text));
            }
            _ => {
                let suffix = if ori::is_ori(text) { ori::SUFFIX } else { SUFFIX };
                squeezes.insert(squeeze.name());
                files.push((folded_name(path, output, suffix), text.clone()));
            }
        }
    }
    let unit = group.as_ref().map(|group| (group.orinv.as_path(), &group.together.unit));
    write_files(&files, unit, how.alphabet)?;
    let squeeze = match (squeezes.len(), texts.first()) {
        (1, Some((_, squeeze))) if group.is_none() => squeeze.clone(),
        (1, Some(_)) => Squeeze::Text,
        _ => how.squeeze.clone(),
    };
    let (indexing, judged) = match &group {
        Some(group) => (unit_indexing(how.index, &group.together.unit), Judged::Weighed),
        None => first.unwrap_or((Indexing::Off, Judged::Estimated)),
    };
    let written = files.into_iter().map(|(target, _)| target).collect();
    let orinv = group.map(|group| group.orinv);
    Ok(FoldedFiles { written, orinv, sharing, indexing, judged, squeeze })
}

/// Files folded together under auto: the ones that share, by their place among all, their fold
/// together, and where their `.orinv` goes.
struct Group {
    sharing: Vec<usize>,
    together: parts::Together,
    orinv: PathBuf,
}

/// Which of the files at `paths`, folded alone by auto into `texts`, take less sharing one unit, as
/// the text squeeze's: those of them that are text, their `inputs`, weighed against what auto made
/// of each alone.
fn grouped(
    paths: &[PathBuf],
    inputs: &[Option<String>],
    texts: &[(String, Squeeze)],
    how: &Fold,
    output: &Output,
) -> Result<Option<Group>, FilesError> {
    let candidates: Vec<(usize, &str)> = inputs
        .iter()
        .enumerate()
        .filter_map(|(at, input)| input.as_deref().map(|text| (at, text)))
        .collect();
    if candidates.len() < 2 {
        return Ok(None);
    }
    let first = folded_name(&paths[candidates[0].0], output, ori::SUFFIX);
    let overheads = ori::overheads(how.alphabet, &orinv_name(&first));
    let alone: Vec<usize> = candidates.iter().map(|&(at, _)| texts[at].0.len()).collect();
    let refs: Vec<&str> = candidates.iter().map(|&(_, text)| text).collect();
    let Some((sharing, together)) = parts::group(&refs, how.alphabet, how.index, overheads, &alone)
    else {
        return Ok(None);
    };
    let sharing: Vec<usize> = sharing.into_iter().map(|at| candidates[at].0).collect();
    let first = folded_name(&paths[sharing[0]], output, ori::SUFFIX);
    let orinv = orinv_path(&first, how.orinv_dir.as_deref())?;
    Ok(Some(Group { sharing, together, orinv }))
}

/// What became of the index of files sharing `unit`.
fn unit_indexing(index: bool, unit: &Unit) -> Indexing {
    match (index, unit.index.clone()) {
        (_, Some(index)) => Indexing::Index(index),
        (true, None) => Indexing::Unneeded,
        (false, None) => Indexing::Off,
    }
}

/// Writes each of `files` where it goes, and first, when `unit` gives one, the `.orinv` holding
/// the unit they share, unless it holds it already; another unit there is replaced only if no
/// file but these unfolds with it.
fn write_files(
    files: &[(PathBuf, String)],
    unit: Option<(&Path, &Unit)>,
    alphabet: crate::Alphabet,
) -> Result<(), FilesError> {
    if let Some((orinv, unit)) = unit {
        let unit_file = ori::unit_file(unit, alphabet);
        let writing: Vec<PathBuf> = files.iter().map(|(target, _)| target.clone()).collect();
        if !already_there(orinv, &unit_file, &writing)? {
            write(orinv, unit_file.as_bytes())?;
        }
    }
    for (target, text) in files {
        write(target, text.as_bytes())?;
    }
    Ok(())
}

/// One stream folded, as the command line folds standard input, or a single file to `--output`:
/// its text, and what became of its index, how it was decided, and its squeeze.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedStream {
    pub text: String,
    pub indexing: Indexing,
    pub judged: Judged,
    pub squeeze: Squeeze,
}

/// `input`, read from `path` (`-` for standard input), folded as the command line folds one
/// stream, to be written in the folder `into`: for the text squeeze, a whole `.ori`; keeping the
/// unit of an `.orinv`, or of an `.ori` naming one (`like`), its lines apart, naming that `.orinv`
/// as a path from `into`. For any other squeeze, a folded file holding its index inside it, with
/// the tables of the folded file at `like` when given.
pub fn fold_stream(
    input: &[u8],
    path: &Path,
    how: &Fold,
    like: Option<&Path>,
    into: &Path,
) -> Result<FoldedStream, FilesError> {
    let failed = |error| FilesError::Fold { path: path.to_path_buf(), error };
    let how = Fold { index_inside: true, ..how.clone() };
    let previous = like.map(|like| Ok::<_, FilesError>((like, read_text(like)?))).transpose()?;
    let text_squeeze = match &previous {
        Some((_, text)) => ori::is_ori(text),
        None => how.squeeze == Squeeze::Text,
    };
    if !text_squeeze {
        return match &previous {
            Some((_, previous)) => {
                let text = crate::fold_keeping(input, previous, None, &how).map_err(failed)?;
                let (indexing, judged) = (Indexing::Off, Judged::Estimated);
                Ok(FoldedStream { text, indexing, judged, squeeze: how.squeeze.clone() })
            }
            None => {
                let folded = crate::fold(input, &how).map_err(failed)?;
                let crate::Folded { text, indexing, judged, squeeze } = folded;
                Ok(FoldedStream { text, indexing, judged, squeeze })
            }
        };
    }
    let input = std::str::from_utf8(input)
        .map_err(|error| failed(FoldError::NotText { at: error.valid_up_to() }))?;
    let (text, unit) = match previous {
        None => {
            let folded = parts::fold(input, how.alphabet, how.index);
            (ori::whole(&folded.lines, &folded.unit, how.alphabet), folded.unit)
        }
        Some((like, text)) => {
            let Kept { unit, alphabet, orinv } = kept(like, &text)?;
            let lines = parts::fold_keeping(input, &unit, alphabet).map_err(failed)?;
            let text = match orinv {
                Some(orinv) => ori::apart(&lines, &relative(into, &orinv), &unit, alphabet),
                None => ori::whole(&lines, &unit, alphabet),
            };
            (text, unit)
        }
    };
    let judged = match like.is_none() && how.index {
        true => Judged::Weighed,
        false => Judged::Estimated,
    };
    let indexing = unit_indexing(how.index, &unit);
    Ok(FoldedStream { text, indexing, judged, squeeze: Squeeze::Text })
}

/// `text`, read from `path` (`-` for standard input), unfolded as the command line unfolds one
/// stream: a folded file of format 1 keeping its index beside it, with `index`; an `.ori` whose
/// lines are apart from their unit, with the `.orinv` at `orinv` when given, and otherwise the one
/// it names, as a path from the folder `from`.
pub fn unfold_stream(
    text: &str,
    path: &Path,
    index: Option<&Index>,
    orinv: Option<&Path>,
    from: &Path,
) -> Result<Vec<u8>, FilesError> {
    let refused = |error| FilesError::Unfold { path: path.to_path_buf(), error };
    if !ori::is_ori(text) {
        return crate::unfold(text, index).map_err(refused);
    }
    let unit = match ori::read(text).map_err(refused)? {
        OriFile::Apart { orinv: named, .. } => Some(match orinv {
            Some(given) => read_unit(given)?.0,
            None => named_unit(path, &from.join(named))?,
        }),
        _ => None,
    };
    Ok(ori::unfold(text, unit.as_ref(), usize::MAX).map_err(refused)?.into_bytes())
}

/// The folder `path` is in: `.` for a bare name.
fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Unfolds the files at `paths`, writing each where `output` says, and gives back where each was
/// written. A folded file of format 1 that keeps its index beside it unfolds with `index`; an
/// `.ori` whose lines are apart from their unit, with the `.orinv` at `orinv` when given, and the
/// one it names otherwise.
pub fn unfold_files<P: AsRef<Path>>(
    paths: &[P],
    index: Option<&Index>,
    orinv: Option<&Path>,
    output: &Output,
) -> Result<Vec<PathBuf>, FilesError> {
    let given = orinv.map(read_unit).transpose()?.map(|(unit, _)| unit);
    let mut units: HashMap<PathBuf, Unit> = HashMap::new();
    let mut unfolded = Vec::with_capacity(paths.len());
    for path in paths {
        let path = path.as_ref();
        let text = read_text(path)?;
        let refused = |error| FilesError::Unfold { path: path.to_path_buf(), error };
        let bytes = match ori::is_ori(&text) {
            false => crate::unfold(&text, index).map_err(refused)?,
            true => {
                let unit = match (ori::read(&text).map_err(refused)?, &given) {
                    (OriFile::Apart { .. }, Some(unit)) => Some(unit.clone()),
                    (OriFile::Apart { orinv, .. }, None) => {
                        let named = folder_of(path).join(&orinv);
                        let key = canonical(&named).unwrap_or_else(|| named.clone());
                        if !units.contains_key(&key) {
                            units.insert(key.clone(), named_unit(path, &named)?);
                        }
                        units.get(&key).cloned()
                    }
                    _ => None,
                };
                ori::unfold(&text, unit.as_ref(), usize::MAX).map_err(refused)?.into_bytes()
            }
        };
        unfolded.push((unfolded_name(path, output), bytes));
    }
    for (target, bytes) in &unfolded {
        write(target, bytes)?;
    }
    Ok(unfolded.into_iter().map(|(target, _)| target).collect())
}

/// The unit of the `.ori` at `path`, in the `.orinv` it names, at `named`: refused as missing
/// when nothing is there, so that it can be given instead.
fn named_unit(path: &Path, named: &Path) -> Result<Unit, FilesError> {
    match read_unit(named) {
        Err(FilesError::Read { error, .. }) if error.kind() == std::io::ErrorKind::NotFound => {
            let error = UnfoldError::NeedsOrinv(named.display().to_string());
            Err(FilesError::Unfold { path: path.to_path_buf(), error })
        }
        read => read.map(|(unit, _)| unit),
    }
}

/// Where a fold of `path` goes, `suffix` being its kind's.
fn folded_name(path: &Path, output: &Output, suffix: &str) -> PathBuf {
    match output {
        Output::InPlace => path.to_path_buf(),
        Output::Beside => with_name(path, |name| format!("{name}{suffix}")),
        Output::Suffix(suffix) => with_name(path, |name| format!("{name}{suffix}")),
    }
}

/// Where an unfold of `path` goes.
fn unfolded_name(path: &Path, output: &Output) -> PathBuf {
    let without = |name: &str, suffixes: &[&str]| {
        let stem = suffixes
            .iter()
            .filter(|suffix| !suffix.is_empty())
            .find_map(|suffix| name.strip_suffix(suffix).filter(|stem| !stem.is_empty()));
        stem.map_or_else(|| format!("{name}.unfolded"), str::to_string)
    };
    match output {
        Output::InPlace => path.to_path_buf(),
        Output::Beside => with_name(path, |name| without(name, &[ori::SUFFIX, SUFFIX])),
        Output::Suffix(suffix) => with_name(path, |name| without(name, &[suffix.as_str()])),
    }
}

fn with_name(path: &Path, rename: impl Fn(&str) -> String) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(rename(&name))
}

fn read(path: &Path) -> Result<Vec<u8>, FilesError> {
    std::fs::read(path).map_err(|error| FilesError::Read { path: path.to_path_buf(), error })
}

fn read_text(path: &Path) -> Result<String, FilesError> {
    String::from_utf8(read(path)?).map_err(|_| FilesError::NotText { path: path.to_path_buf() })
}

/// Writes `bytes` to a temporary file beside `path` and renames it into place, keeping the
/// permissions of a file it replaces.
fn write(path: &Path, bytes: &[u8]) -> Result<(), FilesError> {
    let failed = |error| FilesError::Write { path: path.to_path_buf(), error };
    let temporary = with_name(path, |name| format!(".{name}.orimoji-{}", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(failed)?;
    let kept = std::fs::metadata(path).map(|metadata| metadata.permissions());
    let placed = kept
        .map_or(Ok(()), |permissions| std::fs::set_permissions(&temporary, permissions))
        .and_then(|()| std::fs::rename(&temporary, path));
    if let Err(error) = placed {
        let _ = std::fs::remove_file(&temporary);
        return Err(failed(error));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suffix_is_added_by_a_fold_and_taken_off_by_an_unfold() {
        let beside = Output::default();
        let suffix = Output::Suffix(SUFFIX.to_string());
        let folded = |path, output| folded_name(Path::new(path), output, ori::SUFFIX);
        let unfolded = |path, output| unfolded_name(Path::new(path), output);
        assert_eq!(folded("dir/a.txt", &beside), Path::new("dir/a.txt.ori"));
        assert_eq!(folded("dir/a.txt", &suffix), Path::new("dir/a.txt.folded"));
        assert_eq!(unfolded("dir/a.txt.ori", &beside), Path::new("dir/a.txt"));
        assert_eq!(unfolded("dir/a.txt.folded", &beside), Path::new("dir/a.txt"));
        assert_eq!(unfolded("dir/a.txt.folded", &suffix), Path::new("dir/a.txt"));
        assert_eq!(unfolded("dir/a.txt", &suffix), Path::new("dir/a.txt.unfolded"));
        assert_eq!(unfolded(".ori", &beside), Path::new(".ori.unfolded"));
        assert_eq!(folded("a", &Output::InPlace), Path::new("a"));
    }

    #[test]
    fn a_path_from_one_folder_to_another_climbs_and_descends() {
        let root = std::env::temp_dir().join(format!("orimoji-relative-{}", std::process::id()));
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("c")).unwrap();
        assert_eq!(relative(&root.join("a/b"), &root.join("c/x.orinv")), "../../c/x.orinv");
        assert_eq!(relative(&root.join("c"), &root.join("c/x.orinv")), "x.orinv");
        assert_eq!(relative(&root.join("a"), &root.join("a/b/x.orinv")), "b/x.orinv");
        std::fs::remove_dir_all(root).unwrap();
    }
}
