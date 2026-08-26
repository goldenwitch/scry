//! `hit`: a passage and its score.

use crate::passage::Passage;
use crate::score::Score;

/// A [`Passage`] and the [`Score`] the query gave it.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    passage: Passage,
    score: Score,
}

impl Hit {
    /// The passage, at that score.
    #[must_use]
    pub(crate) const fn new(passage: Passage, score: Score) -> Self {
        Self { passage, score }
    }

    /// What was found.
    #[must_use]
    pub const fn passage(&self) -> &Passage {
        &self.passage
    }

    /// How well it matched.
    #[must_use]
    pub const fn score(&self) -> Score {
        self.score
    }
}
