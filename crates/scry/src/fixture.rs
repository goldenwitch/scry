//! `fixture`: a corpus whose nearest passage is known before anything runs.
//!
//! Search is an instrument, and an instrument that has never been checked
//! against a known answer reports confident structure from nothing. So the
//! answer comes first: six documents on six grossly separated subjects, and
//! one query apiece whose nearest passage was settled by reading rather than
//! by running. Absent this, a ranking bug arrives looking like a model being
//! mediocre, and there is nothing to tell the two apart.
//!
//! Nothing here was measured with scry. An expectation recorded from a search
//! agrees with that search by construction, so it can only ever confirm the
//! ranker to itself — which is the failure this part exists to prevent. The
//! table below is therefore an argument a reader can disagree with, not an
//! output, and the price of that is that it can be wrong: if search disagrees
//! with a row, the row is the first thing to doubt and a human settles it.
//!
//! # The derivation
//!
//! Each query names a subject exactly one document is about, and the other
//! five are about subjects nothing in the query touches. The deciding words
//! are the ones the query and its document share; the column beside them is
//! the only other document with any claim at all.
//!
//! | query | nearest | deciding words | nearest competitor |
//! | --- | --- | --- | --- |
//! | keeping a bread starter going before baking | `sourdough` | starter, bake, bread | none — no other document is about food |
//! | molten rock bursting out of a mountain | `volcano` | magma, lava, slopes, summit | none — no other document is about the ground |
//! | a queen keeping her colony fed through winter | `beehive` | queen, colony, winter | `chess`, which is the only other document holding `queen` |
//! | first moves fighting for the centre of the board | `chess` | opening, moves, centre | none — no other document is about a game |
//! | the wood a stringed instrument's body is carved from | `violin` | carved, spruce, maple, strings | none — no other document is about an instrument |
//! | burning coal turning wheels on a railway | `locomotive` | coal, burns, wheels, steam | none — no other document is about a machine |
//!
//! The third row is the only one with a word planted in two documents, and it
//! is there so a ranker that had quietly fallen back to matching words is
//! caught rather than passed. It is derived the same way as the others: the
//! query is about a colony and a winter, and one document is about those.
//!
//! # What it does not check
//!
//! Nearest-neighbour under bge-small is derivable by hand only for subjects
//! this far apart, so this fixture measures coarse topical separation and
//! nothing finer. A ranking bug that keeps the topic clusters intact — one
//! that scrambles the order within a subject, or that returns the right
//! document's wrong chunk — passes every row above. Sharper claims need a
//! measurement rather than a derivation, and this file is not where they go.

use crate::origin::Origin;
use crate::query::Query;
use crate::text::Text;

/// The most characters a fixture document holds.
///
/// `slice` cuts every 254 tokens of text — a window of 256, less the two the
/// tokenizer adds — and a word-piece token consumes at least one character, so
/// a text shorter than this cannot reach the cut. Every document here is
/// therefore one chunk, which is what lets a row above name a document where
/// what is returned is a passage.
///
/// The number is a bound the fixture holds itself to, not a copy of the
/// window: it stays true while the budget only grows. A budget that shrank
/// below it would leave a document in two pieces, and this bound is where to
/// come.
const BOUND: usize = 254;

/// A document of the fixture: where it is filed and what is there.
///
/// This is deliberately not a [`Document`](crate::Document): a document is cut
/// into spans, and cutting is `slice`'s to do. What a fixture knows is the
/// origin and the text, which is what `add` has after it fetches.
pub(crate) struct Entry {
    origin: &'static str,
    text: &'static str,
}

impl Entry {
    /// Its identity, spelled as every verb that names a document spells it.
    pub(crate) fn origin(&self) -> Origin {
        let Some(origin) = Origin::parse(self.origin) else {
            unreachable!("a fixture origin does not parse")
        };
        origin
    }

    /// What is at it.
    pub(crate) fn text(&self) -> Text {
        Text::from(self.text.to_owned())
    }
}

/// A query, and the document whose passage is nearest it.
///
/// The claim is strict: that document is nearer than any other, so a caller
/// may ask for one hit and check identity, or ask for several and check
/// membership. Membership is the weaker reading and the one search is judged
/// by, since an order nobody derived by hand is a recorded output.
pub(crate) struct Expectation {
    query: &'static str,
    nearest: &'static str,
}

impl Expectation {
    /// What is searched with.
    pub(crate) fn query(&self) -> Query {
        Query::new(Text::from(self.query.to_owned()))
    }

    /// The origin of the document whose passage answers it.
    pub(crate) fn nearest(&self) -> Origin {
        let Some(origin) = Origin::parse(self.nearest) else {
            unreachable!("a fixture expectation names an origin that does not parse")
        };
        origin
    }
}

