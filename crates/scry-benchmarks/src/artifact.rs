use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use grimoire::{Address, CostExpression, CostModel};
use scry::BenchmarkSnapshot;
use serde::Serialize;
use serde_json::{Value, json};

use crate::bridge::{
    BridgeConfig, GRIMOIRE_REVISION, GRIMOIRE_SOURCE, MODEL_DIMENSION, MODEL_LIMIT, MODEL_NAME,
    MODEL_REPOSITORY, MODEL_REVISION, StaticModel, WINDOW,
};

pub(crate) const ARTIFACT_SCHEMA: &str = "scry-benchmark-v1";
pub(crate) const COST_MODEL_VERSION: &str = "cost-model-v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArtifactError {
    message: String,
}

#[derive(Serialize)]
pub(crate) struct BaselineArtifact {
    schema: &'static str,
    identity: Identity,
    static_analysis: StaticAnalysis,
    runtime: RuntimeAnalysis,
}

#[derive(Serialize)]
struct Identity {
    workload_id: &'static str,
    input_fingerprint: String,
    model: ModelIdentity,
    grimoire: GrimoireIdentity,
    graph_hash: String,
    canonical_description_hash: String,
    cost_model_version: &'static str,
    axes: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct ModelIdentity {
    name: &'static str,
    repository: &'static str,
    revision: &'static str,
    dimension: u64,
    limit: u64,
    window: u64,
}

#[derive(Serialize)]
struct GrimoireIdentity {
    source: &'static str,
    revision: &'static str,
    package: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
struct StaticAnalysis {
    canonical_description: String,
    structural: StructuralCounts,
    operator_counts: BTreeMap<String, u64>,
    cost_reports: Vec<CostReportArtifact>,
}

#[derive(Serialize)]
struct StructuralCounts {
    onnx_nodes: u64,
    grimoire_blocks: u64,
    grimoire_connections: u64,
    grimoire_groups: u64,
}

#[derive(Serialize)]
struct CostReportArtifact {
    name: &'static str,
    unit: &'static str,
    target: String,
    total: u64,
    assignments: Vec<CostAssignment>,
}

#[derive(Serialize)]
struct CostAssignment {
    address: String,
    expression: Value,
    value: u64,
}

#[derive(Serialize)]
struct RuntimeAnalysis {
    basis: &'static str,
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
    owned_logical_bytes_high_water: u64,
}

pub(crate) fn build_artifact(
    model: &StaticModel,
    config: BridgeConfig,
    input_fingerprint: String,
    snapshot: &BenchmarkSnapshot,
) -> Result<BaselineArtifact, ArtifactError> {
    if snapshot.overflowed() {
        return Err(error("runtime benchmark counter overflowed"));
    }
    if config.batch_size != 1 || config.sequence_length != 32 {
        return Err(error(
            "v1 baseline requires batch size 1 and sequence length 32",
        ));
    }
    let reprojection = grimoire::evaluate_layer(&model.description, "cost")
        .map_err(|err| error(err.to_string()))?;
    let reports = vec![
        cost_report(
            "macs",
            "macs",
            &model.macs,
            &reprojection.structural,
            &model.matmul_group,
            &model.axes,
        )?,
        cost_report(
            "fma_flops",
            "fma-flops",
            &model.fma_flops,
            &reprojection.structural,
            &model.matmul_group,
            &model.axes,
        )?,
    ];
    let axes = model
        .axes
        .iter()
        .map(|(address, extent)| (address.to_string(), *extent))
        .collect();
    let onnx_nodes = model
        .operator_counts
        .values()
        .try_fold(0u64, |total, count| total.checked_add(*count))
        .ok_or_else(|| error("ONNX node count overflowed"))?;
    let structural = StructuralCounts {
        onnx_nodes,
        grimoire_blocks: count(model.description.core.blocks.len())?,
        grimoire_connections: count(model.description.core.connections.len())?,
        grimoire_groups: count(model.description.core.groups.len())?,
    };
    Ok(BaselineArtifact {
        schema: ARTIFACT_SCHEMA,
        identity: Identity {
            workload_id: "bge-small-onnx-b1-s32",
            input_fingerprint,
            model: ModelIdentity {
                name: MODEL_NAME,
                repository: MODEL_REPOSITORY,
                revision: MODEL_REVISION,
                dimension: MODEL_DIMENSION,
                limit: MODEL_LIMIT,
                window: WINDOW,
            },
            grimoire: GrimoireIdentity {
                source: GRIMOIRE_SOURCE,
                revision: GRIMOIRE_REVISION,
                package: "grimoire",
                version: "1.0.0",
            },
            graph_hash: model.raw_graph_hash.clone(),
            canonical_description_hash: model.canonical_hash.clone(),
            cost_model_version: COST_MODEL_VERSION,
            axes,
        },
        static_analysis: StaticAnalysis {
            canonical_description: model.canonical.clone(),
            structural,
            operator_counts: model.operator_counts.clone(),
            cost_reports: reports,
        },
        runtime: RuntimeAnalysis {
            basis: "private scry runtime boundary observations; no elapsed time, RSS, hardware counters, or runtime FLOP counts",
            source_bytes: snapshot.source_bytes(),
            sliced_spans: snapshot.sliced_spans(),
            embedding_calls: snapshot.embedding_calls(),
            embedding_input_bytes: snapshot.embedding_input_bytes(),
            embedding_vectors: snapshot.embedding_vectors(),
            write_transactions: snapshot.write_transactions(),
            read_transactions: snapshot.read_transactions(),
            search_documents: snapshot.search_documents(),
            search_spans: snapshot.search_spans(),
            search_hits: snapshot.search_hits(),
            search_output_bytes: snapshot.search_output_bytes(),
            neighbour_passages: snapshot.neighbour_passages(),
            neighbour_output_bytes: snapshot.neighbour_output_bytes(),
            provenance_lookups: snapshot.provenance_lookups(),
            add_members: snapshot.add_members(),
            add_upserted: snapshot.add_upserted(),
            add_refused: snapshot.add_refused(),
            add_failed: snapshot.add_failed(),
            add_uncertain: snapshot.add_uncertain(),
            add_not_attempted: snapshot.add_not_attempted(),
            owned_logical_bytes_high_water: snapshot.owned_logical_bytes_high_water(),
        },
    })
}

pub(crate) fn write_artifact(
    artifact: &BaselineArtifact,
    path: &Path,
) -> Result<(), ArtifactError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| error("artifact output has no parent"))?;
    fs::create_dir_all(parent).map_err(|err| error(err.to_string()))?;
    let text = serialize_artifact(artifact)?;
    fs::write(path, text).map_err(|err| error(err.to_string()))
}

