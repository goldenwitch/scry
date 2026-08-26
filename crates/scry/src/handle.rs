//! `handle`: a chunk and the digest of the text it was cut from.
//!
//! The handle seam is here rather than in a struct of its own, because it
//! holds nothing: minting and verifying are two readings of one hash, and this
//! is the only place text is hashed.

use hmac_sha256::Hash;

use crate::chunk::Chunk;
use crate::digest::Digest;
use crate::document::Document;
use crate::passage::Passage;
use crate::span::Span;
use crate::text::Text;

/// A [`Chunk`] and a [`Digest`] of the text at it: what vouches for a passage.
///
/// A handle carries the digest of its own text, so a handle handed back is
/// checked against the text there now, and there is no table to look it up in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Handle {
    chunk: Chunk,
    digest: Digest,
}

impl Handle {
    /// Reconstructs a handle received from outside this process.
    ///
    /// The digest is a claim about the text at `origin` and `start..end`; it
    /// cannot be checked until a verb looks up that origin. Rehydration only
    /// rejects a reversed span, while `provenance` and `neighbours` decide
    /// whether the claim is still true and answer `Stale` or `Gone`.
    #[must_use]
    pub fn from_parts(
        origin: crate::Origin,
        start: usize,
        end: usize,
        digest: [u8; 32],
    ) -> Option<Self> {
        (start <= end).then(|| {
            Self::new(
                Chunk::new(origin, Span::new(start, end)),
                Digest::new(digest),
            )
        })
    }

    /// The handle for `chunk` whose text hashes to `digest`.
    ///
    /// Minting — hashing the text — belongs to the handle seam; this only
    /// holds the pair. A handle assembled anywhere else either agrees with the
    /// text at the span, and is right, or is refused when it is verified.
    #[must_use]
    pub(crate) const fn new(chunk: Chunk, digest: Digest) -> Self {
        Self { chunk, digest }
    }

    /// Mints the handle for the chunk at `span` of `document`, with the
    /// passage it vouches for.
    ///
    /// The text leaves with the handle because every handle an agent holds was
    /// minted from text the store held at that moment — so the pairing is made
    /// here, where the hash was taken, rather than by each caller reading the
    /// span a second time.
    ///
    /// `None` if `span` is not one of the document's spans. A chunk is a span
    /// of the partition, so a handle over a place nobody cut is not minted at
    /// all.
    #[must_use]
    pub(crate) fn mint(document: &Document, span: Span) -> Option<Passage> {
        document.spans().binary_search(&span).ok()?;
        let text = document.text().at(span)?;
        let chunk = Chunk::new(document.origin().clone(), span);
        let handle = Self::new(chunk, digest(text));
        Some(Passage::new(handle, Text::from(text.to_owned())))
    }

    /// The text this handle still vouches for in `document`, or `None` if it
    /// no longer does — because the span no longer fits the text, or because
    /// the text there has changed.
    ///
    /// `None` is what a verb turns into `Stale`. It is not a
    /// [`HandleRefusal`](crate::HandleRefusal) here: `Gone` is the other half
    /// of that word, and this seam holds no corpus to find a document absent
    /// from, so the verb that looked the document up is the one that can tell
    /// the two apart.
    ///
    /// The document's current cut is not consulted. `Stale` says the text is
    /// no longer there, and re-slicing a document does not move its bytes.
    #[must_use]
    pub(crate) fn verify<'a>(&self, document: &'a Document) -> Option<&'a str> {
        if document.origin() != self.chunk.origin() {
            return None;
        }
        let text = document.text().at(self.chunk.span())?;
        (digest(text) == self.digest).then_some(text)
    }

    /// Where the text was cut from.
    #[must_use]
    pub const fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// The hash of the text it was cut from.
    #[must_use]
    pub const fn digest(&self) -> Digest {
        self.digest
    }
}

/// The digest of `text`: SHA-256, over the chunk's text and nothing else.
///
/// Compared with plain equality rather than in constant time. A handle is
/// refused when it disagrees with the text at its span, so a forged one is
/// simply wrong; there is no secret here to leak the comparison of.
fn digest(text: &str) -> Digest {
    Digest::new(Hash::hash(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::Handle;
    use crate::document::Document;
    use crate::scaffold::{document as read, minted as handle_at};
    use crate::span::Span;

    /// The first span of `document`.
    fn first(document: &Document) -> Span {
        let Some(span) = document.spans().first().copied() else {
            unreachable!()
        };
        span
    }

    fn minted(document: &Document) -> Handle {
        handle_at(document, 0)
    }

    #[test]
    fn a_handle_verifies_against_the_text_it_was_cut_from() {
        let document = read("a.md", "one two three", &[7]);
        let handle = minted(&document);
        assert_eq!(handle.verify(&document), Some("one two"));
    }

    #[test]
    fn a_handle_fails_against_edited_text() {
        let handle = minted(&read("a.md", "one two three", &[7]));
        let edited = read("a.md", "ONE two three", &[7]);
        assert_eq!(handle.verify(&edited), None);
    }

    #[test]
    fn a_handle_fails_against_a_span_that_no_longer_fits() {
        let document = read("a.md", "one two three", &[7]);
        let Some(span) = document.spans().last().copied() else {
            unreachable!()
        };
        let Some(passage) = Handle::mint(&document, span) else {
            unreachable!()
        };
        let shortened = read("a.md", "one", &[]);
        assert_eq!(passage.handle().verify(&shortened), None);
    }

    #[test]
    fn text_changing_elsewhere_does_not_stale_a_handle() {
        let handle = minted(&read("a.md", "one two three", &[7]));
        let edited = read("a.md", "one two THREE", &[7]);
        assert_eq!(handle.verify(&edited), Some("one two"));
    }

    #[test]
    fn a_handle_does_not_verify_against_another_document() {
        let handle = minted(&read("a.md", "one two three", &[7]));
        let other = read("b.md", "one two three", &[7]);
        assert_eq!(handle.verify(&other), None);
    }

    #[test]
    fn a_span_the_document_was_not_cut_into_mints_nothing() {
        let document = read("a.md", "one two three", &[7]);
        let Some(span) = document.text().span(0, 3) else {
            unreachable!()
        };
        assert!(Handle::mint(&document, span).is_none());
    }

    #[test]
    fn the_passage_carries_the_text_at_the_span() {
        let document = read("a.md", "one two three", &[7]);
        let Some(passage) = Handle::mint(&document, first(&document)) else {
            unreachable!()
        };
        assert_eq!(passage.text().as_str(), "one two");
        assert_eq!(passage.handle().chunk().span(), first(&document));
    }

    /// The digest is SHA-256 of the chunk's text, and of nothing else: this is
    /// the published vector for `abc`, so which hash this is is measured here
    /// rather than asserted in prose, and a digest that also covered the origin
    /// or the span would not match it.
    #[test]
    fn the_digest_is_sha256_of_the_text() {
        let handle = minted(&read("a.md", "abc", &[]));
        assert_eq!(
            handle.digest().as_bytes(),
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }
}
