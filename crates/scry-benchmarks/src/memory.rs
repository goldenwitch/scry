//! Shape-aware logical memory projections for the large-origin workload.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use grimoire::{
    Address, CostExpression, CostModel, ResourceBundle, ResourceCharge, ResourceKind,
    ResourceModel, ResourceScenario, StructuralReprojection, evaluate_layer,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::bridge::{
    BridgeConfig, GRIMOIRE_REVISION, GRIMOIRE_SOURCE, MODEL_DIMENSION, MODEL_LIMIT, MODEL_NAME,
    MODEL_REPOSITORY, MODEL_REVISION, MemoryFacts, StaticModel, build_static_model,
};
use crate::scale::{self, FixtureFacts};

const SCHEMA: &str = "scry-memory-benchmark-v1";
const PROJECTION_VERSION: &str = "memory-projection-v1";
const FASTEMBED_BATCH_SIZE: u64 = 256;
const SPAN_AXIS: &str = "@scry/benchmark/axis-span-count";
const INPUT_TARGET: &str = "@scry/bge/input-000";
const OUTPUT_TARGET: &str = "@scry/bge/output-000";

const SCENARIOS: [Scenario; 4] = [
    Scenario {
        name: "microbatch-256",
        microbatch_spans: 256,
        assumption: "one Scry call processes at most 256 spans before pooled outputs are released",
    },
    Scenario {
        name: "microbatch-64",
        microbatch_spans: 64,
        assumption: "one Scry call processes at most 64 spans before pooled outputs are released",
    },
    Scenario {
        name: "microbatch-32",
        microbatch_spans: 32,
        assumption: "one Scry call processes at most 32 spans before pooled outputs are released",
    },
    Scenario {
        name: "microbatch-16",
        microbatch_spans: 16,
        assumption: "one Scry call processes at most 16 spans before pooled outputs are released",
    },
];

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    microbatch_spans: u64,
    assumption: &'static str,
}

#[derive(Serialize)]
struct MemoryArtifact {
    schema: &'static str,
    projection_version: &'static str,
    identity: Identity,
    scenarios: Vec<ScenarioArtifact>,
    projections: Vec<ProjectionArtifact>,
    resource_charges: Vec<ResourceChargeArtifact>,
}

