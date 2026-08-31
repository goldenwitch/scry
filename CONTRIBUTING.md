# Contributing

Keep changes small, explicit, and consistent with the library and MCP
contracts. The repository treats benchmark output as evidence about a named
workload, not as a machine-speed ranking.

## Preserve the benchmarks

Before opening a patch, name the boundary it changes:

- Changes to `fetch`, `slice`, `embed`, `add`, record encoding, or store writes
  use the add-only diagnostic workload first.
- Changes to the ONNX parser, shape propagation, Grimoire bridge, cost model,
  pinned model, or workload identity use the v1 artifact workflow.
- Changes to benchmark instrumentation stay behind the
  `benchmark-instrumentation` feature and keep one owner per counter.

For an add-path change, prepare or verify the pinned model cache, then run:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache <model-cache> --add-only
```

The add-only run isolates `Store::add` after model, slicer, store, and local
fixture setup. Check the case fingerprints and exact logical observations
first: source bytes, spans, embedding calls and vectors, writes, and add
outcomes. These values should not change for an unchanged workload.

Stage nanoseconds are diagnostic. Repeat comparable warm-cache samples before
using them to choose an optimization, and do not turn elapsed time, RSS, CPU
utilization, or hardware counters into CI gates. Record meaningful iterations
and their conditions in `performance.md`.

The add-only command does not replace the committed Grimoire-backed artifact
check. Run the normal check as well:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache <model-cache> --check benchmarks/baseline-v1.json
```

A baseline mismatch is a signal to inspect the identity and field that
changed. Do not overwrite `benchmarks/baseline-v1.json` merely to make CI
pass. A deliberate update must include the implementation, model, graph,
workload, source revision, or cost-model change that explains it, along with
any required documentation and tests.

Cross-document embedding, grouped persistence, and other changes to set-upsert
visibility, failure handling, memory bounds, or partial progress require a
contract decision before implementation. They are not ordinary benchmark
optimizations.

## Validation

Run the focused test for the boundary you changed, then the repository checks:

```text
cargo fmt --all -- --check
cargo test --workspace --all-targets --all-features --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo doc --workspace --no-deps
cargo deny check
```

The full benchmark workflow, including cache preparation and the exact CI
baseline check, is documented in [docs/benchmarks.md](docs/benchmarks.md).
On a memory-constrained machine, `CARGO_BUILD_JOBS=1`,
`CARGO_INCREMENTAL=0`, and `RUST_TEST_THREADS=1` reduce operational resource
pressure without changing benchmark identity or results.
