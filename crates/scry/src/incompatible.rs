//! `Incompatible`: not a refusal but an end.

use core::fmt;
use std::error::Error;
use std::path::{Path, PathBuf};

/// The store at that path is not one this build can use.
///
/// It was written by another model or another layout, or its own bytes
/// disagree with each other. Which of the three it is changes nothing an
/// agent can do about it, so the word does not say.
///
/// A terminal state earns a word precisely because nothing proceeds from it:
/// no verb runs, and nothing an agent does will change that. It is the only
/// failure that is not a refusal, so it does not sit with them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Incompatible {
    path: PathBuf,
}

impl Incompatible {
    /// The store that cannot be opened.
    #[must_use]
    pub(crate) const fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Where that store is.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for Incompatible {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the store at {} is not one this build can use",
            self.path.display()
        )
    }
}

impl Error for Incompatible {}
