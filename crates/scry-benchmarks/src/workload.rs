//! Fixed runtime workloads for the v1 artifact and add-only diagnostics.
//!
//! `run` preserves the mixed workload used by the committed artifact.
//! `run_add_only` is intentionally separate so add-stage measurements cannot
//! be attributed to search, provenance, neighbours, or delete.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hmac_sha256::Hash;
use scry::{
    AddOutcome, BenchmarkCollector, BenchmarkSnapshot, Count, Embed, HandleRefusal, Origin, Query,
    Slice, Store, Text,
};

pub(crate) const WORKLOAD_ID: &str = "bge-small-onnx-b1-s32";
const TTL: Duration = Duration::from_secs(3600);
const POOLS: &str = "The tide pool holds anemones and limpets after the sea draws back, and the shallow water warms until the flood returns.";
const ENGINES: &str = "The locomotive raises boiler pressure until the safety valve lifts, and the fireman shovels coal against the long gradient.";
const QUERY: &str = "what lives in a rock pool while the tide is out";

pub(crate) struct WorkloadResult {
    pub(crate) input_fingerprint: String,
    pub(crate) snapshot: BenchmarkSnapshot,
}

struct Workspace {
    path: PathBuf,
}

struct AddCase {
    id: &'static str,
    input_fingerprint: String,
    snapshot: BenchmarkSnapshot,
}

#[derive(Clone, Copy)]
struct ExpectedCounts {
    source_bytes: u64,
    sliced_spans: u64,
    embedding_calls: u64,
    embedding_input_bytes: u64,
    embedding_vectors: u64,
    write_transactions: u64,
    add_members: u64,
    add_upserted: u64,
    add_refused: u64,
}

impl Workspace {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("scry-benchmark-v1-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    fn origin(&self, name: &str, body: &str) -> Result<Origin, String> {
        let path = self.path.join(name);
        fs::write(&path, body).map_err(|error| error.to_string())?;
        origin_for_path(&path)
    }

    fn missing_origin(&self) -> Result<Origin, String> {
        origin_for_path(&self.path.join("03-missing.md"))
    }

