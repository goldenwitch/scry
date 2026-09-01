//! Deterministic large-origin memory diagnostics.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hmac_sha256::Hash;
use scry::{
    AddOutcome, BenchmarkCollector, DEFAULT_PASSAGE_MICROBATCH_SIZE, Embed, Origin, Slice, Store,
    Text,
};

use crate::native_memory::{MaximumObservedMemory, ResidentMemoryKind, Sampler};

pub(crate) const SCALE_ID: &str = "large-single-origin-v1";
pub(crate) const FRAGMENT_REPETITIONS: usize = 2_400;
pub(crate) const SCALE_TOKENS: u64 = 189_600;
pub(crate) const SCALE_SPANS: u64 = 747;
const FRAGMENT: &str = "<section data-paper=\"scry\"><p>The tide pool holds anemones and limpets after the sea draws back, and the shallow water warms until the flood returns.</p><p>The locomotive raises boiler pressure until the safety valve lifts, and the fireman shovels coal against the long gradient.</p></section>\n";
const FASTEMBED_DEFAULT_BATCH_SIZE: usize = 256;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(10);
const TTL: Duration = Duration::from_secs(3600);

pub(crate) struct FixtureFacts {
    pub(crate) body: String,
    pub(crate) source_bytes: u64,
    pub(crate) tokens: u64,
    pub(crate) spans: u64,
    pub(crate) padded_sequence_length: u64,
}

struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn new() -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("scry-scale-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).map_err(|error| error.to_string())?;
        Ok(Self { path })
    }

    fn origin(&self, body: &str) -> Result<Origin, String> {
        let path = self.path.join("large-origin.html");
        fs::write(&path, body).map_err(|error| error.to_string())?;
        let spelling = path
            .to_str()
            .ok_or_else(|| "scale fixture path is not UTF-8".to_owned())?;
        Origin::parse(spelling)
            .ok_or_else(|| format!("scale fixture path is not an origin: {spelling}"))
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

pub(crate) fn run(
    cache: &Path,
    microbatch_size: Option<usize>,
    cpu_arena: Option<bool>,
) -> Result<(), String> {
    let body = fixture();
    let workspace = Workspace::new()?;
    let configured_microbatch_size = microbatch_size.unwrap_or(DEFAULT_PASSAGE_MICROBATCH_SIZE);
    let mut embed = match cpu_arena {
        Some(cpu_arena) => Embed::load_with_passage_microbatch_size_and_cpu_arena(
            cache,
            configured_microbatch_size,
            Some(cpu_arena),
        ),
        None => Embed::load_with_passage_microbatch_size(cache, configured_microbatch_size),
    }
    .map_err(|error| error.to_string())?;
    let slice = Slice::new(&embed).map_err(|error| error.to_string())?;
    let origin = workspace.origin(&body)?;
    let store = match Store::open(&workspace.store_path(), embed.model()) {
        Ok(Ok(store)) => store,
        Ok(Err(error)) => return Err(error.to_string()),
        Err(error) => return Err(error.to_string()),
    };

    let origins = BTreeSet::from([origin.clone()]);
    let collector = BenchmarkCollector::start();
    let sampler = Sampler::start(SAMPLE_INTERVAL)?;
    let report = store.add(&mut embed, &slice, &origins, TTL);
    let native = sampler.finish()?;
    let snapshot = collector.finish();
    let Some(result) = report.items().first() else {
        return Err("scale add report was empty".to_owned());
    };
    if !matches!(result.outcome(), AddOutcome::Upserted) {
        return Err(format!("scale add returned {:?}", result.outcome()));
    }
    let final_embedding_bytes = validate_snapshot(&body, &snapshot, configured_microbatch_size)?;
    println!(
        "scale={} fingerprint={} source_bytes={} tokens={} spans={} configured_microbatch_size={} cpu_arena={} embed_api_calls={} fastembed_batch_size={} fastembed_batches={} final_embedding_vectors={} final_embedding_bytes={} write_transactions={} add_outcome=upserted native_samples={} max_observed_resident_bytes={} resident_metric={} max_observed_secondary_bytes={} secondary_metric={} sample_interval_ms={} native_basis=process-local diagnostic; setup and model loading excluded",
        SCALE_ID,
        fingerprint(&body),
        snapshot.source_bytes(),
        snapshot.sliced_tokens(),
        snapshot.sliced_spans(),
        configured_microbatch_size,
        cpu_arena.map_or("default", |enabled| if enabled { "on" } else { "off" }),
        snapshot.embedding_calls(),
        FASTEMBED_DEFAULT_BATCH_SIZE,
        snapshot.embedding_batches(),
        snapshot.embedding_vectors(),
        final_embedding_bytes,
        snapshot.write_transactions(),
        native.samples,
        native.resident_bytes,
        resident_metric(&native),
        native.secondary_bytes,
        secondary_metric(&native),
        SAMPLE_INTERVAL.as_millis(),
    );
    Ok(())
}

fn resident_metric(native: &MaximumObservedMemory) -> &'static str {
    match native.resident_kind {
        Some(ResidentMemoryKind::WorkingSet) => "working_set_bytes",
        Some(ResidentMemoryKind::ResidentSet) => "resident_set_bytes",
        None => "unsupported",
    }
}

