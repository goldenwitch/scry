//! `span`: a `start` and an `end`, byte offsets into a document's text.

/// `start` and `end`: byte offsets into a document's `text`.
///
/// Spans are only produced from the text they index, which is checked against
/// them, so they fall on character boundaries and stay within their text by
/// construction rather than by checking later. Scry cuts them; a caller reads
/// them off the chunk a handle names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    start: usize,
    end: usize,
}

impl Span {
    /// The offsets, already checked against the text they index.
    pub(crate) const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The byte offset the span begins at.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// The byte offset the span ends at, exclusive.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Whether the span covers no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }
}
