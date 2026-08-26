//! `chunk`: an origin and a span within it.

use crate::origin::Origin;
use crate::span::Span;

/// An [`Origin`] and a [`Span`] within its text: the unit every answer scry
/// gives is cut from.
///
/// The text is not here. It is projected from the document, so a chunk is a
/// place rather than a copy.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chunk {
    origin: Origin,
    span: Span,
}

impl Chunk {
    /// The chunk at `span` of `origin`.
    #[must_use]
    pub(crate) const fn new(origin: Origin, span: Span) -> Self {
        Self { origin, span }
    }

    /// The document it is cut from.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Where in that document.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.span
    }
}
