//! Folds waiting to be weighed against another: held in memory while small, and buffered in a
//! temporary file once large, so that weighing two folds never holds both in memory.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Folded texts, in memory or in a temporary file.
pub(crate) struct Buffered {
    /// Each text's length in bytes, in order.
    lengths: Vec<usize>,
    place: Place,
}

enum Place {
    Memory(Vec<String>),
    File(Temporary),
}

impl Buffered {
    pub(crate) fn new(texts: Vec<String>) -> Self {
        Self { lengths: texts.iter().map(String::len).collect(), place: Place::Memory(texts) }
    }

    /// The bytes the texts take, all together.
    pub(crate) fn size(&self) -> usize {
        self.lengths.iter().sum()
    }

    /// Moves the texts out of memory into a temporary file in `folder`, if they take more than
    /// `past` bytes. If the file can't be made or written, they stay in memory: buffering only
    /// saves memory, and a fold never fails for want of it.
    pub(crate) fn buffer(&mut self, past: usize, folder: &Path) {
        let Place::Memory(texts) = &self.place else {
            return;
        };
        if self.size() <= past {
            return;
        }
        if let Ok(file) = Temporary::holding(texts, folder) {
            self.place = Place::File(file);
        }
    }

    /// The texts, back in memory.
    pub(crate) fn take(self) -> io::Result<Vec<String>> {
        match self.place {
            Place::Memory(texts) => Ok(texts),
            Place::File(file) => file.read(&self.lengths),
        }
    }

    #[cfg(test)]
    fn in_a_file(&self) -> bool {
        matches!(self.place, Place::File(_))
    }
}

/// A file that is gone once it is dropped. Where an open file can lose its name (Unix), it loses
/// it the moment it is opened, so that not even a killed process leaves it behind.
struct Temporary {
    file: Option<File>,
    /// Its name, if it still has one.
    path: Option<PathBuf>,
}

/// How many temporary files this process has made, for names of their own.
static MADE: AtomicUsize = AtomicUsize::new(0);

impl Temporary {
    /// A temporary file in `folder` holding `texts`, one after another, ready to be read.
    fn holding(texts: &[String], folder: &Path) -> io::Result<Self> {
        let temporary = Self::create(folder)?;
        let mut file = temporary.file.as_ref().expect("open until dropped");
        for text in texts {
            file.write_all(text.as_bytes())?;
        }
        file.rewind()?;
        Ok(temporary)
    }

    fn create(folder: &Path) -> io::Result<Self> {
        for _ in 0..64 {
            let made = MADE.fetch_add(1, Ordering::Relaxed);
            let path = folder.join(format!(".orimoji-buffer-{}-{made}", std::process::id()));
            match OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
                Ok(file) => {
                    let path = std::fs::remove_file(&path).is_err().then_some(path);
                    return Ok(Self { file: Some(file), path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free name for a temporary file"))
    }

    /// What it holds, as texts of these lengths, in order.
    fn read(self, lengths: &[usize]) -> io::Result<Vec<String>> {
        let mut file = self.file.as_ref().expect("open until dropped");
        lengths
            .iter()
            .map(|&length| {
                let mut bytes = vec![0; length];
                file.read_exact(&mut bytes)?;
                String::from_utf8(bytes)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            })
            .collect()
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // Closed first: a system that kept its name may not remove an open file.
        drop(self.file.take());
        if let Some(path) = &self.path {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts() -> Vec<String> {
        vec!["=== orimoji 1 text base64url ===\n".into(), String::new(), "ü and 白\n".repeat(500)]
    }

    /// A folder of the test's own, empty.
    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("orimoji-buffer-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// Whatever is in `folder`, which is then removed.
    fn left_in(folder: &Path) -> usize {
        let left = std::fs::read_dir(folder).unwrap().count();
        let _ = std::fs::remove_dir(folder);
        left
    }

    #[test]
    fn texts_past_the_limit_wait_in_a_file_and_come_back_the_same() {
        let folder = folder("back");
        let mut buffered = Buffered::new(texts());
        buffered.buffer(usize::MAX, &folder);
        assert!(!buffered.in_a_file(), "texts under the limit stay in memory");
        buffered.buffer(0, &folder);
        assert!(buffered.in_a_file());
        assert_eq!(buffered.size(), texts().iter().map(String::len).sum::<usize>());
        assert_eq!(buffered.take().unwrap(), texts());
        assert_eq!(left_in(&folder), 0, "the file is gone");
    }

    #[test]
    fn a_buffer_dropped_unread_leaves_nothing_behind() {
        let folder = folder("dropped");
        let mut buffered = Buffered::new(texts());
        buffered.buffer(0, &folder);
        assert!(buffered.in_a_file());
        drop(buffered);
        assert_eq!(left_in(&folder), 0);
    }

    #[test]
    fn texts_that_cant_be_buffered_stay_in_memory() {
        let mut buffered = Buffered::new(texts());
        buffered.buffer(0, Path::new("/nonexistent/orimoji"));
        assert!(!buffered.in_a_file());
        assert_eq!(buffered.take().unwrap(), texts());
    }
}
