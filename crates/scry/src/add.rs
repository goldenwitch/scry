//! `add`: the verb that fills the corpus.
//!
//! Nothing new is decided here. `fetch` says what bytes are, `slice` says
//! where the boundaries fall, `embed` says what a vector is, and `store` says
//! what one document per origin means — this is the order they run in, and the
//! order is the whole of it. A verb that reached past those parts to make its
//! own ruling would be a second place that ruling lived.

use std::collections::BTreeSet;
use std::io;
use std::time::Duration;

use crate::document::Document;
use crate::embed::Embed;
use crate::fetch::fetch;
use crate::origin::Origin;
use crate::refusal::AddRefusal;
use crate::slice::Slice;
use crate::store::Store;

/// The result of one set upsert, in canonical origin order.
#[derive(Debug)]
pub struct AddReport {
    items: Vec<AddResult>,
}

impl AddReport {
    /// The result for each canonical origin submitted to `add`.
    #[must_use]
    pub fn items(&self) -> &[AddResult] {
        &self.items
    }
}

/// The outcome for one canonical origin in an [`AddReport`].
#[derive(Debug)]
pub struct AddResult {
    origin: Origin,
    outcome: AddOutcome,
}

impl AddResult {
    /// The canonical origin this result belongs to.
    #[must_use]
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }

    /// What happened while upserting this origin.
    #[must_use]
    pub const fn outcome(&self) -> &AddOutcome {
        &self.outcome
    }
}

/// The result of processing one origin.
#[derive(Debug)]
pub enum AddOutcome {
    /// The replacement was committed successfully.
    Upserted,
    /// The origin was named, but its bytes were absent or not text.
    Refused(AddRefusal),
    /// Preparation failed before a write could make the replacement visible.
    Failed(io::Error),
    /// The store write phase failed, so visibility of the replacement is unknown.
    Uncertain(io::Error),
    /// Processing did not start because an earlier member failed.
    NotAttempted,
}

trait AddVerb {
    fn add(
        &self,
        embed: &mut Embed,
        slice: &Slice,
        origins: &BTreeSet<Origin>,
        ttl: Duration,
    ) -> AddReport;
}

impl AddVerb for Store {
    fn add(
        &self,
        embed: &mut Embed,
        slice: &Slice,
        origins: &BTreeSet<Origin>,
        ttl: Duration,
    ) -> AddReport {
        orchestrate(origins, |origin| {
            process_member(self, embed, slice, origin, ttl)
        })
    }
}

fn process_member(
    store: &Store,
    embed: &mut Embed,
    slice: &Slice,
    origin: &Origin,
    ttl: Duration,
) -> AddOutcome {
    let (text, fetched_at) = {
        let _stage = crate::benchmark::start_stage(crate::benchmark::Stage::Fetch);
        match fetch(origin) {
            Ok(read) => read,
            Err(refusal) => return AddOutcome::Refused(refusal),
        }
    };
    let sliced = {
        let _stage = crate::benchmark::start_stage(crate::benchmark::Stage::Slice);
        match slice.cut(text) {
            Ok(sliced) => sliced,
            Err(error) => return AddOutcome::Failed(error),
        }
    };
    let embeddings = {
        let _stage = crate::benchmark::start_stage(crate::benchmark::Stage::Embedding);
        match embed.passages_from_slice(&sliced) {
            Ok(embeddings) => embeddings,
            Err(error) => return AddOutcome::Failed(error),
        }
    };
    let (text, spans) = sliced.into_parts();
    let Some(document) = Document::new(origin.clone(), text, fetched_at, ttl, spans) else {
        return AddOutcome::Failed(io::Error::other(
            "the text was not cut into a partition of itself",
        ));
    };
    let prepared = {
        let _stage = crate::benchmark::start_stage(crate::benchmark::Stage::Record);
        match store.prepare_replace(&document, &embeddings) {
            Ok(prepared) => prepared,
            Err(error) => return AddOutcome::Failed(error),
        }
    };
    let commit = {
        let _stage = crate::benchmark::start_stage(crate::benchmark::Stage::Commit);
        store.commit_replace(&prepared)
    };
    match commit {
        Ok(()) => AddOutcome::Upserted,
        Err(error) => AddOutcome::Uncertain(error),
    }
}

