//! `store`: the corpus, in one redb file.
//!
//! Everything redb is kept behind this file, so no other part of scry holds a
//! table, a transaction, or knows how a document is written down. The path
//! back to a document's bytes runs through its [`Origin`], into the world, so
//! nothing outside scry ever reads this file and its layout stays ours to
//! change.

mod record;

use std::io;
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition, TableError};

use crate::document::Document;
use crate::embedding::Embedding;
use crate::incompatible::Incompatible;
use crate::model::Model;
use crate::origin::Origin;

/// One document per origin, keyed by the origin's spelling, with the
/// embedding beside each of its spans in the same record.
const DOCUMENTS: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("documents");

/// The layout and the model this store was written by.
const STAMP: TableDefinition<'_, &str, &[u8]> = TableDefinition::new("stamp");

/// The one key in [`STAMP`]. A table of one entry is how a single fact is
/// spelled here, since redb has no other place to put it.
const STAMPED: &str = "stamp";

/// `$corpus`: one `Document` per [`Origin`], and an `Embedding` beside
/// each of its spans.
///
/// This is the only state scry has. Absent, the file is created; present, it
/// is opened, and after the first run the file is its own configuration.
///
/// A store remembers the model it was written by. A build holding another one
/// does not open it, does not repair it, and runs no verb against it — see
/// [`open`](Self::open).
pub struct Store {
    database: Database,
    model: Model,
    #[cfg(test)]
    fail_next_commit: AtomicBool,
}

pub(crate) struct PreparedRecord {
    key: String,
    bytes: Vec<u8>,
}

impl Store {
    /// Opens the store at `path` for `model`, creating it if it is not there.
    ///
    /// The inner `Err` is the end the vocabulary names: the store was written
    /// by another model or another layout, or its bytes will not read as
    /// either — so it opens for nobody and nothing an agent does repairs it.
    /// It is nested inside the outer `Err` rather than sitting beside it,
    /// because a disk that will not read is weather and weather is not one of
    /// scry's failures — so the two can never be mistaken for one another.
    ///
    /// # Errors
    ///
    /// The disk, which is weather.
    pub fn open(path: &Path, model: &Model) -> io::Result<Result<Self, Incompatible>> {
        let stamp = record::stamp(model)?;
        let database = match Database::create(path) {
            Ok(database) => database,
            Err(error) => {
                return match fault(error) {
                    Fault::End => Ok(Err(end(path))),
                    Fault::Weather(error) => Err(error),
                };
            }
        };
        let store = Self {
            database,
            model: model.clone(),
            #[cfg(test)]
            fail_next_commit: AtomicBool::new(false),
        };
        if store.stamped(&stamp)? {
            Ok(Ok(store))
        } else {
            Ok(Err(end(path)))
        }
    }

    /// Puts `document` and its embeddings where its origin says, replacing
    /// whatever was there.
    ///
    /// Adding the same origin replaces the document, so there is no verb that
    /// asks what was there first.
    ///
    /// Crate-visible: `add` is the only writer the vocabulary names, and a
    /// second way in from outside would be a way to fill the corpus with
    /// text no `fetch` read and vectors no `Embed` produced.
    ///
    /// # Errors
    ///
    /// The disk, which is weather; and embeddings that are not one per span
    /// or name another model, which is a mistake rather than a decision.
    pub(crate) fn prepare_replace(
        &self,
        document: &Document,
        embeddings: &[Embedding],
    ) -> io::Result<PreparedRecord> {
        Ok(PreparedRecord {
            key: document.origin().to_string(),
            bytes: record::encode(document, embeddings, &self.model)?,
        })
    }

    /// Commits one prepared replacement in one redb write transaction.
    pub(crate) fn commit_replace(&self, prepared: &PreparedRecord) -> io::Result<()> {
        let write = self.database.begin_write().map_err(io::Error::other)?;
        #[cfg(test)]
        if self.fail_next_commit.swap(false, Ordering::Relaxed) {
            return Err(io::Error::other("synthetic store write failure"));
        }
        {
            let mut documents = write.open_table(DOCUMENTS).map_err(io::Error::other)?;
            documents
                .insert(prepared.key.as_str(), prepared.bytes.as_slice())
                .map_err(io::Error::other)?;
        }
        write.commit().map_err(io::Error::other)
    }

