//! `digest`: a hash of a chunk's text.

/// A hash of a chunk's text.
///
/// Which hash is the handle seam's to choose; that it is 256 bits wide and
/// fixed is what lets a digest be a value rather than an allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The digest these bytes are.
    #[must_use]
    pub(crate) const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
