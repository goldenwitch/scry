//! `count`: how many hits or passages are wanted.

use core::num::NonZeroUsize;

/// How many are wanted.
///
/// Asking for none is not a question, so it is not a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Count(NonZeroUsize);

impl Count {
    /// A count of `wanted`, or `None` if that is none.
    #[must_use]
    pub const fn new(wanted: usize) -> Option<Self> {
        match NonZeroUsize::new(wanted) {
            Some(wanted) => Some(Self(wanted)),
            None => None,
        }
    }

    /// How many.
    #[must_use]
    pub const fn get(&self) -> NonZeroUsize {
        self.0
    }
}
