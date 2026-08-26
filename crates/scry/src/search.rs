//! `search`: the verb that answers a query with passages.
//!
//! Nothing new is decided here. `embed` says what a vector is, `score` says
//! what a number that ranks a hit is, `store` says what the corpus holds, and
//! `handle` says what vouches for a passage — this is the order they run in,
//! and the order is the whole of it. The one thing this file owns is which
//! chunks are kept, and it keeps them by the only comparison scry has.

use std::io;

use crate::count::Count;
use crate::embed::Embed;
use crate::handle::Handle;
use crate::hit::Hit;
use crate::query::Query;
use crate::score::Score;
use crate::store::Store;

impl Store {
    /// Embeds `query`, scans the corpus, and answers with the best `count`
    /// hits, best first.
    ///
    /// A hit is a passage and the score the query gave it, so every answer
    /// arrives with the handle that vouches for its text — minted from the
    /// text the store held during this scan, and verifiable against the text
    /// there now.
    ///
    /// Fewer than `count` hits come back when the corpus holds fewer chunks
    /// than that, and none come back from a corpus holding nothing. Neither is
    /// a refusal: the vocabulary gives `search` no failure, because a corpus
    /// with nothing near a query is an answer about the corpus rather than a
    /// decision about material an agent named.
    ///
    /// The order is descending by score. It is what selecting the best `count`
    /// already computed, so handing back a permutation of it would be
    /// withholding a fact; but the order within those hits is not something
    /// anyone derived by hand, so it is a recorded output and not a claim.
    ///
    /// # Errors
    ///
    /// The disk and the inference session, which are weather. And a query
    /// embedded by a model other than the one the store was opened with, which
    /// is a mistake rather than a decision — the same reading `store` gives a
    /// caller that hands it a corpus it did not open for.
    pub fn search(&self, embed: &mut Embed, query: &Query, count: Count) -> io::Result<Vec<Hit>> {
        let asked = embed.query(query)?;
        let wanted = count.get().get();
        // A count is a number a caller named, so it never sizes an allocation:
        // the vector grows to what the corpus actually held.
        let mut hits: Vec<Hit> = Vec::new();
        self.scan(|document, embeddings| {
            for (span, embedding) in document.spans().iter().zip(&embeddings) {
                // Everything the store hands back names the model it was
                // opened with, so this is the query's model or nothing. The
                // comparison is made here, where it is already being made,
                // rather than once more before the scan.
                let score = Score::cosine(&asked, embedding).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "the query was embedded by another model than the store was opened with",
                    )
                })?;
                if !admits(&hits, wanted, score) {
                    continue;
                }
                // Every span here is one the document was cut into, so this
                // mints; the alternative to saying so is a panic, which is
                // denied crate-wide.
                let passage = Handle::mint(&document, *span).ok_or_else(|| {
                    io::Error::other("a span of the document is not one it was cut into")
                })?;
                let at =
                    hits.partition_point(|held| held.score().get().total_cmp(&score.get()).is_ge());
                hits.insert(at, Hit::new(passage, score));
                hits.truncate(wanted);
            }
            Ok(())
        })?;
        Ok(hits)
    }
}

