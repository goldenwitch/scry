//! `provenance`: where a passage came from, and how old the material is —
//! and the verb that hands one back.
//!
//! The noun and the verb share this file because the vocabulary gives them one
//! word. `add`, `delete`, `search` and `neighbours` each took a file of their
//! own, but each of those names a thing no noun does; a second file here would
//! have to be called something the vocabulary does not say, and inventing a
//! word to hold a word is the trade the other way round.
//!
//! Nothing new is decided by the verb. `store` says what one document per
//! origin means and `handle` says how a claim about text is judged — this is
//! the order they run in, and what leaves is the chunk the handle already
//! named beside the two facts the document holds about its age.

use std::io;
use std::time::{Duration, SystemTime};

use crate::chunk::Chunk;
use crate::handle::Handle;
use crate::refusal::HandleRefusal;
use crate::store::Store;

/// A [`Chunk`], the moment its document was read, and the `ttl` given for it.
///
/// Whoever holds one judges the material's age with [`Provenance::expired`].
/// Nothing else does: nothing is excluded from search, and nothing refetches
/// itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Provenance {
    chunk: Chunk,
    fetched_at: SystemTime,
    ttl: Duration,
}

trait ProvenanceVerb {
    fn provenance(&self, handle: &Handle) -> io::Result<Result<Provenance, HandleRefusal>>;
}

impl Provenance {
    /// The provenance of `chunk`, from the document read at `fetched_at` with
    /// this `ttl`.
    #[must_use]
    pub(crate) const fn new(chunk: Chunk, fetched_at: SystemTime, ttl: Duration) -> Self {
        Self {
            chunk,
            fetched_at,
            ttl,
        }
    }

    /// Where the passage came from.
    #[must_use]
    pub const fn chunk(&self) -> &Chunk {
        &self.chunk
    }

    /// When the document was read.
    #[must_use]
    pub const fn fetched_at(&self) -> SystemTime {
        self.fetched_at
    }

    /// How long the material was said to stay good for.
    #[must_use]
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// Whether `now` is past `fetched_at` plus `ttl`.
    ///
    /// Advisory, and a different claim from a refusal: this says the material
    /// has aged, not that the text is no longer there.
    ///
    /// The material's age is measured against the ttl rather than `now`
    /// against a deadline. `fetched_at` plus `ttl` need not be a moment the
    /// platform can hold — a ttl of a few thousand years runs a `SystemTime`
    /// off the end of what it counts in — and an addition that cannot be
    /// computed has to be answered with something. Answering `false` there is
    /// reading fresh because the question was not asked. An age is a duration
    /// between two moments that both exist, so there is nothing to leave the
    /// representable range and no answer to invent.
    #[must_use]
    pub fn expired(&self, now: SystemTime) -> bool {
        now.duration_since(self.fetched_at)
            .is_ok_and(|age| age > self.ttl)
    }
}

impl ProvenanceVerb for Store {
    fn provenance(&self, handle: &Handle) -> io::Result<Result<Provenance, HandleRefusal>> {
        let origin = handle.chunk().origin();
        let document = self.document(origin)?;
        crate::benchmark::record_provenance_lookup();
        let Some(document) = document else {
            return Ok(Err(HandleRefusal::Gone(origin.clone())));
        };
        if handle.verify(&document).is_none() {
            return Ok(Err(HandleRefusal::Stale(origin.clone())));
        }
        Ok(Ok(Provenance::new(
            handle.chunk().clone(),
            document.fetched_at(),
            document.ttl(),
        )))
    }
}

impl Store {
    /// Checks `handle` against the text held now, then answers with where that
    /// text came from and how old the material is.
    ///
    /// The chunk that comes back is the handle's own. Verifying is what makes
    /// it exact: the text at that span in the document held now is the text
    /// the handle was cut from, so the origin and the span still name where
    /// those bytes are. `fetched_at` and `ttl` are the document's, and the
    /// only two facts here the handle did not carry in.
    ///
    /// Nothing is judged about the age. `expired` is computed by whoever holds
    /// the provenance, from the two facts in it, and this verb does not
    /// consult a clock.
    ///
    /// # Errors
    ///
    /// [`Stale`](HandleRefusal::Stale) when the text at that span is no longer
    /// the text the handle was cut from, and [`Gone`](HandleRefusal::Gone)
    /// when the document is no longer held. Those two are the whole of what
    /// this verb decides, and it is the store that tells them apart: the seam
    /// that verifies holds no corpus, so an absent document is this file's to
    /// name.
    ///
    /// Outside them: the disk, which is weather. It is nested outside the
    /// refusal rather than seated beside it, as [`open`](Store::open),
    /// [`add`](Store::add) and [`neighbours`](Store::neighbours) do.
    pub fn provenance(&self, handle: &Handle) -> io::Result<Result<Provenance, HandleRefusal>> {
        <Self as ProvenanceVerb>::provenance(self, handle)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    use super::Provenance;
    use crate::chunk::Chunk;
    use crate::handle::Handle;
    use crate::origin::Origin;
    use crate::refusal::HandleRefusal;
    use crate::scaffold::{self, TTL, document, filed, minted, model, store};
    use crate::store::Store;
    use crate::text::Text;

    fn provenance(fetched_at: SystemTime, ttl: Duration) -> Provenance {
        let text = Text::from("a document".to_owned());
        let Some(span) = text.span(0, text.len()) else {
            unreachable!()
        };
        let Some(origin) = Origin::parse("a.md") else {
            unreachable!()
        };
        let chunk = Chunk::new(origin, span);
        Provenance::new(chunk, fetched_at, ttl)
    }

    #[test]
    fn material_within_its_ttl_has_not_expired() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let provenance = provenance(now, Duration::from_secs(60));
        assert!(!provenance.expired(now + Duration::from_secs(59)));
    }

