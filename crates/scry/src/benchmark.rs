#[cfg(feature = "benchmark-instrumentation")]
mod active {
    use std::cell::RefCell;

    thread_local! {
        static CURRENT: RefCell<Option<Snapshot>> = const { RefCell::new(None) };
    }

    /// Deterministic observations collected at private scry runtime boundaries.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub struct Snapshot {
        source_bytes: u64,
        sliced_spans: u64,
        embedding_calls: u64,
        embedding_input_bytes: u64,
        embedding_vectors: u64,
        write_transactions: u64,
        read_transactions: u64,
        search_documents: u64,
        search_spans: u64,
        search_hits: u64,
        search_output_bytes: u64,
        neighbour_passages: u64,
        neighbour_output_bytes: u64,
        provenance_lookups: u64,
        add_members: u64,
        add_upserted: u64,
        add_refused: u64,
        add_failed: u64,
        add_uncertain: u64,
        add_not_attempted: u64,
        owned_live_bytes_high_water: u64,
        overflowed: bool,
    }

    impl Snapshot {
        /// Bytes successfully read from named text origins.
        #[must_use]
        pub const fn source_bytes(&self) -> u64 {
            self.source_bytes
        }

        /// Spans produced by the slicer.
        #[must_use]
        pub const fn sliced_spans(&self) -> u64 {
            self.sliced_spans
        }

        /// Calls made to the embedding model.
        #[must_use]
        pub const fn embedding_calls(&self) -> u64 {
            self.embedding_calls
        }

        /// Input bytes handed to the embedding model.
        #[must_use]
        pub const fn embedding_input_bytes(&self) -> u64 {
            self.embedding_input_bytes
        }

        /// Embedding vectors returned by the model.
        #[must_use]
        pub const fn embedding_vectors(&self) -> u64 {
            self.embedding_vectors
        }

        /// Redb write transactions begun by the store.
        #[must_use]
        pub const fn write_transactions(&self) -> u64 {
            self.write_transactions
        }

        /// Redb read transactions begun by the store.
        #[must_use]
        pub const fn read_transactions(&self) -> u64 {
            self.read_transactions
        }

        /// Documents visited by a search scan.
        #[must_use]
        pub const fn search_documents(&self) -> u64 {
            self.search_documents
        }

        /// Spans visited by a search scan.
        #[must_use]
        pub const fn search_spans(&self) -> u64 {
            self.search_spans
        }

        /// Hits retained by a search result.
        #[must_use]
        pub const fn search_hits(&self) -> u64 {
            self.search_hits
        }

        /// Passage bytes returned by search.
        #[must_use]
        pub const fn search_output_bytes(&self) -> u64 {
            self.search_output_bytes
        }

        /// Passages returned by neighbours.
        #[must_use]
        pub const fn neighbour_passages(&self) -> u64 {
            self.neighbour_passages
        }

        /// Passage bytes returned by neighbours.
        #[must_use]
        pub const fn neighbour_output_bytes(&self) -> u64 {
            self.neighbour_output_bytes
        }

        /// Provenance lookups completed against the store.
        #[must_use]
        pub const fn provenance_lookups(&self) -> u64 {
            self.provenance_lookups
        }

        /// Members submitted to add.
        #[must_use]
        pub const fn add_members(&self) -> u64 {
            self.add_members
        }

        /// Add members that committed successfully.
        #[must_use]
        pub const fn add_upserted(&self) -> u64 {
            self.add_upserted
        }

        /// Add members refused by source policy.
        #[must_use]
        pub const fn add_refused(&self) -> u64 {
            self.add_refused
        }

        /// Add members that failed before a write.
        #[must_use]
        pub const fn add_failed(&self) -> u64 {
            self.add_failed
        }

        /// Add members whose write outcome was uncertain.
        #[must_use]
        pub const fn add_uncertain(&self) -> u64 {
            self.add_uncertain
        }

        /// Add members not attempted after an earlier failure.
        #[must_use]
        pub const fn add_not_attempted(&self) -> u64 {
            self.add_not_attempted
        }

        /// Highest observed logical live-byte region.
        #[must_use]
        pub const fn owned_live_bytes_high_water(&self) -> u64 {
            self.owned_live_bytes_high_water
        }

        /// Whether any counter could not be represented exactly.
        #[must_use]
        pub const fn overflowed(&self) -> bool {
            self.overflowed
        }
    }

    /// Collects private runtime observations for one benchmark workload.
    #[derive(Debug)]
    pub struct Collector {
        previous: Option<Snapshot>,
        active: bool,
    }

    impl Collector {
        /// Starts a fresh collection on the current thread.
        #[must_use]
        pub fn start() -> Self {
            let previous = CURRENT.with(|current| current.replace(Some(Snapshot::default())));
            Self {
                previous,
                active: true,
            }
        }

        /// Stops collection and returns the observations.
        #[must_use]
        pub fn finish(mut self) -> Snapshot {
            self.stop()
        }

        fn stop(&mut self) -> Snapshot {
            if !self.active {
                return Snapshot::default();
            }
            self.active = false;
            CURRENT.with(|current| {
                let snapshot = current.replace(self.previous.take());
                snapshot.unwrap_or_default()
            })
        }
    }

    impl Drop for Collector {
        fn drop(&mut self) {
            let _ = self.stop();
        }
    }

    fn update(update: impl FnOnce(&mut Snapshot)) {
        CURRENT.with(|current| {
            if let Some(snapshot) = current.borrow_mut().as_mut() {
                update(snapshot);
            }
        });
    }

    fn add(slot: &mut u64, amount: u64, overflowed: &mut bool) {
        match slot.checked_add(amount) {
            Some(value) => *slot = value,
            None => *overflowed = true,
        }
    }

    fn add_usize(slot: &mut u64, amount: usize, overflowed: &mut bool) {
        match u64::try_from(amount) {
            Ok(amount) => add(slot, amount, overflowed),
            Err(_) => *overflowed = true,
        }
    }

    pub(super) fn source_bytes(amount: usize) {
        update(|snapshot| add_usize(&mut snapshot.source_bytes, amount, &mut snapshot.overflowed));
    }

    pub(super) fn sliced_spans(amount: usize) {
        update(|snapshot| add_usize(&mut snapshot.sliced_spans, amount, &mut snapshot.overflowed));
    }

    pub(super) fn embedding_call(input_bytes: usize) {
        update(|snapshot| {
            add(&mut snapshot.embedding_calls, 1, &mut snapshot.overflowed);
            add_usize(
                &mut snapshot.embedding_input_bytes,
                input_bytes,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn embedding_vectors(amount: usize) {
        update(|snapshot| {
            add_usize(
                &mut snapshot.embedding_vectors,
                amount,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn write_transaction() {
        update(|snapshot| {
            add(
                &mut snapshot.write_transactions,
                1,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn read_transaction() {
        update(|snapshot| add(&mut snapshot.read_transactions, 1, &mut snapshot.overflowed));
    }

    pub(super) fn search_document(span_count: usize) {
        update(|snapshot| {
            add(&mut snapshot.search_documents, 1, &mut snapshot.overflowed);
            add_usize(
                &mut snapshot.search_spans,
                span_count,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn search_hit(bytes: usize) {
        update(|snapshot| {
            add(&mut snapshot.search_hits, 1, &mut snapshot.overflowed);
            add_usize(
                &mut snapshot.search_output_bytes,
                bytes,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn neighbour_passage(bytes: usize) {
        update(|snapshot| {
            add(
                &mut snapshot.neighbour_passages,
                1,
                &mut snapshot.overflowed,
            );
            add_usize(
                &mut snapshot.neighbour_output_bytes,
                bytes,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn provenance_lookup() {
        update(|snapshot| {
            add(
                &mut snapshot.provenance_lookups,
                1,
                &mut snapshot.overflowed,
            );
        });
    }

    pub(super) fn add_member(outcome: &str) {
        update(|snapshot| {
            add(&mut snapshot.add_members, 1, &mut snapshot.overflowed);
            let slot = match outcome {
                "upserted" => &mut snapshot.add_upserted,
                "refused" => &mut snapshot.add_refused,
                "failed" => &mut snapshot.add_failed,
                "uncertain" => &mut snapshot.add_uncertain,
                "not-attempted" => &mut snapshot.add_not_attempted,
                _ => return,
            };
            add(slot, 1, &mut snapshot.overflowed);
        });
    }

    pub(super) fn owned_live_bytes(amount: usize) {
        update(|snapshot| match u64::try_from(amount) {
            Ok(amount) => {
                snapshot.owned_live_bytes_high_water =
                    snapshot.owned_live_bytes_high_water.max(amount);
            }
            Err(_) => snapshot.overflowed = true,
        });
    }
}

#[cfg(feature = "benchmark-instrumentation")]
pub use active::{Collector, Snapshot};

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_source_bytes(amount: usize) {
    active::source_bytes(amount);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_source_bytes(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_sliced_spans(amount: usize) {
    active::sliced_spans(amount);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_sliced_spans(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_embedding_call(input_bytes: usize) {
    active::embedding_call(input_bytes);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_embedding_call(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_embedding_vectors(amount: usize) {
    active::embedding_vectors(amount);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_embedding_vectors(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_write_transaction() {
    active::write_transaction();
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_write_transaction() {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_read_transaction() {
    active::read_transaction();
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_read_transaction() {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_search_document(span_count: usize) {
    active::search_document(span_count);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_search_document(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_search_hit(bytes: usize) {
    active::search_hit(bytes);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_search_hit(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_neighbour_passage(bytes: usize) {
    active::neighbour_passage(bytes);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_neighbour_passage(_: usize) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_provenance_lookup() {
    active::provenance_lookup();
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_provenance_lookup() {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_add_member(outcome: &str) {
    active::add_member(outcome);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_add_member(_: &str) {}

#[cfg(feature = "benchmark-instrumentation")]
pub(crate) fn record_owned_live_bytes(amount: usize) {
    active::owned_live_bytes(amount);
}

#[cfg(not(feature = "benchmark-instrumentation"))]
pub(crate) const fn record_owned_live_bytes(_: usize) {}

#[cfg(all(test, feature = "benchmark-instrumentation"))]
mod tests {
    use super::{
        Collector, record_add_member, record_embedding_call, record_embedding_vectors,
        record_neighbour_passage, record_owned_live_bytes, record_provenance_lookup,
        record_read_transaction, record_search_document, record_search_hit, record_sliced_spans,
        record_source_bytes, record_write_transaction,
    };

    #[test]
    fn a_collection_starts_empty_and_records_owned_boundaries() {
        let collector = Collector::start();
        record_source_bytes(12);
        record_sliced_spans(3);
        record_embedding_call(20);
        record_embedding_vectors(3);
        record_write_transaction();
        record_read_transaction();
        record_search_document(3);
        record_search_hit(10);
        record_neighbour_passage(4);
        record_provenance_lookup();
        record_add_member("upserted");
        record_owned_live_bytes(99);
        let snapshot = collector.finish();

        assert_eq!(snapshot.source_bytes(), 12);
        assert_eq!(snapshot.sliced_spans(), 3);
        assert_eq!(snapshot.embedding_calls(), 1);
        assert_eq!(snapshot.embedding_input_bytes(), 20);
        assert_eq!(snapshot.embedding_vectors(), 3);
        assert_eq!(snapshot.write_transactions(), 1);
        assert_eq!(snapshot.read_transactions(), 1);
        assert_eq!(snapshot.search_documents(), 1);
        assert_eq!(snapshot.search_spans(), 3);
        assert_eq!(snapshot.search_hits(), 1);
        assert_eq!(snapshot.search_output_bytes(), 10);
        assert_eq!(snapshot.neighbour_passages(), 1);
        assert_eq!(snapshot.neighbour_output_bytes(), 4);
        assert_eq!(snapshot.provenance_lookups(), 1);
        assert_eq!(snapshot.add_members(), 1);
        assert_eq!(snapshot.add_upserted(), 1);
        assert_eq!(snapshot.owned_live_bytes_high_water(), 99);
        assert!(!snapshot.overflowed());
    }

    #[test]
    fn nested_collections_restore_the_outer_collection() {
        let outer = Collector::start();
        record_source_bytes(1);
        let inner = Collector::start();
        record_source_bytes(2);
        let inner_snapshot = inner.finish();
        assert_eq!(inner_snapshot.source_bytes(), 2);
        record_source_bytes(4);
        let outer_snapshot = outer.finish();
        assert_eq!(outer_snapshot.source_bytes(), 5);
    }

    #[test]
    fn counter_overflow_is_reported_in_the_snapshot() {
        let collector = Collector::start();
        record_source_bytes(usize::MAX);
        record_source_bytes(1);
        let snapshot = collector.finish();
        assert!(snapshot.overflowed());
    }
}
