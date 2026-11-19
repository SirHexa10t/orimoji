//! How long a measurement may take: a moment past which the work in progress gives up, checked
//! every so often along the way, so a fold out of time stops within moments of it.

use crate::error::FoldError;
use std::time::{Duration, Instant};

/// The moment work must stop by, if there is one.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Deadline(Option<Instant>);

/// How many lines or strings pass between two looks at the clock.
pub(crate) const STRIDE: usize = 1024;

impl Deadline {
    /// No moment at all: the work runs to its end.
    pub(crate) const NONE: Deadline = Deadline(None);

    /// `limit` from now. A limit too far off to count is no limit.
    pub(crate) fn after(limit: Duration) -> Self {
        Deadline(Instant::now().checked_add(limit))
    }

    /// Out of time, once the moment has passed.
    pub(crate) fn check(self) -> Result<(), FoldError> {
        match self.0 {
            Some(moment) if Instant::now() >= moment => Err(FoldError::OutOfTime),
            _ => Ok(()),
        }
    }

    /// [`Deadline::check`], but only on every [`STRIDE`]th `at`.
    pub(crate) fn check_at(self, at: usize) -> Result<(), FoldError> {
        match at % STRIDE {
            0 => self.check(),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_deadline_never_passes_and_a_spent_one_always_has() {
        assert!(Deadline::NONE.check().is_ok());
        assert!(Deadline::after(Duration::from_secs(3600)).check().is_ok());
        assert!(matches!(Deadline::after(Duration::ZERO).check(), Err(FoldError::OutOfTime)));
        assert!(Deadline::after(Duration::MAX).check().is_ok(), "too far off to count");
        assert!(Deadline::after(Duration::ZERO).check_at(1).is_ok(), "between looks at the clock");
    }
}
