//! The suite over every verb.
//!
//! Each part of scry is already tested beside itself, in its own file, against
//! material built by hand: a document reaches the store through `replace`, and
//! a handle is minted over text no `add` ever read. Nothing there is a caller,
//! and nothing there crosses more than two parts.
//!
//! This is the caller. It sees the crate the way a dependency sees it, so the
//! public API is measured for sufficiency as well as for behaviour, and every
//! claim here is about the composition rather than about a part: a document
//! fetched from a real file, cut, embedded and filed; found by a query; traced
//! back to the bytes it was cut from; and widened into the text beside it.
//!
//! The refusals are provoked through material an agent named rather than
//! through a record assembled to hold them — a path with nothing at it, bytes
//! that are not text, a file edited under a handle already handed out, and a
//! document deleted after one was. `Incompatible` is provoked the same way: a
//! file at a store's path that this build did not write.
//!
//! Nothing here reaches past `Store::open` and the five verbs. The walk back
//! to a passage's bytes is made the way the design says an agent makes it —
//! through the `origin` and into the world — rather than by reading the
//! document out of scry's own file, and what the corpus holds is read by
//! asking `search` rather than by being handed a list.

use core::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime};

use scry::{
    AddRefusal, Count, Embed, Handle, HandleRefusal, Hit, Model, Origin, Passage, Provenance,
    Query, Slice, Store, Text,
};

/// How long this suite's material is said to stay good for. Only the test
/// that judges an age reads it back.
const TTL: Duration = Duration::from_secs(3600);

/// One subject, and another far from it.
///
/// Two documents this far apart is coarse separation, which is the most the
/// crate's own fixture claims a hand-derived answer can carry. What is
/// measured here is that `add`, the store and `search` preserve that
/// separation end to end — not that the ranker is good, which is the
/// fixture's instrument and not this one's.
const POOLS: &str = "The tide pool holds anemones and limpets after the sea draws back, and the shallow water warms until the flood returns.";

/// The other subject. It shares no word with the query below that carries any
/// meaning.
const ENGINES: &str = "The locomotive raises boiler pressure until the safety valve lifts, and the fireman shovels coal against the long gradient.";

/// A query whose answer is the pools document by eye.
const ASKING: &str = "what lives in a rock pool while the tide is out";

/// The two seams a caller holds.
struct Seams {
    embed: Embed,
    slice: Slice,
}

/// One model for the whole binary, behind a lock.
///
/// Loading it costs a hundred and thirty megabytes of session per copy, and
/// `add` and `search` need it mutably, so the suite shares one rather than
/// standing up a session per test. Tests therefore run one at a time through
/// the seams and concurrently everywhere else.
static SEAMS: OnceLock<Mutex<Seams>> = OnceLock::new();

/// The seams, loaded on first use.
///
/// A poisoned lock is stepped over rather than propagated: the model is
/// unchanged by a test that panicked while holding it, and refusing it here
/// would report one failure as every failure.
fn seams() -> MutexGuard<'static, Seams> {
    let seams = SEAMS.get_or_init(|| {
        let embed = match Embed::load(&cache()) {
            Ok(embed) => embed,
            Err(error) => unreachable!("{error}"),
        };
        let slice = match Slice::new(&embed) {
            Ok(slice) => slice,
            Err(error) => unreachable!("{error}"),
        };
        Mutex::new(Seams { embed, slice })
    });
    match seams.lock() {
        Ok(seams) => seams,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Where the weights are kept: the cache the crate's own tests fill, so a
/// machine that has run them once fetches nothing here.
///
/// A caller names this, as the embed seam takes it as an argument. One beside
/// each store would fetch the model once per test, which is the price that
/// decided it.
fn cache() -> PathBuf {
    std::env::temp_dir().join("scry-model-cache")
}

/// A directory of this test's own, emptied first so a rerun starts where the
/// last one did.
fn directory(test: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("scry-suite-{test}"));
    let _ = fs::remove_dir_all(&directory);
    let Ok(()) = fs::create_dir_all(&directory) else {
        unreachable!()
    };
    directory
}

/// The corpus in `directory`, opened for `model` as a caller opens it.
fn opened(directory: &Path, model: &Model) -> Store {
    match Store::open(&directory.join("corpus.redb"), model) {
        Ok(Ok(store)) => store,
        Ok(Err(end)) => unreachable!("{end}"),
        Err(error) => unreachable!("{error}"),
    }
}