fn validate_snapshot(
    body: &str,
    snapshot: &scry::BenchmarkSnapshot,
    configured_microbatch_size: usize,
) -> Result<u64, String> {
    if snapshot.source_bytes() != body.len() as u64
        || snapshot.sliced_tokens() != SCALE_TOKENS
        || snapshot.sliced_spans() != SCALE_SPANS
        || snapshot.embedding_vectors() != snapshot.sliced_spans()
        || snapshot.write_transactions() != 1
        || snapshot.add_members() != 1
        || snapshot.add_upserted() != 1
        || snapshot.add_refused() != 0
        || snapshot.add_failed() != 0
        || snapshot.add_uncertain() != 0
        || snapshot.add_not_attempted() != 0
        || snapshot.read_transactions() != 0
        || snapshot.overflowed()
    {
        return Err(format!(
            "scale runtime observations did not match: source_bytes={} expected={} tokens={} spans={} embedding_calls={} vectors={} writes={} members={} upserted={} refused={} failed={} uncertain={} not_attempted={} reads={} overflowed={}",
            snapshot.source_bytes(),
            body.len(),
            snapshot.sliced_tokens(),
            snapshot.sliced_spans(),
            snapshot.embedding_calls(),
            snapshot.embedding_vectors(),
            snapshot.write_transactions(),
            snapshot.add_members(),
            snapshot.add_upserted(),
            snapshot.add_refused(),
            snapshot.add_failed(),
            snapshot.add_uncertain(),
            snapshot.add_not_attempted(),
            snapshot.read_transactions(),
            snapshot.overflowed(),
        ));
    }
    let final_embedding_bytes = snapshot
        .embedding_vectors()
        .checked_mul(crate::bridge::MODEL_DIMENSION)
        .and_then(|bytes| bytes.checked_mul(4))
        .ok_or_else(|| "scale final embedding byte count overflowed".to_owned())?;
    let span_count = usize::try_from(snapshot.sliced_spans())
        .map_err(|_| "scale span count does not fit usize".to_owned())?;
    let expected_calls = span_count.div_ceil(configured_microbatch_size);
    let actual_calls = usize::try_from(snapshot.embedding_calls())
        .map_err(|_| "scale embedding call count does not fit usize".to_owned())?;
    let actual_batches = usize::try_from(snapshot.embedding_batches())
        .map_err(|_| "scale embedding batch count does not fit usize".to_owned())?;
    let expected_max_call_size = span_count.min(configured_microbatch_size);
    let actual_max_call_size = usize::try_from(snapshot.max_embedding_call_size())
        .map_err(|_| "scale maximum embedding call size does not fit usize".to_owned())?;
    if actual_calls != expected_calls
        || actual_batches != expected_calls
        || actual_max_call_size != expected_max_call_size
    {
        return Err(format!(
            "scale embedding calls did not match the configured microbatch: calls={actual_calls} batches={actual_batches} max_call_size={actual_max_call_size} expected_calls={expected_calls} expected_max_call_size={expected_max_call_size}"
        ));
    }
    Ok(final_embedding_bytes)
}

pub(crate) fn fixture_facts(cache: &Path) -> Result<FixtureFacts, String> {
    let body = fixture();
    let embed = Embed::load(cache).map_err(|error| error.to_string())?;
    let slice = Slice::new(&embed).map_err(|error| error.to_string())?;
    let text = Text::from(body.clone());
    let (tokens, spans) = slice
        .benchmark_counts(&text)
        .map_err(|error| error.to_string())?;
    let passages = slice
        .benchmark_passages(&text)
        .map_err(|error| error.to_string())?;
    let padded_sequence_length = embed
        .benchmark_padded_sequence_length(&passages)
        .map_err(|error| error.to_string())?;
    Ok(FixtureFacts {
        source_bytes: u64::try_from(body.len())
            .map_err(|_| "scale fixture byte count overflowed".to_owned())?,
        tokens: u64::try_from(tokens)
            .map_err(|_| "scale fixture token count overflowed".to_owned())?,
        spans: u64::try_from(spans)
            .map_err(|_| "scale fixture span count overflowed".to_owned())?,
        padded_sequence_length: u64::try_from(padded_sequence_length)
            .map_err(|_| "scale fixture padded sequence length overflowed".to_owned())?,
        body,
    })
}

pub(crate) fn fixture() -> String {
    let mut body = String::with_capacity(FRAGMENT.len() * FRAGMENT_REPETITIONS);
    for _ in 0..FRAGMENT_REPETITIONS {
        body.push_str(FRAGMENT);
    }
    body
}

pub(crate) fn fingerprint(body: &str) -> String {
    let material = format!("{SCALE_ID}\nrepetitions={FRAGMENT_REPETITIONS}\n{body}");
    let digest = Hash::hash(material.as_bytes());
    let mut result = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;

        let _ = write!(result, "{byte:02x}");
    }
    result
}

fn secondary_metric(native: &MaximumObservedMemory) -> &'static str {
    match native.secondary_kind {
        Some(crate::native_memory::SecondaryMemoryKind::Private) => "private_bytes",
        Some(crate::native_memory::SecondaryMemoryKind::Virtual) => "virtual_memory_bytes",
        None => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::{FRAGMENT, FRAGMENT_REPETITIONS, SCALE_ID, fingerprint, fixture};

    #[test]
    fn the_fixture_and_fingerprint_are_deterministic() {
        let first = fixture();
        let second = fixture();
        assert_eq!(first, second);
        assert_eq!(first.len(), FRAGMENT.len() * FRAGMENT_REPETITIONS);
        assert_eq!(fingerprint(&first), fingerprint(&second));
        assert!(first.starts_with(FRAGMENT));
        assert!(SCALE_ID.contains("large-single-origin"));
    }
}
