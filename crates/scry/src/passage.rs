//! `passage`: text, and the handle that vouches for it.

use crate::handle::Handle;
use crate::text::Text;

/// What an agent reads: the [`Text`], and the [`Handle`] that vouches for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Passage {
    handle: Handle,
    text: Text,
}

impl Passage {
    /// The passage `handle` vouches for.
    #[must_use]
    pub(crate) const fn new(handle: Handle, text: Text) -> Self {
        Self { handle, text }
    }

    /// What vouches for the text.
    #[must_use]
    pub const fn handle(&self) -> &Handle {
        &self.handle
    }

    /// The text itself.
    #[must_use]
    pub const fn text(&self) -> &Text {
        &self.text
    }
}