fn orchestrate(
    origins: &BTreeSet<Origin>,
    mut process: impl FnMut(&Origin) -> AddOutcome,
) -> AddReport {
    let mut items = Vec::with_capacity(origins.len());
    let mut halted = false;
    for origin in origins {
        let outcome = if halted {
            AddOutcome::NotAttempted
        } else {
            process(origin)
        };
        crate::benchmark::record_add_member(match &outcome {
            AddOutcome::Upserted => "upserted",
            AddOutcome::Refused(_) => "refused",
            AddOutcome::Failed(_) => "failed",
            AddOutcome::Uncertain(_) => "uncertain",
            AddOutcome::NotAttempted => "not-attempted",
        });
        halted |= matches!(&outcome, AddOutcome::Failed(_) | AddOutcome::Uncertain(_));
        items.push(AddResult {
            origin: origin.clone(),
            outcome,
        });
    }
    AddReport { items }
}

impl Store {
    /// Reads each `origin`, cuts it, embeds it, and files it — replacing
    /// whatever was there under that origin.
    ///
    /// `ttl` is how long the material is said to stay good for. Every add
    /// supplies one and there is no default, so nobody is answered with an age
    /// scry invented.
    ///
    /// The seams arrive as arguments rather than being held here: a store
    /// recognises a model, it does not load one, and this is the verb that
    /// needs both.
    ///
    /// Nothing is written until the document and all of its embeddings are in
    /// hand, and then in one put — so a preparation failure leaves the
    /// document that was there untouched rather than half of a new one.
    ///
    /// # Errors
    ///
    /// Each member reports [`NotFound`](AddRefusal::NotFound) when no bytes
    /// arrive at its origin, or [`NotText`](AddRefusal::NotText) when the bytes
    /// there are not something scry will index. Those two are the whole of
    /// what `add` decides about source material. Preparation failures are
    /// [`AddOutcome::Failed`], write-phase failures are
    /// [`AddOutcome::Uncertain`], and later members after either are
    /// [`AddOutcome::NotAttempted`].
    ///
    /// Refusals continue the sequential operation. A preparation or
    /// persistence failure halts it after recording the failure and leaves
    /// earlier successful replacements committed.
    pub fn add(
        &self,
        embed: &mut Embed,
        slice: &Slice,
        origins: &BTreeSet<Origin>,
        ttl: Duration,
    ) -> AddReport {
        <Self as AddVerb>::add(self, embed, slice, origins, ttl)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeSet;
    use std::fs;
    use std::io;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::document::Document;
    use crate::embed::Embed;
    use crate::embedding::Embedding;
    use crate::origin::Origin;
    use crate::refusal::AddRefusal;
    use crate::scaffold::{TTL, held, seams};
    use crate::slice::Slice;
    use crate::store::Store;

    use super::{AddOutcome, AddReport, AddResult, AddVerb, orchestrate};

    struct CountingAdd<'store> {
        inner: &'store Store,
        calls: Cell<usize>,
    }

