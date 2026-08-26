//! The scaffolding the crate's own tests stand on.
//!
//! It is built for tests, so it ships in no build and is exported nowhere, as
//! `fixture` is. What lives here is what more than one test module was
//! writing for itself: the same construction written twice is two
//! constructions, and two constructions drift.
//!
//! A test module keeps whatever only it says — the wrapper around its own
//! verb, the text it is measured over — and takes the rest from here.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::count::Count;
use crate::document::Document;
use crate::embed::Embed;
use crate::embedding::Embedding;
use crate::handle::Handle;
use crate::limit::Limit;
use crate::model::Model;
use crate::origin::Origin;
use crate::slice::Slice;
use crate::store::Store;
use crate::text::Text;
use crate::vector::Vector;

/// How long the material is said to stay good for.
pub(crate) const TTL: Duration = Duration::from_secs(3600);

/// When the material was read.
///
/// Not the epoch: a `fetched_at` dropped somewhere between here and the disk
/// would read back as the epoch, and a document built on the epoch would say
/// nothing about that.
pub(crate) fn fetched_at() -> SystemTime {
    let Some(fetched_at) = SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(1_700_000_000))
    else {
        unreachable!()
    };
    fetched_at
}

/// Where the weights are kept: one cache for the whole machine, so they are
/// fetched once and never again.
///
/// The store's path decides nothing here. A cache beside each store would
/// fetch a hundred and thirty megabytes per test, and that price is what
/// decided it.
pub(crate) fn cache() -> PathBuf {
    std::env::temp_dir().join("scry-model-cache")
}

/// A directory this test alone writes in, emptied first so a rerun starts
/// where the last one did.
///
/// `module` is the caller's own `module_path!()`, so two modules that name a
/// test the same way still get a directory each rather than remembering not
/// to collide.
pub(crate) fn directory(module: &str, test: &str) -> PathBuf {
    let module = module.replace("::", "-");
    let directory = std::env::temp_dir().join(format!("{module}-{test}"));
    let _ = fs::remove_dir_all(&directory);
    let Ok(()) = fs::create_dir_all(&directory) else {
        unreachable!()
    };
    directory
}

/// The store's own path, inside a directory this test alone writes in.
pub(crate) fn path(module: &str, test: &str) -> PathBuf {
    directory(module, test).join("corpus.redb")
}

/// The embed seam, loaded from the one cache.
pub(crate) fn embed() -> Embed {
    match Embed::load(&cache()) {
        Ok(embed) => embed,
        Err(error) => unreachable!("{error}"),
    }
}

/// The two seams a verb is handed, agreeing on one model.
pub(crate) fn seams() -> (Embed, Slice) {
    let embed = embed();
    let slice = match Slice::new(&embed) {
        Ok(slice) => slice,
        Err(error) => unreachable!("{error}"),
    };
    (embed, slice)
}

/// A model of two dimensions, named `name`.
///
/// Nothing handed one of these embeds anything, so the store only ever
/// compares this name against the one it stamped.
pub(crate) fn model(name: &str) -> Model {
    let Some(limit) = Limit::new(512) else {
        unreachable!()
    };
    let Some(model) = Model::new(name, 2, limit) else {
        unreachable!()
    };
    model
}

/// The store at `path`, opened for `model`.
pub(crate) fn store(path: &Path, model: &Model) -> Store {
    match Store::open(path, model) {
        Ok(Ok(store)) => store,
        Ok(Err(end)) => unreachable!("{end}"),
        Err(error) => unreachable!("{error}"),
    }
}

/// The origin `spelling` names, spelled as every verb that names a document
/// spells it.
pub(crate) fn origin(spelling: &str) -> Origin {
    let Some(origin) = Origin::parse(spelling) else {
        unreachable!()
    };
    origin
}

/// The document read from `spelling` holding `body`, cut at `cuts`.
pub(crate) fn document(spelling: &str, body: &str, cuts: &[usize]) -> Document {
    let text = Text::from(body.to_owned());
    let mut spans = Vec::new();
    let mut start = 0;
    for end in cuts.iter().copied().chain([text.len()]) {
        let Some(span) = text.span(start, end) else {
            unreachable!()
        };
        spans.push(span);
        start = end;
    }
    let Some(document) = Document::new(origin(spelling), text, fetched_at(), TTL, spans) else {
        unreachable!()
    };
    document
}

/// The document read from `spelling` holding `body`, cut in two — so an
/// embedding per span is two of them.
pub(crate) fn halved(spelling: &str, body: &str) -> Document {
    document(spelling, body, &[body.len() / 2])
}

/// The embedding of `values` under `model`, normalised.
pub(crate) fn embedding(model: &Model, values: Vec<f32>) -> Embedding {
    let Some(vector) = Vector::normalise(values) else {
        unreachable!()
    };
    let Some(embedding) = Embedding::new(model.clone(), vector) else {
        unreachable!()
    };
    embedding
}

/// One embedding per span, which is what the store demands of a record.
///
/// No query is asked of these, so where each points decides nothing — except
/// that neighbours differ, so a record that handed two of them back the other
/// way round is not the record that was written. They are the axes, which is
/// what survives the store's own renormalisation to the same bits, so a
/// record read back is compared against the one written and not against a
/// tolerance.
pub(crate) fn embeddings(model: &Model, spans: usize) -> Vec<Embedding> {
    let mut embeddings = Vec::new();
    for span in 0..spans {
        let values = if span % 2 == 0 {
            vec![1.0, 0.0]
        } else {
            vec![0.0, 1.0]
        };
        embeddings.push(embedding(model, values));
    }
    embeddings
}

/// Files `document` in `store`, as `add` would have left it.
pub(crate) fn filed(store: &Store, model: &Model, document: &Document) {
    let Ok(()) = store.replace(document, &embeddings(model, document.spans().len())) else {
        unreachable!()
    };
}

/// What `store` holds under `origin`.
pub(crate) fn held(store: &Store, origin: &Origin) -> Option<Document> {
    match store.document(origin) {
        Ok(document) => document,
        Err(error) => unreachable!("{error}"),
    }
}

/// The handle for the chunk at `index` of `document`.
pub(crate) fn minted(document: &Document, index: usize) -> Handle {
    let Some(&span) = document.spans().get(index) else {
        unreachable!()
    };
    let Some(passage) = Handle::mint(document, span) else {
        unreachable!()
    };
    passage.handle().clone()
}

/// How many hits or passages are wanted.
pub(crate) fn count(wanted: usize) -> Count {
    let Some(count) = Count::new(wanted) else {
        unreachable!()
    };
    count
}
