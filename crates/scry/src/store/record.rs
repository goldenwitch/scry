//! The bytes the store keeps its stamp and its documents as.
//!
//! The layout is private to the store, so this is the only place that knows
//! it, and changing it is a matter of raising [`LAYOUT`] rather than of
//! migrating anything: a store written under another number does not open.

use std::io;
use std::time::{Duration, UNIX_EPOCH};

use hmac_sha256::Hash;

use crate::document::Document;
use crate::embedding::Embedding;
use crate::model::Model;
use crate::origin::Origin;
use crate::text::Text;
use crate::vector::Vector;

/// How this build writes a store.
///
/// It is stamped into the file beside the model, and both are compared byte
/// for byte on the way back in, so another layout and another model are one
/// check rather than two.
///
/// Raised to 2 when the record gained its checksum. A store written under 1
/// holds records shaped as this build no longer reads them, and the number is
/// what makes it say so at `open` rather than at the first read.
const LAYOUT: u32 = 2;

/// A second, in nanoseconds.
const NANOS_PER_SECOND: u32 = 1_000_000_000;

/// The width of the checksum every record ends with.
///
/// The text half of a record was already vouched for from outside: a handle
/// carries the digest of the text it was cut from, so text read back other
/// than as it was written is refused where an agent uses it. The vector half
/// had nothing over it. A flipped bit in a float is still a float,
/// [`Vector::normalise`] gives the altered numbers a length of one and a
/// direction of their own, and the document then ranks by a vector nobody
/// wrote — with no refusal and no error. The record covers its own bytes, so
/// both halves are read back on the same terms.
///
/// Compared with plain equality, as a handle's digest is: there is no secret
/// here whose comparison could leak, only bytes that agree or do not.
pub(super) const CHECKSUM: usize = 32;

/// The bytes that say which layout and which model wrote this store.
///
/// # Errors
///
/// A model whose dimension or limit does not fit the width written here,
/// which no model this build holds can be.
pub(super) fn stamp(model: &Model) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&LAYOUT.to_le_bytes());
    bytes.extend_from_slice(&count(model.dimension().get())?);
    bytes.extend_from_slice(&count(model.limit().tokens().get())?);
    bytes.extend_from_slice(model.name().as_bytes());
    Ok(bytes)
}

/// A document and the embedding beside each of its spans, as bytes.
///
/// The origin is not written: it is the key the record is filed under, and an
/// origin's spelling parses back to the same origin, so writing it twice would
/// be two places for one fact to disagree.
///
/// # Errors
///
/// Embeddings that are not one per span, or that name another model than the
/// one this store was stamped with. Neither is one of the refusals scry names:
/// they are a caller handing the store a corpus it did not open for, which is
/// a mistake rather than a decision.
pub(super) fn encode(
    document: &Document,
    embeddings: &[Embedding],
    model: &Model,
) -> io::Result<Vec<u8>> {
    if embeddings.len() != document.spans().len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the document was given other than one embedding per span",
        ));
    }
    if embeddings
        .iter()
        .any(|embedding| embedding.model() != model)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the embeddings were produced by another model than the store was opened with",
        ));
    }
    let mut bytes = Vec::new();
    let fetched_at = document
        .fetched_at()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?;
    push_duration(&mut bytes, fetched_at);
    push_duration(&mut bytes, document.ttl());
    let text = document.text().as_str();
    bytes.extend_from_slice(&count(text.len())?);
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(&count(document.spans().len())?);
    for span in document.spans() {
        bytes.extend_from_slice(&count(span.start())?);
        bytes.extend_from_slice(&count(span.end())?);
    }
    for embedding in embeddings {
        for value in embedding.vector().as_slice() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&Hash::hash(&bytes));
    Ok(bytes)
}

/// The document filed under `origin`, and the embedding beside each span.
///
/// The document is rebuilt through [`Document::new`], so the partition the
/// type guarantees is checked again on the way out rather than assumed to have
/// survived the disk.
///
/// The checksum is read first, so nothing in the record is believed before the
/// record says it is the one that was written.
///
/// # Errors
///
/// Bytes that are not what this layout writes, and bytes that are not the ones
/// [`encode`] left here.
pub(super) fn decode(
    origin: &Origin,
    bytes: &[u8],
    model: &Model,
) -> io::Result<(Document, Vec<Embedding>)> {
    let Some(split) = bytes.len().checked_sub(CHECKSUM) else {
        return Err(invalid());
    };
    let Some((body, checksum)) = bytes.split_at_checked(split) else {
        return Err(invalid());
    };
    if Hash::hash(body).as_slice() != checksum {
        return Err(invalid());
    }
    let mut reader = Reader(body);
    let fetched_at = UNIX_EPOCH
        .checked_add(reader.duration()?)
        .ok_or_else(invalid)?;
    let ttl = reader.duration()?;
    let length = reader.length()?;
    let text = Text::from_utf8(reader.take(length)?.to_vec()).ok_or_else(invalid)?;
    let cuts = reader.length()?;
    // The count is read off the record, so it is not trusted to size an
    // allocation: a corrupt length would otherwise ask for the whole machine.
    let mut spans = Vec::new();
    for _ in 0..cuts {
        let start = reader.length()?;
        let end = reader.length()?;
        spans.push(text.span(start, end).ok_or_else(invalid)?);
    }
    let mut embeddings = Vec::new();
    for _ in 0..cuts {
        let mut values = Vec::new();
        for _ in 0..model.dimension().get() {
            values.push(reader.float()?);
        }
        let vector = Vector::normalise(values).ok_or_else(invalid)?;
        embeddings.push(Embedding::new(model.clone(), vector).ok_or_else(invalid)?);
    }
    if !reader.is_empty() {
        return Err(invalid());
    }
    let document =
        Document::new(origin.clone(), text, fetched_at, ttl, spans).ok_or_else(invalid)?;
    Ok((document, embeddings))
}

/// A length as the eight bytes it is written as.
fn count(value: usize) -> io::Result<[u8; 8]> {
    u64::try_from(value)
        .map(u64::to_le_bytes)
        .map_err(io::Error::other)
}

/// Seconds and then nanoseconds, which is how both a moment and a ttl are
/// written.
fn push_duration(bytes: &mut Vec<u8>, duration: Duration) {
    bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
    bytes.extend_from_slice(&duration.subsec_nanos().to_le_bytes());
}

/// What a record that is not what this layout writes answers with.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "the store holds a record this layout cannot read",
    )
}

/// A record, read front to back.
///
/// Every read is bounds-checked and every width is exact, so a truncated or
/// forged record ends as [`invalid`] rather than as a panic.
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let Some((head, tail)) = self.0.split_at_checked(count) else {
            return Err(invalid());
        };
        self.0 = tail;
        Ok(head)
    }

    fn eight(&mut self) -> io::Result<u64> {
        let bytes: [u8; 8] = self.take(8)?.try_into().map_err(|_| invalid())?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn four(&mut self) -> io::Result<u32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| invalid())?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn length(&mut self) -> io::Result<usize> {
        usize::try_from(self.eight()?).map_err(|_| invalid())
    }

    fn float(&mut self) -> io::Result<f32> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| invalid())?;
        Ok(f32::from_le_bytes(bytes))
    }

    fn duration(&mut self) -> io::Result<Duration> {
        let seconds = self.eight()?;
        let nanos = self.four()?;
        if nanos >= NANOS_PER_SECOND {
            return Err(invalid());
        }
        Duration::from_secs(seconds)
            .checked_add(Duration::from_nanos(u64::from(nanos)))
            .ok_or_else(invalid)
    }
}
