//! `model`: the embedder, by name, dimension and limit.

use core::num::NonZeroUsize;

use crate::limit::Limit;

/// The embedder: its `name`, the `dimension` of the vectors it produces, and
/// the `limit` of what it accepts.
///
/// Which model that is belongs to the embed seam, not here.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Model {
    name: String,
    dimension: NonZeroUsize,
    limit: Limit,
}

impl Model {
    /// A model of this name, dimension and limit, or `None` if it produces
    /// vectors of no floats.
    #[must_use]
    pub(crate) fn new(name: impl Into<String>, dimension: usize, limit: Limit) -> Option<Self> {
        NonZeroUsize::new(dimension).map(|dimension| Self {
            name: name.into(),
            dimension,
            limit,
        })
    }

    /// The model's name.
    #[must_use]
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// How many floats are in one of its vectors.
    #[must_use]
    pub(crate) const fn dimension(&self) -> NonZeroUsize {
        self.dimension
    }

    /// The most input it accepts.
    #[must_use]
    pub(crate) const fn limit(&self) -> Limit {
        self.limit
    }
}
