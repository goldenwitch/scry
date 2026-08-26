//! `query`: the text searched with.

use crate::text::Text;

/// The text searched with.
///
/// A query is not a passage: the model is told which it is doing, and it can
/// only be told correctly if the two are different things.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Query(Text);

impl Query {
    /// The query this text is.
    #[must_use]
    pub const fn new(text: Text) -> Self {
        Self(text)
    }

    /// The text.
    #[must_use]
    pub const fn text(&self) -> &Text {
        &self.0
    }
}