/// Whether `score` belongs among the `wanted` best, given the `hits` kept so
/// far in descending order.
///
/// Asked before minting rather than after, so the corpus is not hashed and
/// copied to discard it: a chunk that cannot reach the answer is compared and
/// nothing more.
fn admits(hits: &[Hit], wanted: usize, score: Score) -> bool {
    hits.len() < wanted
        || hits
            .last()
            .is_some_and(|worst| score.get().total_cmp(&worst.score().get()).is_gt())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::SystemTime;

    use crate::document::Document;
    use crate::embed::Embed;
    use crate::fixture::{CORPUS, EXPECTATIONS};
    use crate::hit::Hit;
    use crate::query::Query;
    use crate::scaffold::{TTL, count, seams};
    use crate::slice::Slice;
    use crate::store::Store;

    /// A store of this test's own, emptied first so a rerun starts where the
    /// last one did.
    fn path(test: &str) -> PathBuf {
        crate::scaffold::path(module_path!(), test)
    }

    /// The seams and an empty store, all three agreeing on one model.
    fn parts(test: &str) -> (Embed, Slice, Store) {
        let (embed, slice) = seams();
        let store = match Store::open(&path(test), embed.model()) {
            Ok(Ok(store)) => store,
            Ok(Err(end)) => unreachable!("{end}"),
            Err(error) => unreachable!("{error}"),
        };
        (embed, slice, store)
    }

    /// A store holding the fixture corpus.
    ///
    /// The text is sliced, embedded and written through `replace`, which is
    /// the corpus `add` would have left had these origins been fetchable —
    /// they are `.invalid` urls precisely so that nothing here reaches the
    /// network.
    fn corpus(test: &str) -> (Embed, Store) {
        let (mut embed, slice, store) = parts(test);
        for entry in &CORPUS {
            let body = entry.text();
            let Ok(spans) = slice.spans(&body) else {
                unreachable!()
            };
            let Some(document) =
                Document::new(entry.origin(), body, SystemTime::UNIX_EPOCH, TTL, spans)
            else {
                unreachable!()
            };
            let Some(passages) = document
                .spans()
                .iter()
                .map(|span| document.text().at(*span))
                .collect::<Option<Vec<&str>>>()
            else {
                unreachable!()
            };
            let Ok(embeddings) = embed.passages(&passages) else {
                unreachable!()
            };
            let Ok(()) = store.replace(&document, &embeddings) else {
                unreachable!()
            };
        }
        (embed, store)
    }

    fn searched(store: &Store, embed: &mut Embed, query: &Query, wanted: usize) -> Vec<Hit> {
        match store.search(embed, query, count(wanted)) {
            Ok(hits) => hits,
            Err(error) => unreachable!("{error}"),
        }
    }

    /// How many hits a row is judged over. The fixture's claim is that its
    /// document is nearer than any other, which would let this be one; three
    /// is the weaker reading, and it is the one this verb is judged by,
    /// because an order nobody derived by hand is a recorded output.
    const JUDGED: usize = 3;

    /// The known answer. Each fixture query's document is among what comes
    /// back, so the instrument agrees with an expectation derived without it.
    #[test]
    fn the_expected_passage_is_among_the_hits() {
        let (mut embed, store) = corpus("expected");
        for expectation in &EXPECTATIONS {
            let hits = searched(&store, &mut embed, &expectation.query(), JUDGED);
            let nearest = expectation.nearest();
            assert!(
                hits.iter()
                    .any(|hit| hit.passage().handle().chunk().origin() == &nearest),
                "the document derived as nearest is not among the hits for its query"
            );
        }
    }

    /// Every handle handed out was minted from text the store held, so it
    /// vouches for that text the moment it arrives.
    #[test]
    fn every_returned_handle_verifies() {
        let (mut embed, store) = corpus("verifies");
        for expectation in &EXPECTATIONS {
            let hits = searched(&store, &mut embed, &expectation.query(), JUDGED);
            assert!(!hits.is_empty(), "a fixture query found nothing");
            for hit in &hits {
                let handle = hit.passage().handle();
                let Ok(Some(document)) = store.document(handle.chunk().origin()) else {
                    unreachable!()
                };
                assert_eq!(
                    handle.verify(&document),
                    Some(hit.passage().text().as_str()),
                    "a handle does not vouch for the text it came back with"
                );
            }
        }
    }

    /// The count is what is wanted, not what is available: asking for one
    /// gives one, and asking for more than the corpus holds gives what it
    /// holds. Every fixture document is one chunk, so that is six.
    #[test]
    fn the_count_is_honoured_and_the_corpus_bounds_it() {
        let (mut embed, store) = corpus("count");
        let Some(first) = EXPECTATIONS.first() else {
            unreachable!()
        };
        let one = searched(&store, &mut embed, &first.query(), 1);
        let all = searched(&store, &mut embed, &first.query(), 100);
        assert_eq!(one.len(), 1);
        assert_eq!(all.len(), CORPUS.len());
    }

    /// The hits are ordered by their own scores. This claims nothing about
    /// which document ranks where — only that what came back is sorted by the
    /// number each hit carries.
    #[test]
    fn the_hits_come_back_best_first() {
        let (mut embed, store) = corpus("order");
        let Some(first) = EXPECTATIONS.first() else {
            unreachable!()
        };
        let hits = searched(&store, &mut embed, &first.query(), 100);
        assert!(
            hits.is_sorted_by(|earlier, later| earlier.score().get() >= later.score().get()),
            "the hits are not in descending order of score"
        );
    }

    /// A corpus holding nothing answers with nothing. That is an answer, not a
    /// refusal — `search` has none.
    #[test]
    fn an_empty_corpus_answers_with_no_hits() {
        let (mut embed, _slice, store) = parts("empty");
        let Some(first) = EXPECTATIONS.first() else {
            unreachable!()
        };
        let hits = searched(&store, &mut embed, &first.query(), JUDGED);
        assert!(hits.is_empty());
    }
}
