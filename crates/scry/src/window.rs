//! `window`: the chunk size slice targets, counted in tokens.

use core::num::NonZeroUsize;

use crate::limit::Limit;

/// The chunk size slice targets, in tokens.
///
/// A window is only made against the limit it must fit inside, so a window
/// that exceeds its model's limit cannot be held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Window(NonZeroUsize);

impl Window {
    /// A window of `tokens`, or `None` if that is no tokens at all or more
    /// than `limit`.
    #[must_use]
    pub(crate) fn new(tokens: usize, limit: Limit) -> Option<Self> {
        NonZeroUsize::new(tokens)
            .filter(|tokens| *tokens <= limit.tokens())
            .map(Self)
    }

    /// The window in tokens.
    #[must_use]
    pub(crate) const fn tokens(self) -> NonZeroUsize {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Window;
    use crate::limit::Limit;

    fn limit() -> Limit {
        let Some(limit) = Limit::new(512) else {
            unreachable!()
        };
        limit
    }

    #[test]
    fn a_window_cannot_exceed_its_limit() {
        assert_eq!(Window::new(513, limit()), None);
    }

    #[test]
    fn a_window_may_equal_its_limit() {
        assert!(Window::new(512, limit()).is_some());
    }

    #[test]
    fn a_window_of_no_tokens_is_not_a_window() {
        assert_eq!(Window::new(0, limit()), None);
    }
}