/// Writes `body` at `name` in `directory` and returns the origin naming it.
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

/// `sentence`, numbered, until there is enough of it to be cut into several
/// chunks. The number is what makes one chunk's text tell itself from the
/// next one's.
fn long(sentence: &str) -> String {
    (1..=40).fold(String::new(), |mut text, paragraph| {
        // Writing to a string is the one write that cannot fail.
        let Ok(()) = writeln!(text, "Paragraph {paragraph}. {sentence}") else {
            unreachable!()
        };
        text
    })
}

fn added(store: &Store, seams: &mut Seams, origin: &Origin) {
    match store.add(&mut seams.embed, &seams.slice, origin, TTL) {
        Ok(Ok(())) => (),
        Ok(Err(refusal)) => unreachable!("{refusal}"),
        Err(error) => unreachable!("{error}"),
    }
}

fn not_added(store: &Store, seams: &mut Seams, origin: &Origin) -> AddRefusal {
    match store.add(&mut seams.embed, &seams.slice, origin, TTL) {
        Ok(Ok(())) => unreachable!("the origin was added"),
        Ok(Err(refusal)) => refusal,
        Err(error) => unreachable!("{error}"),
    }
}

fn searched(store: &Store, seams: &mut Seams, asking: &str, count: usize) -> Vec<Hit> {
    let query = Query::new(Text::from(asking.to_owned()));
    let Some(count) = Count::new(count) else {
        unreachable!()
    };
    match store.search(&mut seams.embed, &query, count) {
        Ok(hits) => hits,
        Err(error) => unreachable!("{error}"),
    }
}

fn traced(store: &Store, handle: &Handle) -> Provenance {
    match store.provenance(handle) {
        Ok(Ok(provenance)) => provenance,
        Ok(Err(refusal)) => unreachable!("{refusal}"),
        Err(error) => unreachable!("{error}"),
    }
}

fn not_traced(store: &Store, handle: &Handle) -> HandleRefusal {
    match store.provenance(handle) {
        Ok(Ok(provenance)) => unreachable!("the handle traced to {:?}", provenance.chunk()),
        Ok(Err(refusal)) => refusal,
        Err(error) => unreachable!("{error}"),
    }
}

fn widened(store: &Store, handle: &Handle, count: usize) -> Vec<Passage> {
    let Some(count) = Count::new(count) else {
        unreachable!()
    };
    match store.neighbours(handle, count) {
        Ok(Ok(passages)) => passages,
        Ok(Err(refusal)) => unreachable!("{refusal}"),
        Err(error) => unreachable!("{error}"),
    }
}

fn not_widened(store: &Store, handle: &Handle, count: usize) -> HandleRefusal {
    let Some(count) = Count::new(count) else {
        unreachable!()
    };
    match store.neighbours(handle, count) {
        Ok(Ok(passages)) => unreachable!("the handle widened into {} passages", passages.len()),
        Ok(Err(refusal)) => refusal,
        Err(error) => unreachable!("{error}"),
    }
}

fn deleted(store: &Store, origin: &Origin) {
    let Ok(()) = store.delete(origin) else {
        unreachable!()
    };
}

/// The text at `provenance`, read from the bytes its origin names — the walk
/// an agent makes back.
///
/// It runs through the origin and into the world rather than through scry's
/// own file, which is the claim provenance makes: the span still names where
/// those bytes are.
fn text_at(provenance: &Provenance) -> String {
    let Some(path) = provenance.chunk().origin().as_path() else {
        unreachable!("the origin is not a path")
    };
    let Ok(text) = fs::read_to_string(path) else {
        unreachable!("there are no bytes at the origin")
    };
    let span = provenance.chunk().span();
    let Some(text) = text.get(span.start()..span.end()) else {
        unreachable!("the span does not read")
    };
    text.to_owned()
}

/// The best hit for `asking` over a corpus holding both subjects.
fn best(store: &Store, seams: &mut Seams) -> Hit {
    let hits = searched(store, seams, ASKING, 3);
    let Some(best) = hits.into_iter().next() else {
        unreachable!("the corpus answered with nothing")
    };
    best
}

/// Both documents, filed as `add` leaves them.
fn corpus(directory: &Path, store: &Store, seams: &mut Seams) -> (Origin, Origin) {
    let pools = written(directory, "pools.md", long(POOLS).as_bytes());
    let engines = written(directory, "engines.md", long(ENGINES).as_bytes());
    added(store, seams, &pools);
    added(store, seams, &engines);
    (pools, engines)
}