pub(crate) fn check_artifact(
    artifact: &BaselineArtifact,
    path: &Path,
) -> Result<(), ArtifactError> {
    let expected = fs::read_to_string(path).map_err(|err| error(err.to_string()))?;
    let actual = serialize_artifact(artifact)?;
    if expected != actual {
        let expected_lines = expected.lines().collect::<Vec<_>>();
        let actual_lines = actual.lines().collect::<Vec<_>>();
        let line = expected_lines
            .iter()
            .zip(&actual_lines)
            .position(|(expected, actual)| expected != actual)
            .unwrap_or(expected_lines.len().min(actual_lines.len()));
        let expected_line = expected_lines.get(line).copied().unwrap_or("<end of file>");
        let actual_line = actual_lines.get(line).copied().unwrap_or("<end of file>");
        return Err(error(format!(
            "baseline `{}` differs from regenerated v1 artifact at line {}\n- {}\n+ {}",
            path.display(),
            line + 1,
            expected_line,
            actual_line,
        )));
    }
    Ok(())
}

fn serialize_artifact(artifact: &BaselineArtifact) -> Result<String, ArtifactError> {
    let mut text = serde_json::to_string_pretty(artifact).map_err(|err| error(err.to_string()))?;
    text.push('\n');
    Ok(text)
}

fn cost_report(
    name: &'static str,
    unit: &'static str,
    model: &CostModel,
    reprojection: &grimoire::StructuralReprojection,
    target: &Address,
    axes: &BTreeMap<Address, u64>,
) -> Result<CostReportArtifact, ArtifactError> {
    let report = model
        .evaluate(reprojection, axes)
        .map_err(|err| error(err.to_string()))?;
    let total = report
        .group_total(reprojection, target)
        .map_err(|err| error(err.to_string()))?;
    let Some(grimoire::Element::Group(group)) = reprojection.elements.get(target) else {
        return Err(error("cost target is not a visible group"));
    };
    let mut assignments = Vec::new();
    for address in &group.members {
        let Some(expression) = model.expression(address) else {
            return Err(error(format!("cost assignment is missing for `{address}`")));
        };
        let Some(value) = report.value(address) else {
            return Err(error(format!("cost report is missing `{address}`")));
        };
        assignments.push(CostAssignment {
            address: address.to_string(),
            expression: expression_json(expression),
            value,
        });
    }
    Ok(CostReportArtifact {
        name,
        unit,
        target: target.to_string(),
        total,
        assignments,
    })
}

fn expression_json(expression: &CostExpression) -> Value {
    match expression {
        CostExpression::Constant(value) => json!({ "kind": "constant", "value": value }),
        CostExpression::Axis(address) => json!({ "kind": "axis", "address": address.to_string() }),
        CostExpression::Sum(terms) => json!({
            "kind": "sum",
            "terms": terms.iter().map(expression_json).collect::<Vec<_>>(),
        }),
        CostExpression::Product(factors) => json!({
            "kind": "product",
            "factors": factors.iter().map(expression_json).collect::<Vec<_>>(),
        }),
    }
}

fn count(value: usize) -> Result<u64, ArtifactError> {
    u64::try_from(value).map_err(|_| error("structural count overflowed"))
}

fn error(message: impl Into<String>) -> ArtifactError {
    ArtifactError {
        message: message.into(),
    }
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ArtifactError {}