/// The corpus.
///
/// The origins are urls under the reserved `.invalid` domain, so no test can
/// reach one by accident and none of them is a file whose contents could drift
/// from what is written here. Nothing fetches them: a caller slices and embeds
/// this text and puts it in a store, which is the corpus `add` would have
/// left.
pub(crate) const CORPUS: [Entry; 6] = [
    Entry {
        origin: "https://fixture.invalid/sourdough",
        text: "A sourdough starter is flour and water left to sour until wild yeast and lactic bacteria live in it. Feed it daily, let the dough rise slowly, and bake it in a hot oven for an open crumb and a dark crust.",
    },
    Entry {
        origin: "https://fixture.invalid/volcano",
        text: "Magma rises through cracks in the crust until the pressure above it gives way. The eruption throws ash and lava down the slopes, and when the chamber below empties the summit can fall in and leave a caldera.",
    },
    Entry {
        origin: "https://fixture.invalid/beehive",
        text: "A hive holds one queen, a few hundred drones and tens of thousands of workers. Foragers gather nectar and pollen, the colony fans the nectar down to honey, and every wax cell is capped for the winter.",
    },
    Entry {
        origin: "https://fixture.invalid/chess",
        text: "The opening is the first dozen moves, where each side fights for the centre, develops knights and bishops before the queen, and hurries the king to safety behind castled rooks.",
    },
    Entry {
        origin: "https://fixture.invalid/violin",
        text: "A violin's belly is carved from spruce and its back from maple, and the plates are thinned until they ring true. A soundpost, a bridge and four strings tuned in fifths finish the instrument.",
    },
    Entry {
        origin: "https://fixture.invalid/locomotive",
        text: "Coal burns in the firebox, water boils in the barrel above it, and the steam drives pistons that turn the driving wheels. The exhaust is blasted up the chimney, which drags the fire hotter as the train climbs.",
    },
];

/// The answers, derived from the corpus by reading it.
///
/// One per document, so every document is the answer to exactly one query and
/// no document sits in the corpus unclaimed. A query nothing is expected to
/// answer would be a row nobody could be wrong about.
pub(crate) const EXPECTATIONS: [Expectation; 6] = [
    Expectation {
        query: "how do I keep a bread starter going before I bake?",
        nearest: "https://fixture.invalid/sourdough",
    },
    Expectation {
        query: "what makes molten rock burst out of a mountain?",
        nearest: "https://fixture.invalid/volcano",
    },
    Expectation {
        query: "how does a queen keep her colony fed through the winter?",
        nearest: "https://fixture.invalid/beehive",
    },
    Expectation {
        query: "which first moves fight for the centre of the board?",
        nearest: "https://fixture.invalid/chess",
    },
    Expectation {
        query: "what wood is a stringed instrument's body carved from?",
        nearest: "https://fixture.invalid/violin",
    },
    Expectation {
        query: "how does burning coal turn wheels on a railway?",
        nearest: "https://fixture.invalid/locomotive",
    },
];

#[cfg(test)]
mod tests {
    use super::{BOUND, CORPUS, EXPECTATIONS, Entry};
    use crate::origin::Origin;

    /// The corpus is six documents that are six documents: a repeated origin
    /// would be one document replacing another, so a row of the table would
    /// name a passage that is not there.
    #[test]
    fn every_origin_is_its_own() {
        let origins: Vec<Origin> = CORPUS.iter().map(Entry::origin).collect();
        for (at, origin) in origins.iter().enumerate() {
            assert!(
                !origins.iter().skip(at + 1).any(|other| other == origin),
                "two fixture documents share an origin"
            );
        }
    }

    /// Every document is one chunk, so what search returns for a row is the
    /// whole of the text written here rather than a piece of it nobody chose.
    #[test]
    fn every_document_is_one_chunk() {
        for entry in &CORPUS {
            let text = entry.text();
            assert!(!text.is_empty(), "a fixture document holds no text");
            assert!(
                text.as_str().chars().count() <= BOUND,
                "a fixture document is long enough to be cut in two"
            );
        }
    }

    /// Each document answers exactly one query, and each query is answered by
    /// a document that is in the corpus. An expectation naming an origin
    /// nothing was filed under would pass search by never being found.
    #[test]
    fn the_answers_and_the_corpus_are_one_to_one() {
        for entry in &CORPUS {
            let origin = entry.origin();
            let answered = EXPECTATIONS
                .iter()
                .filter(|expectation| expectation.nearest() == origin)
                .count();
            assert_eq!(answered, 1, "a fixture document does not answer one query");
        }
        assert_eq!(
            EXPECTATIONS.len(),
            CORPUS.len(),
            "there is a query no fixture document answers"
        );
    }

    /// The queries are six questions, and each is text a search could be run
    /// with. Two rows spelled the same would be one claim counted twice.
    #[test]
    fn every_query_is_its_own() {
        let queries: Vec<String> = EXPECTATIONS
            .iter()
            .map(|expectation| expectation.query().text().as_str().to_owned())
            .collect();
        for (at, query) in queries.iter().enumerate() {
            assert!(!query.is_empty(), "a fixture query is empty");
            assert!(
                !queries.iter().skip(at + 1).any(|other| other == query),
                "two fixture rows ask the same question"
            );
        }
    }
}
