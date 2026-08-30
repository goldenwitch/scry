//! `neighbours`: the verb that widens a passage into the text around it.
//!
//! Nothing new is decided here. `store` says what one document per origin
//! means, `document` says how a text is cut, and `handle` says what vouches
//! for a passage and how a claim about it is judged — this is the order they
//! run in. What this file owns is which of a document's chunks are adjacent to
//! one of them, and how many of those are wanted.

use std::io;

use crate::count::Count;
use crate::handle::Handle;
use crate::passage::Passage;
use crate::refusal::HandleRefusal;
use crate::span::Span;
use crate::store::Store;

trait NeighboursVerb {
    fn neighbours(
        &self,
        handle: &Handle,
        count: Count,
    ) -> io::Result<Result<Vec<Passage>, HandleRefusal>>;
}

impl NeighboursVerb for Store {
    fn neighbours(
        &self,
        handle: &Handle,
        count: Count,
    ) -> io::Result<Result<Vec<Passage>, HandleRefusal>> {
        let origin = handle.chunk().origin();
        let Some(document) = self.document(origin)? else {
            return Ok(Err(HandleRefusal::Gone(origin.clone())));
        };
        if handle.verify(&document).is_none() {
            return Ok(Err(HandleRefusal::Stale(origin.clone())));
        }
        let mut passages = Vec::new();
        for span in around(document.spans(), handle.chunk().span(), count) {
            // Every span here came from the document's own cut, so this mints;
            // the alternative to saying so is a panic, which is denied
            // crate-wide.
            let passage = Handle::mint(&document, span).ok_or_else(|| {
                io::Error::other("a span of the document is not one it was cut into")
            })?;
            crate::benchmark::record_neighbour_passage(passage.text().as_str().len());
            passages.push(passage);
        }
        Ok(Ok(passages))
    }
}

impl Store {
    /// Checks `handle` against the text held now, then answers with up to
    /// `count` of the passages around it, in reading order.
    ///
    /// The chunk `handle` names is not among them. An agent asking for
    /// neighbours is holding that text already, so returning it would be
    /// spending one of the `count` on something the agent has — and asking
    /// again from a passage that came back is how a fragment is widened
    /// further.
    ///
    /// `count` is how many passages come back, not how many on each side: it
    /// is the same word `search` takes, and one word with two referents is
    /// what the plan spends its rules compensating for. They are taken
    /// outward from the chunk, a step behind for every step ahead and the odd
    /// one behind, so what comes back is a run of the document with the
    /// chunk's own text in the middle of it. When one side runs out the other
    /// takes what is left, and when the document does, fewer come back than
    /// were asked for — a document holding one chunk has no neighbours, which
    /// is an answer about the document rather than a refusal.
    ///
    /// Adjacency is read off the offsets rather than off the cut, because
    /// `Handle::verify` does not consult the cut: a document written again
    /// with the same bytes and cut differently still verifies, and its chunks
    /// still have a before and an after. A chunk of the current cut that
    /// overlaps the handle's span without being it is neither, so it is passed
    /// over rather than handed back as surrounding context it is not.
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
    /// refusal rather than seated beside it, as [`open`](Store::open) and
    /// [`add`](Store::add) do.
    pub fn neighbours(
        &self,
        handle: &Handle,
        count: Count,
    ) -> io::Result<Result<Vec<Passage>, HandleRefusal>> {
        <Self as NeighboursVerb>::neighbours(self, handle, count)
    }
}

