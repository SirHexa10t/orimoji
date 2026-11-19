//! Folding and unfolding files: over themselves, or beside them under a suffix.
//!
//! Every file is folded (or unfolded) before any is written, so a file that fails leaves all of
//! them as they were. Each is then written whole to a temporary file beside it and renamed into
//! place, keeping the permissions of a file it replaces. Files folded together share one index,
//! which comes back to the caller like any fold's. A fold waiting to be weighed against another
//! (see [`Fold::buffer_past`]) is buffered beside the files too, unless [`Fold::buffer_folder`]
//! says where.
//!
//! ```no_run
//! use orimoji::files::{self, Output};
//! use orimoji::{Alphabet, Fold, Squeeze};
//!
//! let how = Fold::new(Squeeze::Text, Alphabet::Base64Url).indexed();
//! let folded = files::fold_files(&["a.txt", "b.txt"], &how, &Output::default())?; // a.txt.folded, …
//! let index = folded.indexing.index(); // the caller keeps it where it likes
//! files::unfold_files(&folded.written, index, &Output::default())?; // a.txt, b.txt again
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::{Fold, FoldError, Index, Indexing, Judged, Squeeze, UnfoldError};
use std::fmt;
use std::path::{Path, PathBuf};

/// The suffix [`Output::default`] folds files under.
pub const SUFFIX: &str = ".folded";

/// Where folded or unfolded files are written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output {
    /// Over the files themselves.
    InPlace,
    /// Beside them: a fold adds the suffix to each file's name; an unfold takes it off again, and
    /// adds `.unfolded` to a name that doesn't end in it.
    Suffix(String),
}

impl Default for Output {
    /// Beside the files, under [`SUFFIX`].
    fn default() -> Self {
        Output::Suffix(SUFFIX.to_string())
    }
}

/// Files folded together: where each was written, in their order, the index they share, how it
/// was decided, and the squeeze they were folded with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoldedFiles {
    pub written: Vec<PathBuf>,
    pub indexing: Indexing,
    pub judged: Judged,
    /// The squeeze asked for, or the one [`Squeeze::Auto`] chose.
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
        }
    }
}

impl std::error::Error for FilesError {}

/// Folds the files at `paths` together, writing each where `output` says.
pub fn fold_files<P: AsRef<Path>>(
    paths: &[P],
    how: &Fold,
    output: &Output,
) -> Result<FoldedFiles, FilesError> {
    let inputs: Vec<Vec<u8>> =
        paths.iter().map(|path| read(path.as_ref())).collect::<Result<_, _>>()?;
    let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
    let written: Vec<PathBuf> =
        paths.iter().map(|path| folded_name(path.as_ref(), output)).collect();
    // On the disk the folds go to, which a system's temporary folder may not be.
    let folder = match (&how.buffer_folder, written.first()) {
        (Some(folder), _) => folder.clone(),
        (None, Some(first)) => folder_of(first).to_path_buf(),
        (None, None) => std::env::temp_dir(),
    };
    let folded = crate::fold_each(&refs, how, &folder).map_err(|(at, error)| FilesError::Fold {
        path: at.map_or_else(|| folder.clone(), |at| paths[at].as_ref().to_path_buf()),
        error,
    })?;
    // The inputs aren't needed past the fold: their memory goes before the writing.
    drop(refs);
    drop(inputs);
    for (target, text) in written.iter().zip(folded.texts) {
        write(target, text.as_bytes())?;
    }
    Ok(FoldedFiles {
        written,
        indexing: folded.indexing,
        judged: folded.judged,
        squeeze: folded.squeeze,
    })
}

/// The folder `path` is in: `.` for a bare name.
fn folder_of(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Unfolds the files at `paths`, given the index they were folded with, if they need one, writing
/// each where `output` says. Gives back where each was written.
pub fn unfold_files<P: AsRef<Path>>(
    paths: &[P],
    index: Option<&Index>,
    output: &Output,
) -> Result<Vec<PathBuf>, FilesError> {
    let mut unfolded = Vec::with_capacity(paths.len());
    for path in paths {
        let path = path.as_ref();
        let text = String::from_utf8(read(path)?)
            .map_err(|_| FilesError::NotText { path: path.to_path_buf() })?;
        let bytes = crate::unfold(&text, index)
            .map_err(|error| FilesError::Unfold { path: path.to_path_buf(), error })?;
        unfolded.push((unfolded_name(path, output), bytes));
    }
    for (target, bytes) in &unfolded {
        write(target, bytes)?;
    }
    Ok(unfolded.into_iter().map(|(target, _)| target).collect())
}

/// Where a fold of `path` goes.
fn folded_name(path: &Path, output: &Output) -> PathBuf {
    match output {
        Output::InPlace => path.to_path_buf(),
        Output::Suffix(suffix) => with_name(path, |name| format!("{name}{suffix}")),
    }
}

/// Where an unfold of `path` goes.
fn unfolded_name(path: &Path, output: &Output) -> PathBuf {
    match output {
        Output::InPlace => path.to_path_buf(),
        Output::Suffix(suffix) => {
            with_name(path, |name| match name.strip_suffix(suffix.as_str()) {
                Some(stem) if !stem.is_empty() && !suffix.is_empty() => stem.to_string(),
                _ => format!("{name}.unfolded"),
            })
        }
    }
}

fn with_name(path: &Path, rename: impl Fn(&str) -> String) -> PathBuf {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    path.with_file_name(rename(&name))
}

fn read(path: &Path) -> Result<Vec<u8>, FilesError> {
    std::fs::read(path).map_err(|error| FilesError::Read { path: path.to_path_buf(), error })
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
        let suffix = Output::default();
        assert_eq!(folded_name(Path::new("dir/a.txt"), &suffix), Path::new("dir/a.txt.folded"));
        assert_eq!(unfolded_name(Path::new("dir/a.txt.folded"), &suffix), Path::new("dir/a.txt"));
        assert_eq!(unfolded_name(Path::new("dir/a.txt"), &suffix), Path::new("dir/a.txt.unfolded"));
        assert_eq!(unfolded_name(Path::new(".folded"), &suffix), Path::new(".folded.unfolded"));
        assert_eq!(folded_name(Path::new("a"), &Output::InPlace), Path::new("a"));
    }
}