    fn store_path(&self) -> PathBuf {
        self.path.join("corpus.redb")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub(crate) fn run_add_only(cache: &Path) -> Result<(), String> {
    // Load once so model startup is outside every case; each case creates its
    // own local store and starts collection only after that setup is complete.
    let mut embed = Embed::load(cache).map_err(|error| error.to_string())?;
    let slice = Slice::new(&embed).map_err(|error| error.to_string())?;
    let cases = vec![
        empty_case(&mut embed, &slice)?,
        short_singleton(&mut embed, &slice)?,
        multi_span_singleton(&mut embed, &slice)?,
        valid_multi_member(&mut embed, &slice)?,
        refused_member(&mut embed, &slice)?,
    ];
    println!("add-only diagnostics (warm model cache, setup excluded)");
    for case in cases {
        print_add_case(&case);
    }
    Ok(())
}

fn empty_case(embed: &mut Embed, slice: &Slice) -> Result<AddCase, String> {
    let workspace = Workspace::new()?;
    let origins = BTreeSet::new();
    collect_add_case(
        &workspace,
        embed,
        slice,
        "empty-set-v1",
        "empty-set-v1",
        &origins,
        ExpectedCounts {
            source_bytes: 0,
            sliced_spans: 0,
            embedding_calls: 0,
            embedding_input_bytes: 0,
            embedding_vectors: 0,
            write_transactions: 0,
            add_members: 0,
            add_upserted: 0,
            add_refused: 0,
        },
    )
}

fn short_singleton(embed: &mut Embed, slice: &Slice) -> Result<AddCase, String> {
    let workspace = Workspace::new()?;
    let body = "the kettle boiled beside the quiet window";
    let body_bytes = bytes(body)?;
    let origin = workspace.origin("01-short.md", body)?;
    let origins = BTreeSet::from([origin]);
    collect_add_case(
        &workspace,
        embed,
        slice,
        "short-singleton-v1",
        &format!("short-singleton-v1\n01-short.md\n{body}"),
        &origins,
        ExpectedCounts {
            source_bytes: body_bytes,
            sliced_spans: 1,
            embedding_calls: 1,
            embedding_input_bytes: body_bytes,
            embedding_vectors: 1,
            write_transactions: 1,
            add_members: 1,
            add_upserted: 1,
            add_refused: 0,
        },
    )
}

fn multi_span_singleton(embed: &mut Embed, slice: &Slice) -> Result<AddCase, String> {
    let workspace = Workspace::new()?;
    let body = long("The tide pool holds anemones and limpets after the sea draws back.");
    let body_bytes = bytes(&body)?;
    let origin = workspace.origin("01-long.md", &body)?;
    let origins = BTreeSet::from([origin]);
    collect_add_case(
        &workspace,
        embed,
        slice,
        "multi-span-singleton-v1",
        &format!("multi-span-singleton-v1\n01-long.md\n{body}"),
        &origins,
        ExpectedCounts {
            source_bytes: body_bytes,
            sliced_spans: 3,
            embedding_calls: 1,
            embedding_input_bytes: body_bytes,
            embedding_vectors: 3,
            write_transactions: 1,
            add_members: 1,
            add_upserted: 1,
            add_refused: 0,
        },
    )
}

fn valid_multi_member(embed: &mut Embed, slice: &Slice) -> Result<AddCase, String> {
    let workspace = Workspace::new()?;
    let first = "the kettle boiled beside the quiet window";
    let second = "the locomotive climbed the long gradient under a clear sky";
    let first_origin = workspace.origin("01-first.md", first)?;
    let second_origin = workspace.origin("02-second.md", second)?;
    let first_length = bytes(first)?;
    let second_length = bytes(second)?;
    let origins = BTreeSet::from([first_origin, second_origin]);
    collect_add_case(
        &workspace,
        embed,
        slice,
        "valid-multi-member-v1",
        &format!("valid-multi-member-v1\n01-first.md\n{first}\n02-second.md\n{second}"),
        &origins,
        ExpectedCounts {
            source_bytes: first_length + second_length,
            sliced_spans: 2,
            embedding_calls: 2,
            embedding_input_bytes: first_length + second_length,
            embedding_vectors: 2,
            write_transactions: 2,
            add_members: 2,
            add_upserted: 2,
            add_refused: 0,
        },
    )
}

fn refused_member(embed: &mut Embed, slice: &Slice) -> Result<AddCase, String> {
    let workspace = Workspace::new()?;
    let body = "the valid document remains searchable";
    let body_bytes = bytes(body)?;
    let valid = workspace.origin("01-valid.md", body)?;
    let missing = workspace.missing_origin()?;
    let origins = BTreeSet::from([valid, missing]);
    collect_add_case(
        &workspace,
        embed,
        slice,
        "refused-member-v1",
        &format!("refused-member-v1\n01-valid.md\n{body}\n03-missing.md"),
        &origins,
        ExpectedCounts {
            source_bytes: body_bytes,
            sliced_spans: 1,
            embedding_calls: 1,
            embedding_input_bytes: body_bytes,
            embedding_vectors: 1,
            write_transactions: 1,
            add_members: 2,
            add_upserted: 1,
            add_refused: 1,
        },
    )
}

fn collect_add_case(
    workspace: &Workspace,
    embed: &mut Embed,
    slice: &Slice,
    id: &'static str,
    fingerprint_material: &str,
    origins: &BTreeSet<Origin>,
    expected: ExpectedCounts,
) -> Result<AddCase, String> {
    // Store initialization, including its stamp transaction, is setup rather
    // than add work and must stay outside the per-case collector.
    let store = match Store::open(&workspace.store_path(), embed.model()) {
        Ok(Ok(store)) => store,
        Ok(Err(error)) => return Err(error.to_string()),
        Err(error) => return Err(error.to_string()),
    };
    let expected_origins = origins.iter().cloned().collect::<Vec<_>>();
    let collector = BenchmarkCollector::start();
    let report = store.add(embed, slice, origins, TTL);
    let snapshot = collector.finish();
    let report_origins = report
        .items()
        .iter()
        .map(|item| item.origin().clone())
        .collect::<Vec<_>>();
    if report_origins != expected_origins {
        return Err(format!("{id} changed canonical report order"));
    }
    let upserted = report
        .items()
        .iter()
        .filter(|item| matches!(item.outcome(), AddOutcome::Upserted))
        .count();
    let refused = report
        .items()
        .iter()
        .filter(|item| matches!(item.outcome(), AddOutcome::Refused(_)))
        .count();
    let upserted = u64::try_from(upserted).map_err(|_| format!("{id} upsert count overflowed"))?;
    let refused = u64::try_from(refused).map_err(|_| format!("{id} refusal count overflowed"))?;
    if snapshot.source_bytes() != expected.source_bytes
        || snapshot.sliced_spans() != expected.sliced_spans
        || snapshot.embedding_calls() != expected.embedding_calls
        || snapshot.embedding_input_bytes() != expected.embedding_input_bytes
        || snapshot.embedding_vectors() != expected.embedding_vectors
        || snapshot.write_transactions() != expected.write_transactions
        || snapshot.add_members() != expected.add_members
        || snapshot.add_upserted() != expected.add_upserted
        || snapshot.add_refused() != expected.add_refused
        || upserted != expected.add_upserted
        || refused != expected.add_refused
        || snapshot.read_transactions() != 0
        || snapshot.overflowed()
    {
        return Err(format!(
            "{id} runtime observations did not match its fixed case: source_bytes={}/{} spans={}/{} embedding_calls={}/{} embedding_input_bytes={}/{} embedding_vectors={}/{} write_transactions={}/{} add_members={}/{} add_upserted={}/{} add_refused={}/{} read_transactions={} overflowed={}",
            snapshot.source_bytes(),
            expected.source_bytes,
            snapshot.sliced_spans(),
            expected.sliced_spans,
            snapshot.embedding_calls(),
            expected.embedding_calls,
            snapshot.embedding_input_bytes(),
            expected.embedding_input_bytes,
            snapshot.embedding_vectors(),
            expected.embedding_vectors,
            snapshot.write_transactions(),
            expected.write_transactions,
            snapshot.add_members(),
            expected.add_members,
            snapshot.add_upserted(),
            expected.add_upserted,
            snapshot.add_refused(),
            expected.add_refused,
            snapshot.read_transactions(),
            snapshot.overflowed(),
        ));
    }
    Ok(AddCase {
        id,
        input_fingerprint: fingerprint_for(fingerprint_material),
        snapshot,
    })
}

fn print_add_case(case: &AddCase) {
    let snapshot = &case.snapshot;
    println!(
        "case={} fingerprint={} source_bytes={} spans={} embedding_calls={} embedding_input_bytes={} embedding_vectors={} write_transactions={} add_members={} add_upserted={} add_refused={} owned_logical_bytes_high_water={} fetch_nanos={} slice_nanos={} embedding_nanos={} record_nanos={} commit_nanos={}",
        case.id,
        case.input_fingerprint,
        snapshot.source_bytes(),
        snapshot.sliced_spans(),
        snapshot.embedding_calls(),
        snapshot.embedding_input_bytes(),
        snapshot.embedding_vectors(),
        snapshot.write_transactions(),
        snapshot.add_members(),
        snapshot.add_upserted(),
        snapshot.add_refused(),
        snapshot.owned_logical_bytes_high_water(),
        snapshot.fetch_nanos(),
        snapshot.slice_nanos(),
        snapshot.embedding_nanos(),
        snapshot.record_nanos(),
        snapshot.commit_nanos(),
    );
}

fn bytes(text: &str) -> Result<u64, String> {
    u64::try_from(text.len()).map_err(|_| "benchmark input is too large".to_owned())
}

pub(crate) fn run(cache: &Path) -> Result<WorkloadResult, String> {
    let workspace = Workspace::new()?;
    let mut embed = Embed::load(cache).map_err(|error| error.to_string())?;
    let slice = Slice::new(&embed).map_err(|error| error.to_string())?;
    let store = match Store::open(&workspace.store_path(), embed.model()) {
        Ok(Ok(store)) => store,
        Ok(Err(error)) => return Err(error.to_string()),
        Err(error) => return Err(error.to_string()),
    };
    let pools = workspace.origin("01-pools.md", &long(POOLS))?;
    let engines = workspace.origin("02-engines.md", &long(ENGINES))?;
    let missing = workspace.missing_origin()?;
    let input_fingerprint = fingerprint();
    let collector = BenchmarkCollector::start();

    let origins = BTreeSet::from([pools.clone(), engines.clone(), missing.clone()]);
    let report = store.add(&mut embed, &slice, &origins, TTL);
    expect_upserted(report.items(), &pools)?;
    expect_upserted(report.items(), &engines)?;
    expect_refused(report.items(), &missing)?;

    let query = Query::new(Text::from(QUERY.to_owned()));
    let count = Count::new(3).ok_or_else(|| "benchmark search count is zero".to_owned())?;
    let hits = store
        .search(&mut embed, &query, count)
        .map_err(|error| error.to_string())?;
    let hit = hits
        .iter()
        .find(|hit| hit.passage().handle().chunk().origin() == &pools)
        .ok_or_else(|| "known answer was absent from the search result".to_owned())?;
    let handle = hit.passage().handle().clone();
    let provenance = match store
        .provenance(&handle)
        .map_err(|error| error.to_string())?
    {
        Ok(provenance) => provenance,
        Err(refusal) => return Err(refusal.to_string()),
    };
    if provenance.chunk().origin() != &pools {
        return Err("provenance returned the wrong origin".to_owned());
    }
    let neighbour_count =
        Count::new(2).ok_or_else(|| "benchmark neighbour count is zero".to_owned())?;
    let neighbours = match store
        .neighbours(&handle, neighbour_count)
        .map_err(|error| error.to_string())?
    {
        Ok(passages) => passages,
        Err(refusal) => return Err(refusal.to_string()),
    };
    if neighbours.len() != 2 {
        return Err(format!(
            "known answer document returned {} neighbours",
            neighbours.len()
        ));
    }

    store.delete(&pools).map_err(|error| error.to_string())?;
    match store
        .provenance(&handle)
        .map_err(|error| error.to_string())?
    {
        Ok(_) => return Err("deleted handle still has provenance".to_owned()),
        Err(HandleRefusal::Gone(origin)) if origin == pools => {}
        Err(refusal) => return Err(format!("deleted handle returned {refusal}")),
    }

    let snapshot = collector.finish();
    if snapshot.add_members() != 3
        || snapshot.add_upserted() != 2
        || snapshot.add_refused() != 1
        || snapshot.add_failed() != 0
        || snapshot.add_uncertain() != 0
        || snapshot.add_not_attempted() != 0
    {
        return Err("runtime add outcome counters did not match the fixed workload".to_owned());
    }
    Ok(WorkloadResult {
        input_fingerprint,
        snapshot,
    })
}

fn expect_upserted(items: &[scry::AddResult], origin: &Origin) -> Result<(), String> {
    let result = items
        .iter()
        .find(|result| result.origin() == origin)
        .ok_or_else(|| format!("add report has no item for {origin}"))?;
    if matches!(result.outcome(), AddOutcome::Upserted) {
        Ok(())
    } else {
        Err(format!("add of {origin} returned {:?}", result.outcome()))
    }
}

fn expect_refused(items: &[scry::AddResult], origin: &Origin) -> Result<(), String> {
    let result = items
        .iter()
        .find(|result| result.origin() == origin)
        .ok_or_else(|| format!("add report has no item for {origin}"))?;
    if matches!(result.outcome(), AddOutcome::Refused(_)) {
        Ok(())
    } else {
        Err(format!(
            "missing origin {origin} returned {:?}",
            result.outcome()
        ))
    }
}

fn origin_for_path(path: &Path) -> Result<Origin, String> {
    let spelling = path
        .to_str()
        .ok_or_else(|| "benchmark path is not UTF-8".to_owned())?;
    Origin::parse(spelling)
        .ok_or_else(|| format!("benchmark path is not a valid origin: {spelling}"))
}

fn long(sentence: &str) -> String {
    let mut text = String::new();
    for paragraph in 1..=40 {
        text.push_str("Paragraph ");
        text.push_str(&paragraph.to_string());
        text.push_str(". ");
        text.push_str(sentence);
        text.push('\n');
    }
    text
}

fn fingerprint() -> String {
    let material = format!(
        "{WORKLOAD_ID}\n{POOLS}\n{ENGINES}\n{QUERY}\nB=1\nS=32\nTTL={}\nneighbours=2\nsearch=3",
        TTL.as_secs()
    );
    fingerprint_for(&material)
}

fn fingerprint_for(material: &str) -> String {
    let digest = Hash::hash(material.as_bytes());
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;

        let _ = write!(result, "{byte:02x}");
    }
    result
}