/// Job 1 and job 3 in one pass: a document that was fetched, cut, embedded and
/// filed is found by a query, and the handle that comes back with it names
/// exactly the bytes the passage holds.
#[test]
fn a_hit_traces_back_to_the_bytes_it_was_cut_from() {
    let mut seams = seams();
    let directory = directory("traced");
    let store = opened(&directory, seams.embed.model());
    let (pools, _) = corpus(&directory, &store, &mut seams);
    let best = best(&store, &mut seams);
    assert_eq!(best.passage().handle().chunk().origin(), &pools);
    let provenance = traced(&store, best.passage().handle());
    assert_eq!(provenance.chunk(), best.passage().handle().chunk());
    assert_eq!(text_at(&provenance), best.passage().text().as_str());
    assert_eq!(provenance.ttl(), TTL);
}

/// Every handle handed out is one the corpus will answer for.
#[test]
fn every_hit_traces_and_every_neighbour_traces() {
    let mut seams = seams();
    let directory = directory("all-trace");
    let store = opened(&directory, seams.embed.model());
    corpus(&directory, &store, &mut seams);
    let hits = searched(&store, &mut seams, ASKING, 3);
    assert_eq!(hits.len(), 3);
    for hit in &hits {
        let provenance = traced(&store, hit.passage().handle());
        assert_eq!(text_at(&provenance), hit.passage().text().as_str());
        for passage in widened(&store, hit.passage().handle(), 2) {
            let provenance = traced(&store, passage.handle());
            assert_eq!(text_at(&provenance), passage.text().as_str());
        }
    }
}

/// The fourth job the vocabulary declines to make a job: a fragment widens
/// into the text beside it, and the fragment is not among what comes back.
#[test]
fn a_hit_widens_into_the_text_beside_it() {
    let mut seams = seams();
    let directory = directory("widened");
    let store = opened(&directory, seams.embed.model());
    let (pools, _) = corpus(&directory, &store, &mut seams);
    let best = best(&store, &mut seams);
    let passages = widened(&store, best.passage().handle(), 2);
    // Two came back and neither is the fragment, so the document was cut into
    // at least three — which is what makes widening mean anything.
    assert_eq!(passages.len(), 2);
    let text = long(POOLS);
    for passage in &passages {
        assert_ne!(passage.handle(), best.passage().handle());
        assert_eq!(passage.handle().chunk().origin(), &pools);
        assert!(text.contains(passage.text().as_str()));
    }
}

/// `Stale`, provoked by the case the design exists for: the file changed after
/// the handle was handed out.
///
/// The edit keeps the byte count, so the span still reads and the digest is
/// what disagrees. A shorter file would have staled the handle by moving the
/// offsets, which is the accident rather than the claim.
#[test]
fn a_handle_is_stale_when_the_file_changed_under_it() {
    let mut seams = seams();
    let directory = directory("stale");
    let store = opened(&directory, seams.embed.model());
    let (pools, _) = corpus(&directory, &store, &mut seams);
    let best = best(&store, &mut seams);
    let edited = long(POOLS).replace("warms", "cools");
    assert_eq!(edited.len(), long(POOLS).len());
    let again = written(&directory, "pools.md", edited.as_bytes());
    assert_eq!(again, pools);
    added(&store, &mut seams, &pools);
    let handle = best.passage().handle();
    assert_eq!(
        not_traced(&store, handle),
        HandleRefusal::Stale(pools.clone())
    );
    assert_eq!(not_widened(&store, handle, 2), HandleRefusal::Stale(pools));
}

/// `Gone`, provoked by a document that was added and then deleted, and
/// deleting it again is still not an error.
#[test]
fn a_handle_is_gone_when_the_document_was_deleted() {
    let mut seams = seams();
    let directory = directory("gone");
    let store = opened(&directory, seams.embed.model());
    let (pools, engines) = corpus(&directory, &store, &mut seams);
    let best = best(&store, &mut seams);
    deleted(&store, &pools);
    deleted(&store, &pools);
    let handle = best.passage().handle();
    assert_eq!(
        not_traced(&store, handle),
        HandleRefusal::Gone(pools.clone())
    );
    assert_eq!(not_widened(&store, handle, 2), HandleRefusal::Gone(pools));
    // What was not deleted is still there to be found.
    let hits = searched(&store, &mut seams, ASKING, 3);
    assert!(!hits.is_empty());
    for hit in &hits {
        assert_eq!(hit.passage().handle().chunk().origin(), &engines);
    }
}

