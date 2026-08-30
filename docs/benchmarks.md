# Benchmark interpretation

The v1 benchmark is a validation instrument for algorithmic work and owned
logical resources. It is not a speed ranking and does not claim that one
machine executes scry faster than another.

## Command

The runner is a separate workspace package and is not part of the public scry
or MCP API. Required model assets must already be present in the cache:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --output benchmarks/baseline-v1.json
```

CI compares a regenerated artifact without changing the committed baseline:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --check benchmarks/baseline-v1.json
```

The cache path is an execution input only. No path, timestamp, process id, or
runner property enters the artifact.

## v1 identity

The baseline is one fixed workload:

- workload id: `bge-small-onnx-b1-s32`;
- model: `Xenova/bge-small-en-v1.5` at revision
  `ea104dacec62c0de699686887e3f920caeb4f3e3`;
- batch size: `1`;
- padded sequence length: `32`;
- slice window: `256` tokens;
- Grimoire source: `https://github.com/goldenwitch/grimoire.git` at revision
  `bd9920bc1ae79d40383fedcacea3adb87fd98109`;
- cost model: `cost-model-v1`; and
- workload fingerprint: the fixed local documents, query, ttl, counts, and
  operation sequence in `crates/scry-benchmarks/src/workload.rs`.

The raw ONNX graph is parsed offline. The artifact records its SHA-256 hash,
canonical Grimoire description hash, addressed graph, operator groups, tensor
shapes, symbolic axis bindings, and the complete structural operator census.
The raw graph is not an optimized ORT graph and is never loaded by the bridge.

## Static fields

Static cost follows this lineage:

```text
ONNX bytes -> addressed Grimoire description -> explicit axes and shapes -> CostModel -> CostReport
```

The v1 reports are separate named projections:

- `macs`: one multiplication-accumulation per MatMul reduction;
- `fma_flops`: two operations per MAC; and
- operator counts and group membership: structural facts, not runtime events.

The first cost projection prices only `MatMul`. Operations such as `Erf`,
`Sqrt`, and `Softmax` remain visible in the description and operator groups but
receive no invented FLOP price. Missing shapes, unknown symbolic axes, unknown
sources, incompatible dimensions, invalid Grimoire, missing cost assignments,
and arithmetic overflow fail the runner.

## Runtime fields

Runtime fields come from the feature-gated private scry collector and are
reported separately from static cost. They include source bytes, sliced spans,
embedding calls and returned vectors, redb read/write transactions, search scan
cardinalities, returned passage bytes, add outcome cardinalities, provenance
lookups, and the high-water mark of selected logical owned buffers.

These are logical boundary observations. They do not include filesystem
allocation, network transfer rate, native ORT residency, allocator policy, or
process RSS.

## Excluded from gates

Elapsed time, FLOP/s, CPU frequency, utilization, scheduler behavior, raw RSS,
hardware performance counters, provider execution, ORT graph optimization, and
device topology are diagnostic measurements only. They cannot change the v1
pass/fail result.

## Baseline updates

The runner's `--check` mode requires exact artifact equality. A baseline update
must change the implementation, model, graph, workload, source revision, or
cost model in the same reviewed change, or explain why a deterministic artifact
changed. Identity mismatches fail rather than regenerate a reference silently.

The canonical JSON artifact contains no machine-specific gate field. Its static
description and cost reports are recomputable from the pinned model bytes and
explicit axis bindings; its runtime fields are recomputable from the fixed
workload through the private boundary seams.