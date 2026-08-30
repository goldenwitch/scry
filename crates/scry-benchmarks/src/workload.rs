use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hmac_sha256::Hash;
use scry::{
    AddOutcome, BenchmarkCollector, BenchmarkSnapshot, Count, Embed, HandleRefusal, Origin, Query,
    Slice, Store, Text,
};

const WORKLOAD_ID: &str = "scry-runtime-v1";
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
    let digest = Hash::hash(material.as_bytes());
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;

        let _ = write!(result, "{byte:02x}");
    }
    result
}
