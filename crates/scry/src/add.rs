//! `add`: the verb that fills the corpus.
//!
//! Nothing new is decided here. `fetch` says what bytes are, `slice` says
//! where the boundaries fall, `embed` says what a vector is, and `store` says
//! what one document per origin means — this is the order they run in, and the
//! order is the whole of it. A verb that reached past those parts to make its
//! own ruling would be a second place that ruling lived.

use std::io;
use std::time::Duration;

use crate::document::Document;
use crate::embed::Embed;
use crate::fetch::fetch;
use crate::origin::Origin;
use crate::refusal::AddRefusal;
use crate::slice::Slice;
use crate::store::Store;

impl Store {
    /// Reads what is at `origin`, cuts it, embeds it, and files it — replacing
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
    /// hand, and then in one put — so an add that fails leaves the document
    /// that was there untouched rather than half of a new one.
    ///
    /// # Errors
    ///
    /// [`NotFound`](AddRefusal::NotFound) when no bytes arrive at `origin`,
    /// and [`NotText`](AddRefusal::NotText) when the bytes there are not
    /// something scry will index. Those two are the whole of what `add`
    /// decides.
    ///
    /// Outside them: the disk, the network's plumbing, the tokenizer and the
    /// inference session, which are weather. They are nested outside the
    /// refusal rather than seated beside it, as
    /// [`open`](Store::open) does, because the vocabulary gives `add` two
    /// failures and weather is not a word in it.
    pub fn add(
        &self,
        embed: &mut Embed,
        slice: &Slice,
        origin: &Origin,
        ttl: Duration,
    ) -> io::Result<Result<(), AddRefusal>> {
        let (text, fetched_at) = match fetch(origin) {
            Ok(read) => read,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let spans = slice.spans(&text)?;
        let document = Document::new(origin.clone(), text, fetched_at, ttl, spans)
            .ok_or_else(|| io::Error::other("the text was not cut into a partition of itself"))?;
        // Every span is one the document was built from, so this reads; the
        // alternative to saying so is a panic, which is not one of the five.
        let passages = document
            .spans()
            .iter()
            .map(|span| document.text().at(*span))
            .collect::<Option<Vec<&str>>>()
            .ok_or_else(|| io::Error::other("a span of the document does not read"))?;
        let embeddings = embed.passages(&passages)?;
        self.replace(&document, &embeddings)?;
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::document::Document;
    use crate::embed::Embed;
    use crate::embedding::Embedding;
    use crate::origin::Origin;
    use crate::refusal::AddRefusal;
    use crate::scaffold::{TTL, held, seams};
    use crate::slice::Slice;
    use crate::store::Store;

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
        match store.add(embed, slice, origin, TTL) {
            Ok(Ok(())) => (),
            Ok(Err(refusal)) => unreachable!("{refusal}"),
            Err(error) => unreachable!("{error}"),
        }
    }

    fn refused(store: &Store, embed: &mut Embed, slice: &Slice, origin: &Origin) -> AddRefusal {
        match store.add(embed, slice, origin, TTL) {
            Ok(Ok(())) => unreachable!("the origin was added"),
            Ok(Err(refusal)) => refusal,
            Err(error) => unreachable!("{error}"),
        }
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
}
