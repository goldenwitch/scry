//! scry: a corpus an agent can search, add to, and trace back to its bytes.
//!
//! The vocabulary this crate speaks is fixed by
//! `human-authored-specs/vocabulary.md`. Every production there has one type
//! here, and no type here names anything that is not there:
//!
//! | production | type |
//! | --- | --- |
//! | `corpus` | [`Store`] |
//! | `document` | `Document` |
//! | `origin` | [`Origin`] |
//! | `chunk` | [`Chunk`] |
//! | `span` | [`Span`] |
//! | `handle` | [`Handle`] |
//! | `passage` | [`Passage`] |
//! | `hit` | [`Hit`] |
//! | `provenance` | [`Provenance`] |
//! | `model` | [`Model`] |
//! | `embedding` | `Embedding` |
//! | `vector` | `Vector` |
//! | `limit` | `Limit` |
//! | `window` | `Window` |
//!
//! The terminals the prose names get types too, where a bare number or string
//! would have let a mistake through: [`Text`], [`Digest`], [`Score`],
//! [`Query`] and [`Count`]. `start` and `end` live inside [`Span`];
//! `fetched_at` is a [`SystemTime`](std::time::SystemTime) and `ttl` a
//! [`Duration`](std::time::Duration), which already have those names.
//!
//! The rows written without a link are the productions no signature a caller
//! can call ever names, so a caller cannot name them either. `document` is
//! the whole of a document's text, which is reached through its `origin` and
//! not through us; `embedding`, `vector`, `limit` and `window` are the embed
//! seam's own and are seen nowhere else. [`Model`] is exported with no way to
//! make one and no way to read one: [`Store::open`] takes it and
//! [`Embed::model`] is the only thing that yields it, so a store cannot be
//! stamped with a name no [`Embed`] produces.
//!
//! What a caller can call is [`Store::open`] and the five verbs — [`add`],
//! [`delete`], [`search`], [`neighbours`] and [`provenance`] — over the two
//! seams they take, which are built with [`Embed::load`] and [`Slice::new`].
//! Everything else here is a value in flight: minted by scry and read by a
//! caller. A handle may also be rehydrated after transport with
//! [`Handle::from_parts`]; the verbs still verify its claim against the corpus.
//!
//! [`add`]: Store::add
//! [`delete`]: Store::delete
//! [`search`]: Store::search
//! [`neighbours`]: Store::neighbours
//! [`provenance`]: Store::provenance
//!
//! The five failures are [`AddRefusal`], [`HandleRefusal`] and
//! [`Incompatible`], grouped as the verbs that answer with them are.
//!
//! What the types make impossible rather than merely checked: a span that
//! exceeds its text, a document whose spans do not partition its text, a
//! window larger than its model's limit, a vector that is not normalised, an
//! embedding separated from its model or of the wrong dimension, a score that
//! is not a cosine, and a cosine between models that share no space.
//!
//! The parts `human-authored-specs/architecture.md` names take their own names
//! from that table as they arrive: `fetch` is where an origin becomes bytes,
//! and the only place `fetched_at` is taken; [`Embed`] is the embed seam, and
//! the model and the vectors it binds are seen nowhere else; [`Slice`] is where
//! the boundaries fall, and holds the only tokenizer that counts; [`Store`] is
//! the one place state lives, and holds the only redb file; `Handle::mint`
//! and `Handle::verify` are the handle seam, and the only place text is
//! hashed.

mod add;
mod benchmark;
mod chunk;
mod count;
mod delete;
mod digest;
mod document;
mod embed;
mod embedding;
mod fetch;
/// The known answer search is checked against. It is data and a derivation,
/// not a part, so it ships in no build and is exported nowhere.
#[cfg(test)]
mod fixture;
mod handle;
mod hit;
mod incompatible;
mod limit;
mod model;
mod neighbours;
mod origin;
mod passage;
mod provenance;
mod query;
mod refusal;
/// The scaffolding the crate's own tests stand on. It is built for tests, so
/// it ships in no build and is exported nowhere.
#[cfg(test)]
mod scaffold;
mod score;
mod search;
mod slice;
mod span;
mod store;
mod text;
mod vector;
mod window;

pub use crate::add::{AddOutcome, AddReport, AddResult};
#[cfg(feature = "benchmark-instrumentation")]
pub use crate::benchmark::{Collector as BenchmarkCollector, Snapshot as BenchmarkSnapshot};
pub use crate::chunk::Chunk;
pub use crate::count::Count;
pub use crate::digest::Digest;
#[cfg(feature = "benchmark-instrumentation")]
pub use crate::embed::DEFAULT_PASSAGE_MICROBATCH_SIZE;
pub use crate::embed::Embed;
#[cfg(feature = "benchmark-instrumentation")]
pub use crate::embed::MAX_PASSAGE_MICROBATCH_SIZE;
pub use crate::handle::Handle;
pub use crate::hit::Hit;
pub use crate::incompatible::Incompatible;
pub use crate::model::Model;
pub use crate::origin::Origin;
pub use crate::passage::Passage;
pub use crate::provenance::Provenance;
pub use crate::query::Query;
pub use crate::refusal::{AddRefusal, HandleRefusal};
pub use crate::score::Score;
pub use crate::slice::Slice;
pub use crate::span::Span;
pub use crate::store::Store;
pub use crate::text::Text;