    impl AddVerb for CountingAdd<'_> {
        fn add(
            &self,
            embed: &mut Embed,
            slice: &Slice,
            origins: &BTreeSet<Origin>,
            ttl: Duration,
        ) -> AddReport {
            self.calls.set(self.calls.get() + 1);
            <Store as AddVerb>::add(self.inner, embed, slice, origins, ttl)
        }
    }

    /// A directory of this test's own, emptied first so a rerun starts where
    /// the last one did.
    fn directory(test: &str) -> PathBuf {
        crate::scaffold::directory(module_path!(), test)
    }

    /// The three parts `add` is wired from, over a store of this test's own.
    fn parts(test: &str) -> (Embed, Slice, Store, PathBuf) {
        let directory = directory(test);
        let (embed, slice) = seams();
        let store = match Store::open(&directory.join("corpus.redb"), embed.model()) {
            Ok(Ok(store)) => store,
            Ok(Err(end)) => unreachable!("{end}"),
            Err(error) => unreachable!("{error}"),
        };
        (embed, slice, store, directory)
    }

    /// Writes `body` at `name` in `directory` and returns the origin naming
    /// it, spelled as every verb that names a document spells it.
    fn written(directory: &Path, name: &str, body: &[u8]) -> Origin {
        let path = directory.join(name);
        let Ok(()) = fs::write(&path, body) else {
            unreachable!()
        };
        let Some(spelling) = path.to_str() else {
            unreachable!()
        };
        let Some(origin) = Origin::parse(spelling) else {
            unreachable!()
        };
        origin
    }

    fn added(store: &Store, embed: &mut Embed, slice: &Slice, origin: &Origin) {
        let origins = BTreeSet::from([origin.clone()]);
        let report = store.add(embed, slice, &origins, TTL);
        let Some(result) = report.items().first() else {
            unreachable!("the add report was empty")
        };
        match result.outcome() {
            AddOutcome::Upserted => (),
            outcome => unreachable!("{outcome:?}"),
        }
    }

    fn refused(store: &Store, embed: &mut Embed, slice: &Slice, origin: &Origin) -> AddRefusal {
        let origins = BTreeSet::from([origin.clone()]);
        let report = store.add(embed, slice, &origins, TTL);
        let Some(result) = report.items().first() else {
            unreachable!("the add report was empty")
        };
        match result.outcome() {
            AddOutcome::Refused(refusal) => refusal.clone(),
            outcome => unreachable!("{outcome:?}"),
        }
    }

    #[test]
    fn an_add_implementation_can_wrap_another() {
        let (mut embed, slice, store, directory) = parts("wrapped");
        let origin = written(&directory, "note.md", b"the wrapped document");
        let origins = BTreeSet::from([origin]);
        let wrapped = CountingAdd {
            inner: &store,
            calls: Cell::new(0),
        };

        let report = <CountingAdd<'_> as AddVerb>::add(&wrapped, &mut embed, &slice, &origins, TTL);
        assert_eq!(wrapped.calls.get(), 1);
        assert!(matches!(
            report.items().first().map(AddResult::outcome),
            Some(AddOutcome::Upserted)
        ));
    }

    /// Every document in the corpus, with the embeddings filed beside it.
    fn all(store: &Store) -> Vec<(Document, Vec<Embedding>)> {
        let mut all = Vec::new();
        let Ok(()) = store.scan(|document, embeddings| {
            all.push((document, embeddings));
            Ok(())
        }) else {
            unreachable!()
        };
        all
    }

    /// A text of `lines` repetitions, long enough to be cut into more than one
    /// chunk.
    fn long(lines: usize) -> String {
        let mut text = String::new();
        for _ in 0..lines {
            text.push_str("the kettle boiled and the room filled with steam\n");
        }
        text
    }

    #[test]
    fn a_path_becomes_a_document() {
        let (mut embed, slice, store, directory) = parts("a-path");
        let origin = written(&directory, "note.md", b"the kettle boiled");
        added(&store, &mut embed, &slice, &origin);
        let Some(document) = held(&store, &origin) else {
            unreachable!("nothing was filed")
        };
        assert_eq!(document.text().as_str(), "the kettle boiled");
        assert_eq!(document.origin(), &origin);
        assert_eq!(document.ttl(), TTL);
    }

    #[test]
    fn adding_the_same_origin_twice_leaves_one_document() {
        let (mut embed, slice, store, directory) = parts("twice");
        let origin = written(&directory, "note.md", b"the kettle boiled");
        added(&store, &mut embed, &slice, &origin);
        let second = written(&directory, "note.md", b"the room filled with steam");
        assert_eq!(second, origin);
        added(&store, &mut embed, &slice, &origin);
        let corpus = all(&store);
        assert_eq!(corpus.len(), 1);
        let Some((document, _)) = corpus.first() else {
            unreachable!()
        };
        assert_eq!(document.text().as_str(), "the room filled with steam");
    }

    #[test]
    fn every_span_is_embedded() {
        let (mut embed, slice, store, directory) = parts("embedded");
        let origin = written(&directory, "long.md", long(60).as_bytes());
        added(&store, &mut embed, &slice, &origin);
        let corpus = all(&store);
        let Some((document, embeddings)) = corpus.first() else {
            unreachable!("nothing was filed")
        };
        // More than one, so the batch path is what was measured and not the
        // single-chunk case wearing its clothes.
        assert!(document.spans().len() > 1);
        assert_eq!(embeddings.len(), document.spans().len());
    }

    #[test]
    fn an_origin_with_no_bytes_is_refused_and_changes_nothing() {
        let (mut embed, slice, store, directory) = parts("no-bytes");
        let origin = written(&directory, "note.md", b"the kettle boiled");
        added(&store, &mut embed, &slice, &origin);
        let Ok(()) = fs::remove_file(directory.join("note.md")) else {
            unreachable!()
        };
        assert_eq!(
            refused(&store, &mut embed, &slice, &origin),
            AddRefusal::NotFound(origin.clone())
        );
        let Some(document) = held(&store, &origin) else {
            unreachable!("the refusal emptied the corpus")
        };
        assert_eq!(document.text().as_str(), "the kettle boiled");
    }

    #[test]
    fn bytes_that_are_not_text_are_refused_and_file_nothing() {
        let (mut embed, slice, store, directory) = parts("not-text");
        let origin = written(&directory, "note.bin", &[0xff, 0xfe, 0x00]);
        assert_eq!(
            refused(&store, &mut embed, &slice, &origin),
            AddRefusal::NotText(origin.clone())
        );
        assert!(held(&store, &origin).is_none());
    }

    #[test]
    fn a_file_with_nothing_in_it_is_a_document_with_no_chunks() {
        let (mut embed, slice, store, directory) = parts("empty");
        let origin = written(&directory, "empty.md", b"");
        added(&store, &mut embed, &slice, &origin);
        let corpus = all(&store);
        let Some((document, embeddings)) = corpus.first() else {
            unreachable!("nothing was filed")
        };
        assert!(document.spans().is_empty());
        assert!(embeddings.is_empty());
    }

    fn origins(names: &[&str]) -> BTreeSet<Origin> {
        names
            .iter()
            .map(|name| crate::scaffold::origin(name))
            .collect()
    }

    #[test]
    fn an_empty_set_does_no_member_work() {
        let origins = BTreeSet::new();
        let mut invoked = false;
        let report = orchestrate(&origins, |_| {
            invoked = true;
            AddOutcome::Upserted
        });
        assert!(!invoked);
        assert!(report.items().is_empty());
    }

    #[test]
    fn a_refusal_does_not_stop_later_members() {
        let origins = origins(&["a.md", "b.md", "c.md"]);
        let mut attempted = 0;
        let report = orchestrate(&origins, |origin| {
            attempted += 1;
            if attempted == 1 {
                AddOutcome::Refused(AddRefusal::NotFound(origin.clone()))
            } else {
                AddOutcome::Upserted
            }
        });
        assert_eq!(attempted, origins.len());
        assert_eq!(report.items().len(), origins.len());
        assert!(matches!(
            report.items().first().map(AddResult::outcome),
            Some(AddOutcome::Refused(AddRefusal::NotFound(_)))
        ));
        assert!(
            report
                .items()
                .iter()
                .skip(1)
                .all(|item| matches!(item.outcome(), AddOutcome::Upserted))
        );
    }

    fn assert_halts_at(failure: AddOutcome) {
        let origins = origins(&["a.md", "b.md", "c.md"]);
        let expected = origins.iter().take(2).cloned().collect::<Vec<_>>();
        let mut attempted = Vec::new();
        let mut failure = Some(failure);
        let report = orchestrate(&origins, |origin| {
            attempted.push(origin.clone());
            if attempted.len() == 1 {
                AddOutcome::Upserted
            } else {
                let Some(failure) = failure.take() else {
                    unreachable!("the member after a halt was attempted")
                };
                failure
            }
        });
        assert_eq!(attempted, expected);
        assert_eq!(report.items().len(), origins.len());
        assert!(matches!(
            report.items().first().map(AddResult::outcome),
            Some(AddOutcome::Upserted)
        ));
        assert!(matches!(
            report.items().get(1).map(AddResult::outcome),
            Some(AddOutcome::Failed(_) | AddOutcome::Uncertain(_))
        ));
        assert!(
            report
                .items()
                .iter()
                .skip(2)
                .all(|item| matches!(item.outcome(), AddOutcome::NotAttempted))
        );
    }

    #[test]
    fn a_failed_member_halts_later_members() {
        assert_halts_at(AddOutcome::Failed(io::Error::other("processing failed")));
    }

    #[test]
    fn an_uncertain_member_halts_later_members() {
        assert_halts_at(AddOutcome::Uncertain(io::Error::other("write failed")));
    }

    #[test]
    fn a_store_write_failure_is_uncertain_and_halts_later_members() {
        let (mut embed, slice, store, directory) = parts("uncertain-write");
        let first = written(&directory, "first.md", b"the first document");
        let second = written(&directory, "second.md", b"the second document");
        let origins = BTreeSet::from([first, second]);
        store.fail_next_commit();

        let report = store.add(&mut embed, &slice, &origins, TTL);
        assert_eq!(report.items().len(), 2);
        assert!(matches!(
            report.items().first().map(AddResult::outcome),
            Some(AddOutcome::Uncertain(_))
        ));
        assert!(matches!(
            report.items().get(1).map(AddResult::outcome),
            Some(AddOutcome::NotAttempted)
        ));
    }
}
