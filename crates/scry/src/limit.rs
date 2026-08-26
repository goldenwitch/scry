//! `limit`: the most input the model accepts, counted in tokens.

use core::num::NonZeroUsize;

/// The most input the model accepts, in tokens.
///
/// It bounds [`Window`](crate::window::Window), so slice cannot hand the
/// model text it will not accept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Limit(NonZeroUsize);

impl Limit {
    /// A limit of `tokens`, or `None` if that is no tokens at all.
    #[must_use]
    pub(crate) const fn new(tokens: usize) -> Option<Self> {
        match NonZeroUsize::new(tokens) {
            Some(tokens) => Some(Self(tokens)),
            None => None,
        }
    }

    /// The limit in tokens.
    #[must_use]
    pub(crate) const fn tokens(self) -> NonZeroUsize {
        self.0
    }
}