/// The `count` members of `spans` nearest `span` on either side of it, in
/// reading order.
///
/// `spans` partitions a text, so it is sorted and its members do not overlap:
/// the ones wholly before `span` and the ones wholly after it are each a run,
/// found by where `span` falls rather than by looking it up. A member that
/// overlaps `span` without being it belongs to neither run.
///
/// The two counts are arithmetic rather than a walk, and they are the same
/// answer: half the wanted behind, rounded up, then as much of the rest ahead
/// as there is, then back to fill from what is left behind.
fn around(spans: &[Span], span: Span, count: Count) -> Vec<Span> {
    let first = spans.partition_point(|held| held.end() <= span.start());
    let past = spans.partition_point(|held| held.start() < span.end());
    let preceding = spans.get(..first).unwrap_or_default();
    let following = spans.get(past..).unwrap_or_default();
    let wanted = count.get().get();
    let behind = preceding.len().min(wanted.div_ceil(2));
    let ahead = following.len().min(wanted - behind);
    let behind = preceding.len().min(wanted - ahead);
    preceding
        .get(preceding.len() - behind..)
        .unwrap_or_default()
        .iter()
        .chain(following.get(..ahead).unwrap_or_default())
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::handle::Handle;
    use crate::passage::Passage;
    use crate::refusal::HandleRefusal;
    use crate::scaffold::{count, document, filed, minted, model, store};
    use crate::store::Store;

    /// A store of this test's own, emptied first so a rerun starts where the
    /// last one did.
    fn path(test: &str) -> PathBuf {
        crate::scaffold::path(module_path!(), test)
    }

    fn widened(store: &Store, handle: &Handle, wanted: usize) -> Vec<Passage> {
        match store.neighbours(handle, count(wanted)) {
            Ok(Ok(passages)) => passages,
            Ok(Err(refusal)) => unreachable!("{refusal}"),
            Err(error) => unreachable!("{error}"),
        }
    }

    fn refused(store: &Store, handle: &Handle, wanted: usize) -> HandleRefusal {
        match store.neighbours(handle, count(wanted)) {
            Ok(Ok(_)) => unreachable!("the handle was not refused"),
            Ok(Err(refusal)) => refusal,
            Err(error) => unreachable!("{error}"),
        }
    }

    fn read(passages: &[Passage]) -> Vec<&str> {
        passages
            .iter()
            .map(|passage| passage.text().as_str())
            .collect()
    }

    /// Five chunks, each two bytes, so a position in the answer is legible.
    const BODY: &str = "aabbccddee";
    const CUTS: &[usize] = &[2, 4, 6, 8];

    /// A store holding one five-chunk document, and the handle for its middle
    /// chunk.
    fn middle(test: &str) -> (Store, Handle) {
        let model = model("test");
        let store = store(&path(test), &model);
        let document = document("a.md", BODY, CUTS);
        filed(&store, &model, &document);
        let handle = minted(&document, 2);
        (store, handle)
    }

    #[test]
    fn the_passages_around_a_chunk_come_back_in_reading_order() {
        let (store, handle) = middle("order");
        assert_eq!(read(&widened(&store, &handle, 4)), ["aa", "bb", "dd", "ee"]);
    }

    /// The projection says neighbours are the chunks adjacent to this one, so
    /// the chunk itself is not one of them — asked for more than the document
    /// holds, its own text still does not come back.
    #[test]
    fn the_chunk_itself_is_not_among_its_neighbours() {
        let (store, handle) = middle("itself");
        let passages = widened(&store, &handle, 100);
        assert_eq!(passages.len(), 4);
        assert!(!read(&passages).contains(&"cc"));
    }

    /// `count` is how many passages come back, not how many on each side. The
    /// odd one is taken behind, which is the tie the outward walk has to break.
    #[test]
    fn the_count_is_the_whole_answer_rather_than_one_side_of_it() {
        let (store, handle) = middle("count");
        assert_eq!(read(&widened(&store, &handle, 3)), ["aa", "bb", "dd"]);
    }

    #[test]
    fn a_chunk_at_the_edge_takes_what_is_left_from_the_other_side() {
        let model = model("test");
        let store = store(&path("edge"), &model);
        let document = document("a.md", BODY, CUTS);
        filed(&store, &model, &document);
        let handle = minted(&document, 0);
        assert_eq!(read(&widened(&store, &handle, 3)), ["bb", "cc", "dd"]);
    }

    /// A document holding one chunk has no neighbours, and that is an answer
    /// about the document rather than a refusal.
    #[test]
    fn a_document_of_one_chunk_has_nothing_around_it() {
        let model = model("test");
        let store = store(&path("alone"), &model);
        let document = document("a.md", "aa", &[]);
        filed(&store, &model, &document);
        let handle = minted(&document, 0);
        assert!(widened(&store, &handle, 3).is_empty());
    }

    /// Every passage handed back carries a handle minted from the text the
    /// store held during this read, so an agent can widen again from any of
    /// them.
    #[test]
    fn every_passage_returned_carries_a_handle_that_verifies() {
        let (store, handle) = middle("verifies");
        let document = document("a.md", BODY, CUTS);
        for passage in &widened(&store, &handle, 4) {
            assert_eq!(
                passage.handle().verify(&document),
                Some(passage.text().as_str())
            );
        }
    }

    /// The node's criterion. The chunk's text is still in the document — it
    /// has moved — and this refuses rather than handing back the two bytes now
    /// at that span and the chunks around them.
    #[test]
    fn a_handle_whose_text_has_moved_is_stale_rather_than_widened() {
        let model = model("test");
        let store = store(&path("stale"), &model);
        let original = document("a.md", BODY, CUTS);
        filed(&store, &model, &original);
        let handle = minted(&original, 2);
        let moved = document("a.md", "zzaabbccddee", &[2, 4, 6, 8, 10]);
        filed(&store, &model, &moved);
        assert_eq!(
            refused(&store, &handle, 4),
            HandleRefusal::Stale(handle.chunk().origin().clone())
        );
    }

    /// The other half of the pair: the document is not held at all, which is a
    /// different repair from text that disagrees, and so a different word.
    #[test]
    fn a_handle_into_a_deleted_document_is_gone_rather_than_stale() {
        let (store, handle) = middle("gone");
        let Ok(()) = store.delete(handle.chunk().origin()) else {
            unreachable!()
        };
        assert_eq!(
            refused(&store, &handle, 4),
            HandleRefusal::Gone(handle.chunk().origin().clone())
        );
    }

    /// A handle verifies against a document written again with the same bytes
    /// and cut differently, so this has to answer for one. Adjacency is read
    /// off the offsets: the chunk that overlaps the handle's span without
    /// being it is neither before nor after, and is passed over.
    #[test]
    fn a_document_cut_differently_still_has_a_before_and_an_after() {
        let model = model("test");
        let store = store(&path("recut"), &model);
        let original = document("a.md", BODY, CUTS);
        filed(&store, &model, &original);
        let handle = minted(&original, 2);
        let recut = document("a.md", BODY, &[2, 6, 8]);
        filed(&store, &model, &recut);
        assert_eq!(read(&widened(&store, &handle, 3)), ["aa", "dd", "ee"]);
    }
}