    /// Prepares and commits one replacement.
    #[cfg(test)]
    pub(crate) fn replace(&self, document: &Document, embeddings: &[Embedding]) -> io::Result<()> {
        let prepared = self.prepare_replace(document, embeddings)?;
        self.commit_replace(&prepared)
    }

    #[cfg(test)]
    pub(crate) fn fail_next_commit(&self) {
        self.fail_next_commit.store(true, Ordering::Relaxed);
    }

    /// Removes whatever is filed under `origin`.
    ///
    /// An origin holding nothing is a no-op, as `add` replaces without asking
    /// what was there.
    ///
    /// Crate-visible, as the store's other operations are: this one is the
    /// whole of a verb, and [`delete`](Self::delete) is the vocabulary's name
    /// for it. Two public names for one operation would be the plan's own
    /// pitfall wearing its other face.
    ///
    /// # Errors
    ///
    /// The disk, which is weather.
    pub(crate) fn remove(&self, origin: &Origin) -> io::Result<()> {
        let key = origin.to_string();
        let write = self.database.begin_write().map_err(io::Error::other)?;
        {
            let mut documents = write.open_table(DOCUMENTS).map_err(io::Error::other)?;
            documents.remove(key.as_str()).map_err(io::Error::other)?;
        }
        write.commit().map_err(io::Error::other)
    }

    /// The document filed under `origin`, if one is.
    ///
    /// Crate-visible: the whole of a document is not something the
    /// vocabulary hands back, and the path to its bytes runs through its
    /// [`Origin`], into the world, rather than through this file.
    ///
    /// # Errors
    ///
    /// The disk, which is weather.
    pub(crate) fn document(&self, origin: &Origin) -> io::Result<Option<Document>> {
        let key = origin.to_string();
        let read = self.database.begin_read().map_err(io::Error::other)?;
        let documents = read.open_table(DOCUMENTS).map_err(io::Error::other)?;
        let Some(found) = documents.get(key.as_str()).map_err(io::Error::other)? else {
            return Ok(None);
        };
        let (document, _) = record::decode(origin, found.value(), &self.model)?;
        Ok(Some(document))
    }

