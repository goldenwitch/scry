# Benchmark interpretation

The v1 benchmark is a validation instrument for algorithmic work and owned
logical resources. It is not a speed ranking and does not claim that one
machine executes scry faster than another.

## Command

The runner is a separate workspace package and is not part of the public scry
or MCP API. The cache can be prepared explicitly; this command is the only
network-capable benchmark step:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --prepare-cache
```

After preparation, the artifact command requires all five pinned files to be
present at their expected byte sizes and performs no model download:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --output benchmarks/baseline-v1.json
```

CI compares a regenerated artifact without changing the committed baseline:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --check benchmarks/baseline-v1.json
```

The cache path is an execution input only. No path, timestamp, process id, or
runner property enters the artifact. CI prepares the cache before the test and
comparison steps, so the benchmark gate does not depend on test ordering to
populate its inputs.

## Choose a mode

Use `--prepare-cache` once when the pinned model files are absent. It is the
only network-capable mode. Use `--output` when deliberately regenerating the
committed artifact after an implementation, model, graph, workload, or cost
model change. Use `--check` for routine review and CI; it regenerates in
memory, compares exact text, and never overwrites the reference.

For a local add-performance iteration, use the diagnostic workload instead of
the mixed v1 artifact workload:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --add-only
```

`--add-only` loads the pinned model once, prepares each local case outside the
measurement boundary, and collects only the call to `Store::add`. It reports
five fixed cases: an empty set, a short singleton, a multi-span singleton, a
valid multi-member set, and a valid-plus-refused member set. Compare case
fingerprints and exact logical counts first. The per-stage nanosecond values
are local diagnostics: repeat the same warm-cache case under the same process
conditions before using them to choose an optimization, and do not promote
them to a CI gate.

The add-only command writes no artifact and does not replace the Grimoire v1
check. Run `--check benchmarks/baseline-v1.json` separately when a change also
needs to prove that static graph and mixed-workload identity stayed intact.

On a memory-constrained development machine, compilation and test concurrency
may be reduced with `CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0`, and
`RUST_TEST_THREADS=1`. These are operational settings only; they do not enter
the benchmark identity or any reported gate value.

## For patch authors

Start by naming the boundary your patch changes:

- Changes to `fetch`, `slice`, `embed`, `add`, record encoding, or store writes
  use `--add-only` first. Compare the same case fingerprint and exact logical
  counts before looking at timing.
- Changes to the ONNX parser, shape propagation, Grimoire bridge, cost model,
  pinned model, or workload identity use the normal artifact command and
  inspect the canonical description, axes, shapes, operator groups, and named
  cost reports.
- Changes to benchmark instrumentation must remain behind
  `benchmark-instrumentation`, keep one owner per counter, and prove that the
  normal `scry` build has no collector state or changed product behavior.

For an add-path patch, use this sequence:

1. Prepare or verify the pinned cache.
2. Run `--add-only` with a warm cache. Keep model loading, store setup, and
   temporary file creation outside the measured region.
3. Check fingerprints, source bytes, spans, embedding calls and vectors,
   writes, and add outcomes. These should remain exact for an unchanged case.
4. Repeat timing samples under the same process conditions. Stage nanoseconds
   locate work; they do not establish a portable speed claim by themselves.
5. Run `--check benchmarks/baseline-v1.json`. A passing check means the static
   Grimoire and mixed-workload artifact did not change; it does not replace the
   add-only evidence.

There is no universal millisecond threshold for “too expensive.” A stable
increase in deterministic logical work or bytes is a regression unless the
workload or contract explains it. A timing difference needs comparable sample
counts and conditions, and should be recorded in `performance.md` before it
drives an optimization. Elapsed time, RSS, CPU utilization, and hardware
counters never become CI gates.

Do not rewrite `benchmarks/baseline-v1.json` just because a patch changes a
number. First determine whether the changed field is static work, a workload
identity, or an owned runtime observation. An intentional baseline update must
carry the implementation, model, graph, workload, source-revision, or
cost-model cause in the same reviewed change. Cross-document embedding,
grouped persistence, and other changes to set-upsert visibility or failure
semantics require a separate contract decision and tests.

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
lookups, and the high-water mark of selected logical owned buffers, reported as
`owned_logical_bytes_high_water`.

These are logical boundary observations. They do not include filesystem
allocation, network transfer rate, native ORT residency, allocator policy, or
process RSS.

## Memory accounting

Memory evidence has three separate layers:

- Grimoire-derived logical tensor and allocation projections;
- scry-owned logical live-byte observations; and
- native process-memory observations from the embedding runtime.

The first layer is deterministic analysis, not a profiler. A memory projection
uses the pinned Grimoire description, explicit tensor shapes, dtype width, and
named scenario axes such as inference microbatch and padded sequence length.
Shape-dependent quantities are derived with checked `CostExpression` arithmetic.
The typed Grimoire `memory-bytes` account may then carry explicit scenario
charges through `ResourceModel`. `bytes` remains a transfer quantity and is not
interchangeable with resident or charged `memory-bytes`.

The memory analysis uses a separate artifact from `baseline-v1.json`. Its
identity includes the same model, graph, Grimoire source, schema, and workload
provenance, plus the scenario axes that determine the projection. It may report
active raw inference output bytes, padded input tensor bytes, pooled embedding
bytes, and other named accounts whose ownership is explicit. A sum of tensor
sizes is not a peak-live allocation claim unless an execution order and liveness
rule are part of the analyzed input. The fixed v1 artifact's `(B,S)=(1,32)`
identity is not the runtime microbatch setting.

The scale and memory commands are separate from the v1 gate:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --scale
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --scale --microbatch-size 32
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --memory
cargo run --quiet --locked --package scry-benchmarks -- --cache /tmp/scry-model-cache --memory-check benchmarks/memory-v1.json
```

`--scale` runs the deterministic large-origin add fixture and reports exact
source/token/span/vector relationships alongside process-local native-memory
diagnostics. `--microbatch-size` selects a candidate in the safe range 1..=256
for the scale run; the product default is 32 spans per embedding call. The benchmark-only
`--cpu-arena on|off` option selects the ORT CPU arena setting for allocator
experiments; its default is unchanged when the option is absent. `--memory` generates
`benchmarks/memory-v1.json`; its Grimoire
projections compare named microbatch scenarios using the pinned model
dimension, dtype width, padded sequence length, and measured scale span count.
The artifact can show the logical difference between one active raw tensor and
raw outputs retained by one API call, but it does not claim that either value
is the ONNX Runtime peak.

The second layer remains exact when its owner can count it: source bytes,
record bytes, final embedding storage, and selected logical live-byte
high-water marks. These values describe scry-owned data structures and do not
include the model file, filesystem allocation, or native runtime state.

The third layer may record maximum-observed resident-memory and secondary
process-memory observations with the operating system, runtime version,
execution provider, process lifetime, and sampling protocol. The
`max_observed_resident_bytes` field is working-set memory on Windows and
resident-set memory on Unix; `resident_metric` identifies which meaning was
sampled. These observations are useful for comparing a large-origin experiment
under the same conditions, but allocator arenas, threading, page policy, and
device residency make them machine-local. They remain diagnostic and never
become a machine-agnostic CI gate. A sourced native observation belongs to
`measurement/1`; symbolic shape arithmetic belongs to a cost or resource
projection instead.

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