/// `NotFound`: an origin with no bytes at it.
#[test]
fn an_origin_with_no_bytes_is_refused() {
    let mut seams = seams();
    let directory = directory("not-found");
    let store = opened(&directory, seams.embed.model());
    let Some(spelling) = directory.join("nothing.md").to_str().map(str::to_owned) else {
        unreachable!()
    };
    let Some(origin) = Origin::parse(&spelling) else {
        unreachable!()
    };
    assert_eq!(
        not_added(&store, &mut seams, &origin),
        AddRefusal::NotFound(origin)
    );
    // Nothing was filed, read the only way the corpus is read.
    assert!(searched(&store, &mut seams, ASKING, 3).is_empty());
}

/// `NotText`: bytes arrived, and they are not something scry will index.
#[test]
fn bytes_that_are_not_text_are_refused() {
    let mut seams = seams();
    let directory = directory("not-text");
    let store = opened(&directory, seams.embed.model());
    let origin = written(&directory, "picture.bin", &[0xff, 0xfe, 0x00, 0x80]);
    assert_eq!(
        not_added(&store, &mut seams, &origin),
        AddRefusal::NotText(origin)
    );
    // Nothing was filed, read the only way the corpus is read.
    assert!(searched(&store, &mut seams, ASKING, 3).is_empty());
}

/// The corpus is state, so it survives the store that wrote it — and a handle
/// minted before the close still traces after the reopen, which is what makes
/// a handle worth handing to an agent that will come back later.
#[test]
fn a_handle_traces_after_the_store_is_closed_and_opened_again() {
    let mut seams = seams();
    let directory = directory("reopened");
    let store = opened(&directory, seams.embed.model());
    let (pools, _) = corpus(&directory, &store, &mut seams);
    let best = best(&store, &mut seams);
    drop(store);
    let store = opened(&directory, seams.embed.model());
    let provenance = traced(&store, best.passage().handle());
    assert_eq!(provenance.chunk().origin(), &pools);
    assert_eq!(text_at(&provenance), best.passage().text().as_str());
}

/// The end: a file at a store's path that this build did not write. No verb
/// runs against it, because there is no store to run one on.
///
/// The other model is not the provocation it once was: a caller cannot name a
/// model this build does not hold, which is the narrowing working rather than
/// a case going untested — the store's own tests still stamp one and meet it.
#[test]
fn a_file_this_build_did_not_write_opens_for_nobody() {
    let mut seams = seams();
    let directory = directory("incompatible");
    let store = opened(&directory, seams.embed.model());
    let (pools, _) = corpus(&directory, &store, &mut seams);
    drop(store);
    let path = directory.join("elsewhere.redb");
    let Ok(()) = fs::write(&path, b"this is not a redb database") else {
        unreachable!()
    };
    match Store::open(&path, seams.embed.model()) {
        Ok(Ok(_)) => unreachable!("the store opened"),
        Ok(Err(end)) => assert_eq!(end.path(), path),
        Err(error) => unreachable!("{error}"),
    }
    // And it is that file that ended it, not this build: the corpus beside it
    // still opens and still answers with what was added.
    let store = opened(&directory, seams.embed.model());
    let best = best(&store, &mut seams);
    assert_eq!(best.passage().handle().chunk().origin(), &pools);
}

/// Job 3's other half: the age an agent judges is the `ttl` its own `add`
/// named and the moment the bytes were actually read, carried through the
/// store untouched. Nothing here is scry's opinion — `expired` is computed by
/// whoever holds the provenance.
#[test]
fn the_age_an_agent_judges_is_the_one_its_add_named() {
    let mut seams = seams();
    let directory = directory("age");
    let store = opened(&directory, seams.embed.model());
    let before = SystemTime::now();
    let (pools, _) = corpus(&directory, &store, &mut seams);
    let after = SystemTime::now();
    let best = best(&store, &mut seams);
    assert_eq!(best.passage().handle().chunk().origin(), &pools);
    let provenance = traced(&store, best.passage().handle());
    assert!(provenance.fetched_at() >= before);
    assert!(provenance.fetched_at() <= after);
    assert_eq!(provenance.ttl(), TTL);
    assert!(!provenance.expired(provenance.fetched_at()));
    let Some(past) = provenance
        .fetched_at()
        .checked_add(TTL + Duration::from_secs(1))
    else {
        unreachable!()
    };
    assert!(provenance.expired(past));
}