#[derive(Serialize)]
struct Identity {
    workload_id: &'static str,
    input_fingerprint: String,
    source_bytes: u64,
    tokenizer_tokens: u64,
    spans: u64,
    fastembed_default_batch_size: u64,
    model: ModelIdentity,
    grimoire: GrimoireIdentity,
    graph_hash: String,
    canonical_description_hash: String,
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
struct ScenarioArtifact {
    name: &'static str,
    microbatch_spans: u64,
    inferred_api_calls: u64,
    padded_sequence_length: u64,
    assumption: &'static str,
}

#[derive(Serialize)]
struct ProjectionArtifact {
    name: &'static str,
    unit: &'static str,
    resource_kind: &'static str,
    target: String,
    basis: &'static str,
    expression: Value,
    values: Vec<ProjectionValue>,
}

#[derive(Serialize)]
struct ProjectionValue {
    scenario: &'static str,
    quantity: u64,
}

#[derive(Serialize)]
struct ResourceChargeArtifact {
    projection: &'static str,
    scenario: &'static str,
    target: String,
    resource: &'static str,
    quantity: u64,
}

struct ProjectionDefinition {
    name: &'static str,
    target: Address,
    basis: &'static str,
    expression: CostExpression,
}

pub(crate) fn run(cache: &Path, output: &Path, check: Option<&Path>) -> Result<(), String> {
    let model_path = cache.join(crate::bridge::MODEL_REVISION).join("model.onnx");
    let model_bytes = fs::read(&model_path).map_err(|error| {
        format!(
            "cannot read pinned model at {}: {error}",
            model_path.display()
        )
    })?;
    let config = BridgeConfig::new(1, 32).map_err(|error| error.to_string())?;
    let static_model =
        build_static_model(&model_bytes, config).map_err(|error| error.to_string())?;
    let fixture = scale::fixture_facts(cache)?;
    let artifact = build_artifact(&static_model, &fixture)?;
    if let Some(path) = check {
        check_artifact(&artifact, path)?;
        println!("memory baseline matches {}", path.display());
    } else {
        write_artifact(&artifact, output)?;
        println!("wrote {}", output.display());
    }
    Ok(())
}

fn build_artifact(
    static_model: &StaticModel,
    fixture: &FixtureFacts,
) -> Result<MemoryArtifact, String> {
    let finalized =
        evaluate_layer(&static_model.description, "cost").map_err(|error| error.to_string())?;
    let reprojection = &finalized.structural;
    let batch_axis = static_model
        .axes_by_symbol
        .get(&static_model.memory.output_batch_symbol)
        .cloned()
        .ok_or_else(|| "the static model has no batch-size axis".to_owned())?;
    let sequence_axis = static_model
        .axes_by_symbol
        .get(&static_model.memory.output_sequence_symbol)
        .cloned()
        .ok_or_else(|| "the static model has no sequence-length axis".to_owned())?;
    let span_axis = Address::parse(SPAN_AXIS).map_err(|error| error.to_string())?;
    let input_target = Address::parse(INPUT_TARGET).map_err(|error| error.to_string())?;
    let output_target = Address::parse(OUTPUT_TARGET).map_err(|error| error.to_string())?;
    ensure_target(reprojection, &input_target)?;
    ensure_target(reprojection, &output_target)?;

    let definitions = definitions(
        batch_axis.clone(),
        sequence_axis.clone(),
        span_axis.clone(),
        input_target,
        output_target,
        &static_model.memory,
    );

    let mut axes = static_model.axes.clone();
    axes.insert(span_axis, fixture.spans);
    let scenarios = scenarios(
        axes,
        &batch_axis,
        &sequence_axis,
        fixture.spans,
        fixture.padded_sequence_length,
    );
    let (projections, resource_charges) =
        projection_artifacts(reprojection, &definitions, &scenarios)?;
    Ok(MemoryArtifact {
        schema: SCHEMA,
        projection_version: PROJECTION_VERSION,
        identity: identity(static_model, fixture),
        scenarios: scenarios
            .into_iter()
            .map(|(_, _, artifact)| artifact)
            .collect(),
        projections,
        resource_charges,
    })
}

type ScenarioInput = (Scenario, BTreeMap<Address, u64>, ScenarioArtifact);

fn definitions(
    batch_axis: Address,
    sequence_axis: Address,
    span_axis: Address,
    input_target: Address,
    output_target: Address,
    memory: &MemoryFacts,
) -> Vec<ProjectionDefinition> {
    vec![
        ProjectionDefinition {
            name: "active-raw-inference-output",
            target: output_target.clone(),
            basis: "logical float32 bytes for one active output tensor with shape [microbatch_spans, padded_sequence_length, model_dimension]",
            expression: product([
                CostExpression::axis(batch_axis.clone()),
                CostExpression::axis(sequence_axis.clone()),
                CostExpression::constant(memory.output_dimension),
                CostExpression::constant(memory.output_element_width),
            ]),
        },
        ProjectionDefinition {
            name: "one-call-retained-raw-output",
            target: output_target,
            basis: "logical float32 bytes if one Scry API call retains all raw output rows for the complete scale fixture before pooling",
            expression: product([
                CostExpression::axis(span_axis.clone()),
                CostExpression::axis(sequence_axis.clone()),
                CostExpression::constant(memory.output_dimension),
                CostExpression::constant(memory.output_element_width),
            ]),
        },
        ProjectionDefinition {
            name: "final-pooled-embeddings",
            target: input_target.clone(),
            basis: "logical float32 bytes retained for one final vector per scale-fixture span until atomic record preparation completes",
            expression: product([
                CostExpression::axis(span_axis),
                CostExpression::constant(memory.output_dimension),
                CostExpression::constant(memory.output_element_width),
            ]),
        },
        ProjectionDefinition {
            name: "active-model-input-tensors",
            target: input_target,
            basis: "logical int64 bytes for the three model input tensors at one active padded inference batch",
            expression: product([
                CostExpression::axis(batch_axis),
                CostExpression::axis(sequence_axis),
                CostExpression::constant(memory.input_tensor_count),
                CostExpression::constant(memory.input_element_width),
            ]),
        },
    ]
}

fn scenarios(
    mut axes: BTreeMap<Address, u64>,
    batch_axis: &Address,
    sequence_axis: &Address,
    span_count: u64,
    padded_sequence_length: u64,
) -> Vec<ScenarioInput> {
    let mut result = Vec::with_capacity(SCENARIOS.len());
    for scenario in SCENARIOS {
        axes.insert(batch_axis.clone(), scenario.microbatch_spans);
        axes.insert(sequence_axis.clone(), padded_sequence_length);
        result.push((
            scenario,
            axes.clone(),
            ScenarioArtifact {
                name: scenario.name,
                microbatch_spans: scenario.microbatch_spans,
                inferred_api_calls: span_count.div_ceil(scenario.microbatch_spans),
                padded_sequence_length,
                assumption: scenario.assumption,
            },
        ));
    }
    result
}

fn projection_artifacts(
    reprojection: &StructuralReprojection,
    definitions: &[ProjectionDefinition],
    scenarios: &[ScenarioInput],
) -> Result<(Vec<ProjectionArtifact>, Vec<ResourceChargeArtifact>), String> {
    let mut projections = Vec::new();
    let mut resource_charges = Vec::new();
    for definition in definitions {
        let mut values = Vec::with_capacity(scenarios.len());
        for (scenario, axes, _) in scenarios {
            let quantity = evaluate(
                reprojection,
                &definition.target,
                &definition.expression,
                axes,
            )?;
            values.push(ProjectionValue {
                scenario: scenario.name,
                quantity,
            });
            if matches!(
                definition.name,
                "active-raw-inference-output" | "final-pooled-embeddings"
            ) {
                validate_resource_charge(reprojection, scenario, &definition.target, quantity)?;
                resource_charges.push(ResourceChargeArtifact {
                    projection: definition.name,
                    scenario: scenario.name,
                    target: definition.target.to_string(),
                    resource: ResourceKind::MemoryBytes.as_str(),
                    quantity,
                });
            }
        }
        projections.push(ProjectionArtifact {
            name: definition.name,
            unit: "bytes",
            resource_kind: ResourceKind::MemoryBytes.as_str(),
            target: definition.target.to_string(),
            basis: definition.basis,
            expression: expression_json(&definition.expression),
            values,
        });
    }
    Ok((projections, resource_charges))
}

fn identity(static_model: &StaticModel, fixture: &FixtureFacts) -> Identity {
    let mut axes = BTreeMap::new();
    axes.insert("scale_span_count".to_owned(), fixture.spans);
    axes.insert("scale_token_count".to_owned(), fixture.tokens);
    axes.insert("model_dimension".to_owned(), MODEL_DIMENSION);
    axes.insert("model_limit".to_owned(), MODEL_LIMIT);
    axes.insert(
        "padded_sequence_length".to_owned(),
        fixture.padded_sequence_length,
    );
    Identity {
        workload_id: scale::SCALE_ID,
        input_fingerprint: scale::fingerprint(&fixture.body),
        source_bytes: fixture.source_bytes,
        tokenizer_tokens: fixture.tokens,
        spans: fixture.spans,
        fastembed_default_batch_size: FASTEMBED_BATCH_SIZE,
        model: ModelIdentity {
            name: MODEL_NAME,
            repository: MODEL_REPOSITORY,
            revision: MODEL_REVISION,
            dimension: MODEL_DIMENSION,
            limit: MODEL_LIMIT,
            window: crate::bridge::WINDOW,
        },
        grimoire: GrimoireIdentity {
            source: GRIMOIRE_SOURCE,
            revision: GRIMOIRE_REVISION,
            package: "grimoire",
            version: "1.0.0",
        },
        graph_hash: static_model.raw_graph_hash.clone(),
        canonical_description_hash: static_model.canonical_hash.clone(),
        axes,
    }
}

fn evaluate(
    reprojection: &StructuralReprojection,
    target: &Address,
    expression: &CostExpression,
    axes: &BTreeMap<Address, u64>,
) -> Result<u64, String> {
    let model = CostModel::new(vec![(target.clone(), expression.clone())])
        .map_err(|error| error.to_string())?;
    let report = model
        .evaluate(reprojection, axes)
        .map_err(|error| error.to_string())?;
    report
        .value(target)
        .ok_or_else(|| format!("memory projection has no value for `{target}`"))
}

fn validate_resource_charge(
    reprojection: &StructuralReprojection,
    scenario: &Scenario,
    target: &Address,
    quantity: u64,
) -> Result<(), String> {
    let resources = ResourceBundle::new(vec![(ResourceKind::MemoryBytes, quantity)])
        .map_err(|error| error.to_string())?;
    let charge =
        ResourceCharge::new(target.clone(), resources).map_err(|error| error.to_string())?;
    let scenario = ResourceScenario::new(
        scenario.name,
        1.0,
        scenario.assumption,
        Vec::new(),
        vec![charge],
    )
    .map_err(|error| error.to_string())?;
    let model = ResourceModel::new(vec![scenario]).map_err(|error| error.to_string())?;
    let report = model
        .evaluate(reprojection)
        .map_err(|error| error.to_string())?;
    let actual = report
        .charge(target)
        .and_then(|estimate| estimate.quantity(ResourceKind::MemoryBytes))
        .ok_or_else(|| format!("resource report has no memory charge for `{target}`"))?;
    let expected = quantity
        .to_string()
        .parse::<f64>()
        .map_err(|error| format!("memory charge was not representable as f64: {error}"))?;
    if actual.to_bits() != expected.to_bits() {
        return Err(format!(
            "resource report changed memory charge for `{target}`: {actual} != {expected}"
        ));
    }
    Ok(())
}

fn ensure_target(reprojection: &StructuralReprojection, target: &Address) -> Result<(), String> {
    if reprojection.elements.contains_key(target) {
        Ok(())
    } else {
        Err(format!(
            "memory projection target `{target}` is not visible"
        ))
    }
}

fn product(factors: impl IntoIterator<Item = CostExpression>) -> CostExpression {
    CostExpression::product(factors.into_iter().collect())
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

fn write_artifact(artifact: &MemoryArtifact, path: &Path) -> Result<(), String> {
    let text = serialize_artifact(artifact)?;
    fs::write(path, text).map_err(|error| error.to_string())
}

fn check_artifact(artifact: &MemoryArtifact, path: &Path) -> Result<(), String> {
    let expected = fs::read_to_string(path)
        .map_err(|error| error.to_string())?
        .replace("\r\n", "\n");
    let actual = serialize_artifact(artifact)?;
    if expected == actual {
        return Ok(());
    }
    let expected_lines = expected.lines().collect::<Vec<_>>();
    let actual_lines = actual.lines().collect::<Vec<_>>();
    let line = expected_lines
        .iter()
        .zip(&actual_lines)
        .position(|(expected, actual)| expected != actual)
        .unwrap_or(expected_lines.len().min(actual_lines.len()));
    let expected_line = expected_lines.get(line).copied().unwrap_or("<end of file>");
    let actual_line = actual_lines.get(line).copied().unwrap_or("<end of file>");
    Err(format!(
        "memory baseline `{}` differs from regenerated artifact at line {}\n- {}\n+ {}",
        path.display(),
        line + 1,
        expected_line,
        actual_line,
    ))
}

fn serialize_artifact(artifact: &MemoryArtifact) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(artifact).map_err(|error| error.to_string())?;
    text.push('\n');
    Ok(text)
}
