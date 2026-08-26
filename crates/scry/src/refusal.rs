//! The refusals: decisions scry makes and an agent can act on.
//!
//! Four of the vocabulary's five failures live here. The fifth,
//! [`Incompatible`](crate::Incompatible), is not a refusal but an end.
//!
//! They are grouped by the verbs that can answer with them, so a verb cannot
//! be written to return a refusal it has no way to reach.

use core::fmt;
use std::error::Error;

use crate::origin::Origin;

/// What `add` answers with instead of a document.
///
/// Each is repaired differently, which is why each has its own name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AddRefusal {
    /// There are no bytes at that origin.
    NotFound(Origin),
    /// The bytes there are not something scry will index.
    NotText(Origin),
}

impl fmt::Display for AddRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(origin) => write!(f, "no bytes at {origin}"),
            Self::NotText(origin) => write!(f, "the bytes at {origin} are not text"),
        }
    }
}

impl Error for AddRefusal {}

/// What a verb given a handle answers with instead of what the handle names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum HandleRefusal {
    /// The text at that span is no longer the text the handle was cut from.
    Stale(Origin),
    /// The document is no longer held.
    Gone(Origin),
}

impl fmt::Display for HandleRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stale(origin) => write!(f, "the text at that span of {origin} has changed"),
            Self::Gone(origin) => write!(f, "{origin} is no longer held"),
        }
    }
}

impl Error for HandleRefusal {}