    #[test]
    fn material_past_its_ttl_has() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let provenance = provenance(now, Duration::from_secs(60));
        assert!(provenance.expired(now + Duration::from_secs(61)));
    }

    #[test]
    fn a_ttl_with_no_deadline_is_answered_from_the_age() {
        let fetched_at = SystemTime::UNIX_EPOCH + Duration::from_secs(100);
        let provenance = provenance(fetched_at, Duration::MAX);
        // The provocation, stated rather than described: there is no moment
        // this platform can hold that is `fetched_at` plus this ttl.
        assert_eq!(fetched_at.checked_add(Duration::MAX), None);
        // The answer is the age against the ttl, and the age is a duration
        // between two moments that both exist.
        assert!(!provenance.expired(fetched_at + Duration::from_secs(1_000_000)));
        assert!(!provenance.expired(fetched_at - Duration::from_secs(1)));
    }

    /// A store of this test's own, emptied first so a rerun starts where the
    /// last one did.
    fn path(test: &str) -> PathBuf {
        scaffold::path(module_path!(), test)
    }

    fn traced(store: &Store, handle: &Handle) -> Provenance {
        match store.provenance(handle) {
            Ok(Ok(provenance)) => provenance,
            Ok(Err(refusal)) => unreachable!("{refusal}"),
            Err(error) => unreachable!("{error}"),
        }
    }

    fn refused(store: &Store, handle: &Handle) -> HandleRefusal {
        match store.provenance(handle) {
            Ok(Ok(_)) => unreachable!("the handle was not refused"),
            Ok(Err(refusal)) => refusal,
            Err(error) => unreachable!("{error}"),
        }
    }

    /// Three chunks, each two bytes, so a span in the answer is legible.
    const BODY: &str = "aabbcc";
    const CUTS: &[usize] = &[2, 4];

    /// A store holding one three-chunk document, and the handle for its middle
    /// chunk.
    fn middle(test: &str) -> (Store, Handle) {
        let model = model("test");
        let store = store(&path(test), &model);
        let document = document("a.md", BODY, CUTS);
        filed(&store, &model, &document);
        let handle = minted(&document, 1);
        (store, handle)
    }

    /// The node's first criterion. Every one of the four things job 3 asks for
    /// comes back, and each is the one the document holds rather than a
    /// plausible neighbour of it — so the span is checked against the middle
    /// chunk's offsets rather than against the document's.
    #[test]
    fn a_fresh_handle_traces_back_to_exactly_where_its_text_is() {
        let (store, handle) = middle("exact");
        let provenance = traced(&store, &handle);
        assert_eq!(provenance.chunk(), handle.chunk());
        assert_eq!(provenance.chunk().span().start(), 2);
        assert_eq!(provenance.chunk().span().end(), 4);
        assert_eq!(provenance.fetched_at(), scaffold::fetched_at());
        assert_eq!(provenance.ttl(), TTL);
    }

    /// The document is written again at the same origin with different bytes
    /// at that span. The chunk's text is elsewhere in the new document, so
    /// this is the case where answering would be answering wrongly.
    #[test]
    fn a_handle_whose_text_has_changed_is_stale() {
        let model = model("test");
        let store = store(&path("stale"), &model);
        let read = document("a.md", BODY, CUTS);
        filed(&store, &model, &read);
        let handle = minted(&read, 1);
        filed(&store, &model, &document("a.md", "aaxxbb", CUTS));
        let Some(origin) = Origin::parse("a.md") else {
            unreachable!()
        };
        assert_eq!(refused(&store, &handle), HandleRefusal::Stale(origin));
    }

    /// The node's third criterion, and the line delete drew: the whole record
    /// leaves, so the document is not there to disagree with the handle.
    #[test]
    fn a_handle_into_a_deleted_document_is_gone() {
        let (store, handle) = middle("gone");
        let Ok(()) = store.delete(handle.chunk().origin()) else {
            unreachable!()
        };
        assert_eq!(
            refused(&store, &handle),
            HandleRefusal::Gone(handle.chunk().origin().clone())
        );
    }

    /// A handle into a document that was never added is `Gone` too. It is the
    /// same corpus read the other way round — the store answers absent, and
    /// nothing here asks how it came to be.
    #[test]
    fn a_handle_into_a_document_never_added_is_gone_as_well() {
        let (store, _) = middle("absent");
        let elsewhere = document("b.md", BODY, CUTS);
        let handle = minted(&elsewhere, 1);
        assert_eq!(
            refused(&store, &handle),
            HandleRefusal::Gone(handle.chunk().origin().clone())
        );
    }

    /// The age is the caller's to judge, and the two facts needed to judge it
    /// survive the store. Nothing here refetches, and no clock was read to
    /// produce the answer.
    #[test]
    fn the_age_is_computed_by_whoever_holds_the_provenance() {
        let (store, handle) = middle("age");
        let provenance = traced(&store, &handle);
        assert!(!provenance.expired(scaffold::fetched_at() + TTL - Duration::from_secs(1)));
        assert!(provenance.expired(scaffold::fetched_at() + TTL + Duration::from_secs(1)));
    }
}