    /// Walks the whole corpus, handing each document and its embeddings to
    /// `visit`.
    ///
    /// The walk stays inside the store rather than being handed out as an
    /// iterator, so the transaction it runs in is not something a caller can
    /// hold open, and no verb ever enumerates the corpus for an agent.
    ///
    /// `visit` may fail, and the walk stops where it does. A caller that can
    /// tell something is wrong at one document has no way to say so once the
    /// corpus has been walked past it, and would otherwise have to carry the
    /// news out in a variable.
    ///
    /// # Errors
    ///
    /// The disk, which is weather; and whatever `visit` answers with.
    pub(crate) fn scan(
        &self,
        mut visit: impl FnMut(Document, Vec<Embedding>) -> io::Result<()>,
    ) -> io::Result<()> {
        let read = self.database.begin_read().map_err(io::Error::other)?;
        let documents = read.open_table(DOCUMENTS).map_err(io::Error::other)?;
        for entry in documents.iter().map_err(io::Error::other)? {
            let (key, value) = entry.map_err(io::Error::other)?;
            let origin = Origin::parse(key.value()).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "the store holds a key that is not an origin",
                )
            })?;
            let (document, embeddings) = record::decode(&origin, value.value(), &self.model)?;
            visit(document, embeddings)?;
        }
        Ok(())
    }

    /// Whether this store carries `stamp`, writing it first if the file is
    /// one nothing has written to yet.
    ///
    /// A database holding no tables at all is one this call just created, or
    /// one indistinguishable from it, so it is stamped. A database holding
    /// tables that are not ours is another layout.
    ///
    /// A file whose own bytes disagree with each other carries no stamp this
    /// build can read, and `false` is already the answer that sends the one
    /// caller to the end — so corruption needs no second channel out of here.
    fn stamped(&self, stamp: &[u8]) -> io::Result<bool> {
        match self.carries(stamp) {
            Ok(carried) => Ok(carried),
            Err(error) => match fault(error) {
                Fault::End => Ok(false),
                Fault::Weather(error) => Err(error),
            },
        }
    }

    /// The read and the write [`stamped`](Self::stamped) is the judgement of.
    ///
    /// Every failure here leaves as redb's own, so which of the two it is is
    /// decided once, in [`fault`], rather than at each call.
    fn carries(&self, stamp: &[u8]) -> Result<bool, redb::Error> {
        let read = self.database.begin_read()?;
        match read.open_table(STAMP) {
            Ok(table) => {
                let found = table.get(STAMPED)?;
                Ok(found.is_some_and(|found| found.value() == stamp))
            }
            Err(TableError::TableDoesNotExist(_)) => {
                let written = read.list_tables()?.next().is_some();
                drop(read);
                if written {
                    return Ok(false);
                }
                let write = self.database.begin_write()?;
                {
                    let mut table = write.open_table(STAMP)?;
                    table.insert(STAMPED, stamp)?;
                    write.open_table(DOCUMENTS)?;
                }
                write.commit()?;
                Ok(true)
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Which of the two a failure from redb is.
enum Fault {
    /// A store this build cannot use, from which nothing proceeds.
    End,
    /// The disk, which is weather.
    Weather(io::Error),
}

/// Reads a failure from redb as one or the other.
///
/// Weather says try again: a disk that is busy, a permission that is missing,
/// a volume that is full. The end says nothing an agent does will change this.
/// A file that is not a redb database, or is one written by a redb this build
/// does not read, is the end by the plainest reading — and so is a file whose
/// own bytes disagree with each other, which no verb can run against and no
/// second attempt will mend.
fn fault(error: impl Into<redb::Error>) -> Fault {
    match error.into() {
        redb::Error::UpgradeRequired(_) | redb::Error::Corrupted(_) => Fault::End,
        redb::Error::Io(error) if error.kind() == io::ErrorKind::InvalidData => Fault::End,
        redb::Error::Io(error) => Fault::Weather(error),
        error => Fault::Weather(io::Error::other(error)),
    }
}

/// The end, for the store at `path`.
fn end(path: &Path) -> Incompatible {
    Incompatible::new(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use hmac_sha256::Hash;

    use super::record::{self, CHECKSUM};
    use super::{STAMP, STAMPED, Store};
    use crate::embedding::Embedding;
    use crate::incompatible::Incompatible;
    use crate::model::Model;
    use crate::scaffold::{embedding, embeddings, halved as document, model, origin, store};

    /// The byte of redb's own header this test turns over to make a file its
    /// own reader calls corrupt. Found by walking the header a byte at a time
    /// rather than read off a layout redb does not publish, and it is the one
    /// place in the first ninety-six bytes that answers `Corrupted` rather
    /// than opening, reading as another layout, or panicking inside redb.
    const CORRUPT: usize = 64;

    /// A store of this test's own, emptied first so a rerun starts where the
    /// last one did.
    fn path(test: &str) -> PathBuf {
        crate::scaffold::path(module_path!(), test)
    }

    fn refused(path: &Path, model: &Model) -> Incompatible {
        match Store::open(path, model) {
            Ok(Ok(_)) => unreachable!("the store opened"),
            Ok(Err(end)) => end,
            Err(error) => unreachable!("{error}"),
        }
    }

    /// The embeddings a two-span document is filed with.
    fn pair(model: &Model) -> Vec<Embedding> {
        embeddings(model, 2)
    }

    #[test]
    fn a_corpus_written_by_one_open_is_read_by_the_next() {
        let path = path("survives");
        let model = model("test");
        let document = document("a.md", "one two three four");
        {
            let store = store(&path, &model);
            let Ok(()) = store.replace(&document, &pair(&model)) else {
                unreachable!()
            };
        }
        let store = store(&path, &model);
        let Ok(read) = store.document(document.origin()) else {
            unreachable!()
        };
        assert_eq!(read.as_ref(), Some(&document));
    }

    #[test]
    fn an_origin_holding_nothing_reads_as_nothing() {
        let store = store(&path("absent"), &model("test"));
        let Ok(read) = store.document(&origin("nowhere.md")) else {
            unreachable!()
        };
        assert!(read.is_none());
    }

    #[test]
    fn replacing_overwrites_in_place() {
        let path = path("replace");
        let model = model("test");
        let store = store(&path, &model);
        let first = document("a.md", "one two three four");
        let second = document("a.md", "five six seven eight");
        let (Ok(()), Ok(())) = (
            store.replace(&first, &pair(&model)),
            store.replace(&second, &pair(&model)),
        ) else {
            unreachable!()
        };
        let mut held = Vec::new();
        let Ok(()) = store.scan(|document, _| {
            held.push(document);
            Ok(())
        }) else {
            unreachable!()
        };
        assert_eq!(held, vec![second]);
    }

    #[test]
    fn a_write_phase_failure_is_reported_after_preparation() {
        let path = path("write-failure");
        let model = model("test");
        let store = store(&path, &model);
        let document = document("a.md", "one two three four");
        let Ok(prepared) = store.prepare_replace(&document, &pair(&model)) else {
            unreachable!()
        };
        store.fail_next_commit();
        assert!(store.commit_replace(&prepared).is_err());
        let Ok(read) = store.document(document.origin()) else {
            unreachable!()
        };
        assert!(read.is_none());
    }

    #[test]
    fn a_preparation_failure_leaves_the_existing_record_untouched() {
        let path = path("preparation-failure");
        let model = model("test");
        let store = store(&path, &model);
        let first = document("a.md", "one two three four");
        let replacement = document("a.md", "five six seven eight");
        let Ok(()) = store.replace(&first, &pair(&model)) else {
            unreachable!()
        };
        let invalid = vec![embedding(&model, vec![1.0, 0.0])];
        assert!(store.prepare_replace(&replacement, &invalid).is_err());
        let Ok(read) = store.document(first.origin()) else {
            unreachable!()
        };
        assert_eq!(read.as_ref(), Some(&first));
    }

    #[test]
    fn removing_an_origin_holding_nothing_is_a_no_op() {
        let path = path("remove-absent");
        let model = model("test");
        let store = store(&path, &model);
        let document = document("a.md", "one two three four");
        let Ok(()) = store.replace(&document, &pair(&model)) else {
            unreachable!()
        };
        let (Ok(()), Ok(())) = (
            store.remove(&origin("nowhere.md")),
            store.remove(&origin("nowhere.md")),
        ) else {
            unreachable!()
        };
        let Ok(read) = store.document(document.origin()) else {
            unreachable!()
        };
        assert!(read.is_some());
    }

    #[test]
    fn removing_takes_the_document_away() {
        let path = path("remove");
        let model = model("test");
        let store = store(&path, &model);
        let document = document("a.md", "one two three four");
        let (Ok(()), Ok(())) = (
            store.replace(&document, &pair(&model)),
            store.remove(document.origin()),
        ) else {
            unreachable!()
        };
        let Ok(read) = store.document(document.origin()) else {
            unreachable!()
        };
        assert!(read.is_none());
    }

    #[test]
    fn a_scan_walks_every_document_with_its_embeddings() {
        let path = path("scan");
        let model = model("test");
        let store = store(&path, &model);
        let first = document("a.md", "one two three four");
        let second = document("b.md", "five six seven eight");
        let (Ok(()), Ok(())) = (
            store.replace(&first, &pair(&model)),
            store.replace(&second, &pair(&model)),
        ) else {
            unreachable!()
        };
        let mut walked = Vec::new();
        let Ok(()) = store.scan(|document, embeddings| {
            walked.push((document, embeddings));
            Ok(())
        }) else {
            unreachable!()
        };
        assert_eq!(walked.len(), 2);
        for (document, embeddings) in &walked {
            assert_eq!(embeddings.len(), document.spans().len());
            assert_eq!(embeddings, &pair(&model));
        }
    }

    #[test]
    fn a_store_written_by_another_model_opens_for_nobody() {
        let path = path("another-model");
        {
            let _ = store(&path, &model("test"));
        }
        let end = refused(&path, &model("other"));
        assert_eq!(end.path(), path);
    }

    #[test]
    fn a_store_written_by_another_layout_opens_for_nobody() {
        let path = path("another-layout");
        {
            let _ = store(&path, &model("test"));
        }
        // Another layout is another stamp, which is what this build compares
        // against; forging one is the only way to write a layout this build
        // does not have.
        {
            let Ok(database) = redb::Database::create(&path) else {
                unreachable!()
            };
            let Ok(write) = database.begin_write() else {
                unreachable!()
            };
            {
                let Ok(mut table) = write.open_table(STAMP) else {
                    unreachable!()
                };
                let Ok(_) = table.insert(STAMPED, b"another layout".as_slice()) else {
                    unreachable!()
                };
            }
            let Ok(()) = write.commit() else {
                unreachable!()
            };
        }
        let _ = refused(&path, &model("test"));
    }

    #[test]
    fn a_file_that_is_not_a_store_opens_for_nobody() {
        let path = path("not-a-store");
        let Ok(()) = fs::write(&path, b"this is not a redb database") else {
            unreachable!()
        };
        let _ = refused(&path, &model("test"));
    }

    #[test]
    fn a_store_whose_bytes_disagree_with_each_other_opens_for_nobody() {
        let path = path("corrupt");
        let model = model("test");
        {
            let store = store(&path, &model);
            let Ok(()) = store.replace(&document("a.md", "one two three four"), &pair(&model))
            else {
                unreachable!()
            };
        }
        // One byte of redb's own header, turned over. The file is still a
        // redb database by every other reading, and redb answers `Corrupted`
        // for it — which is not another model and not another layout, and is
        // not weather either: no second attempt makes this file read.
        let Ok(mut bytes) = fs::read(&path) else {
            unreachable!()
        };
        let Some(byte) = bytes.get_mut(CORRUPT) else {
            unreachable!()
        };
        *byte ^= 0xff;
        let Ok(()) = fs::write(&path, &bytes) else {
            unreachable!()
        };
        let end = refused(&path, &model);
        assert_eq!(end.path(), path);
    }

    #[test]
    fn a_stored_vector_that_was_altered_is_not_read_back() {
        let model = model("test");
        let document = document("a.md", "one two three four");
        // Two directions that are not one another's multiples, so altering
        // one float moves where the vector points rather than only how long
        // it is — which normalising would put back.
        let embeddings = vec![
            embedding(&model, vec![3.0, 4.0]),
            embedding(&model, vec![1.0, 2.0]),
        ];
        let Ok(bytes) = record::encode(&document, &embeddings, &model) else {
            unreachable!()
        };
        // The vector half is the tail of the record: two embeddings of two
        // floats, before the checksum. This writes another finite float over
        // the first of them, which is what a flipped bit leaves behind.
        let Some(at) = bytes.len().checked_sub(CHECKSUM + 2 * 2 * 4) else {
            unreachable!()
        };
        let mut altered = bytes.clone();
        let Some(slot) = altered.get_mut(at..at + 4) else {
            unreachable!()
        };
        slot.copy_from_slice(&(-3.0f32).to_le_bytes());
        assert!(record::decode(document.origin(), &altered, &model).is_err());

        // What the cover is worth, provoked rather than argued: the same
        // altered bytes under a checksum of their own read back, and they read
        // back as an embedding nobody wrote, with no refusal and no error.
        let Some(body) = altered.get(..at + 2 * 2 * 4) else {
            unreachable!()
        };
        let mut forged = body.to_vec();
        forged.extend_from_slice(&Hash::hash(body));
        let Ok((_, read)) = record::decode(document.origin(), &forged, &model) else {
            unreachable!()
        };
        assert_ne!(read, embeddings);
    }

    #[test]
    fn a_database_holding_tables_that_are_not_ours_opens_for_nobody() {
        let path = path("another-database");
        {
            let Ok(database) = redb::Database::create(&path) else {
                unreachable!()
            };
            let Ok(write) = database.begin_write() else {
                unreachable!()
            };
            let theirs: redb::TableDefinition<'_, &str, &[u8]> =
                redb::TableDefinition::new("theirs");
            {
                let Ok(mut table) = write.open_table(theirs) else {
                    unreachable!()
                };
                let Ok(_) = table.insert("a", b"b".as_slice()) else {
                    unreachable!()
                };
            }
            let Ok(()) = write.commit() else {
                unreachable!()
            };
        }
        let _ = refused(&path, &model("test"));
    }

    #[test]
    fn embeddings_that_are_not_one_per_span_are_refused() {
        let path = path("count");
        let model = model("test");
        let store = store(&path, &model);
        let document = document("a.md", "one two three four");
        let one = vec![embedding(&model, vec![1.0, 0.0])];
        assert!(store.replace(&document, &one).is_err());
    }

    #[test]
    fn embeddings_from_another_model_are_refused() {
        let path = path("mismatch");
        let mine = model("test");
        let store = store(&path, &mine);
        let document = document("a.md", "one two three four");
        assert!(
            store
                .replace(&document, &embeddings(&model("other"), 2))
                .is_err()
        );
    }
}
