//! `delete`: the verb that takes a document out of the corpus.
//!
//! Nothing is decided here either. The store already removes by origin, and
//! `origin` already made one spelling of a document the only one — so this is
//! the vocabulary's word for that operation and no second implementation of
//! it.

use std::io;

use crate::origin::Origin;
use crate::store::Store;

trait DeleteVerb {
    fn delete(&self, origin: &Origin) -> io::Result<()>;
}

impl DeleteVerb for Store {
    fn delete(&self, origin: &Origin) -> io::Result<()> {
        self.remove(origin)
    }
}

impl Store {
    /// Takes away whatever is filed under `origin`.
    ///
    /// An origin holding nothing is a no-op, so deleting twice is not an
    /// error: `add` replaces without asking what was there, and this is the
    /// same corpus read the other way round. There is no verb that asks what
    /// was held, so an answer here could only be one an agent had no way to
    /// use.
    ///
    /// `origin` is normalised by its one constructor, so this reaches the
    /// document `add` filed rather than missing it by a spelling.
    ///
    /// A handle into a document deleted this way is `Gone` rather than
    /// `Stale`: the whole record leaves, so the verb that looks the document
    /// up finds nothing, instead of finding text that disagrees.
    ///
    /// # Errors
    ///
    /// The disk, which is weather. The vocabulary gives `delete` no failure of
    /// its own — it is the one verb that decides nothing.
    pub fn delete(&self, origin: &Origin) -> io::Result<()> {
        <Self as DeleteVerb>::delete(self, origin)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::handle::Handle;
    use crate::origin::Origin;
    use crate::scaffold::{filed, halved as document, held, model, origin, store};
    use crate::store::Store;

    /// A store of this test's own, emptied first so a rerun starts where the
    /// last one did.
    fn path(test: &str) -> PathBuf {
        crate::scaffold::path(module_path!(), test)
    }

    fn deleted(store: &Store, origin: &Origin) {
        let Ok(()) = store.delete(origin) else {
            unreachable!()
        };
    }

    #[test]
    fn deleting_takes_the_document_away() {
        let model = model("test");
        let store = store(&path("away"), &model);
        let document = document("a.md", "one two three four");
        filed(&store, &model, &document);
        deleted(&store, document.origin());
        assert!(held(&store, document.origin()).is_none());
    }

    #[test]
    fn deleting_twice_is_not_an_error() {
        let model = model("test");
        let store = store(&path("twice"), &model);
        let document = document("a.md", "one two three four");
        filed(&store, &model, &document);
        deleted(&store, document.origin());
        deleted(&store, document.origin());
        assert!(held(&store, document.origin()).is_none());
    }

    #[test]
    fn deleting_an_origin_holding_nothing_is_a_no_op() {
        let model = model("test");
        let store = store(&path("absent"), &model);
        let document = document("a.md", "one two three four");
        filed(&store, &model, &document);
        deleted(&store, &origin("nowhere.md"));
        assert!(held(&store, document.origin()).is_some());
    }

    #[test]
    fn deleting_one_origin_leaves_the_others() {
        let model = model("test");
        let store = store(&path("others"), &model);
        let first = document("a.md", "one two three four");
        let second = document("b.md", "five six seven eight");
        filed(&store, &model, &first);
        filed(&store, &model, &second);
        deleted(&store, first.origin());
        assert!(held(&store, first.origin()).is_none());
        assert_eq!(held(&store, second.origin()).as_ref(), Some(&second));
    }

    /// The criterion the node states, measured as far as it can be here.
    /// `Gone` and `Stale` are words `provenance` and `neighbours` say, and
    /// neither is built yet; what decides between them is this — the document
    /// is absent, and the handle's own text never stopped agreeing. A delete
    /// that edited the record instead of removing it would leave the other
    /// half of that pair true.
    #[test]
    fn a_handle_into_a_deleted_document_finds_nothing_rather_than_changed_text() {
        let model = model("test");
        let store = store(&path("handle"), &model);
        let document = document("a.md", "one two three four");
        filed(&store, &model, &document);
        let Some(&span) = document.spans().first() else {
            unreachable!()
        };
        let Some(passage) = Handle::mint(&document, span) else {
            unreachable!()
        };
        let handle = passage.handle();
        deleted(&store, document.origin());
        assert!(held(&store, handle.chunk().origin()).is_none());
        let Some(text) = handle.verify(&document) else {
            unreachable!("the handle's text stopped agreeing")
        };
        assert_eq!(Some(text), document.text().at(handle.chunk().span()));
    }
}
