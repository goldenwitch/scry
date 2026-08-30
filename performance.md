# Scry Performance Iterations

This is the running evidence log for the add-item performance pass. It records
conditions, raw observations, and decisions in order. It is not a product
performance claim.

The Grimoire description and cost reports remain the static model authority in
`benchmarks/baseline-v1.json`. This log records runtime add work separately.
Elapsed time is diagnostic until a workload and collection boundary make it
useful for local comparison; it is never a machine-agnostic CI gate.

## Direction

- Target: the library `Store::add` path.
- Current contract: canonical origins are processed sequentially; each origin
  fetches, slices, embeds all of its spans in one model call, prepares one
  record, and commits one replacement transaction.
- Optimization rule: measure an owning boundary before changing it.
- Contract rule: cross-document embedding and grouped persistence require a
  reviewed set-upsert decision; they are not implicit optimizations.

## Iteration 0: Existing Baseline and Scope

Date: 2026-08-30
Status: complete

The existing Grimoire-backed runner uses one mixed workload. It adds two local
text documents and one missing origin, then searches, looks up provenance,
widens a passage, and deletes one document. The committed runtime observations
are therefore not an add-only measurement.

Observed in `benchmarks/baseline-v1.json`:

- `source_bytes`: 10862
- `sliced_spans`: 10
- `embedding_calls`: 3
- `embedding_vectors`: 11
- `write_transactions`: 3
- `read_transactions`: 4
- `add_members`: 3
- `add_upserted`: 2
- `add_refused`: 1
- `owned_logical_bytes_high_water`: 13343

The ten passage spans and vectors belong to the two successful adds. The third
embedding call and vector belong to the search query. The third write is outside
the two successful add commits, so these totals must not be used to infer a
per-item add cost.

A local warm-cache baseline check completed successfully in approximately 90.1
seconds on Windows. This includes model loading, static ONNX and Grimoire
analysis, the mixed workload, and artifact generation. It is diagnostic only.

Working direction: isolate add-only cases before selecting an optimization.
The first candidates to measure are passage tokenization/validation, record
preparation allocations and copying, and the existing per-origin transaction
boundary. No implementation change is selected from this iteration.

## Measurement Contract

Add-only collection begins after the pinned model is loaded, the slicer is
constructed, the store is opened, and fixed local source files are written.
It ends immediately after `Store::add` returns. Model loading, ONNX/Grimoire
analysis, temporary workspace setup, and artifact serialization are outside
that collection boundary.

Warm-cache runs are the comparison condition. Cold model startup may be
recorded as a diagnostic observation but cannot be compared with add-only work.
Each case has a stable workload id and fingerprint. Exact logical counts and
bytes are the reproducible evidence; elapsed time is a local diagnostic sampled
under the same process and cache condition.

## Iteration 1: Add-Only Matrix and Stage Diagnostics

Date: 2026-08-30
Status: complete

Command:

```text
cargo run --quiet --locked --package scry-benchmarks -- --cache <warm-pinned-cache> --add-only
```

The command loaded the pinned model before collection and ran five fixed local
cases. The stage values below are nanoseconds from one Windows warm-cache run;
they are diagnostic observations, not portable performance claims.

| Case | Fingerprint | Source bytes | Spans | Embedding calls/vectors | Writes | Fetch ns | Slice ns | Embed ns | Record ns | Commit ns |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| empty-set-v1 | `4bb62635b10a52ea074208219d48100c7a289bc119809b55a46c81136a730ac0` | 0 | 0 | 0/0 | 0 | 0 | 0 | 0 | 0 | 0 |
| short-singleton-v1 | `8284ed70ad0a95383918a91929243af956920016782f8716ea4edecc5ceca37e` | 41 | 1 | 1/1 | 1 | 154800 | 748400 | 9421600 | 98500 | 1374300 |
| multi-span-singleton-v1 | `de833c6a341f85211103d50469a0b4b455aeca1eb92a20b5089bbff665fc072e` | 3231 | 3 | 1/3 | 1 | 164600 | 13982200 | 236296100 | 338200 | 1770900 |
| valid-multi-member-v1 | `494c1f7fc72b41f3386595f623ef4b98c4b6c8d72f8680af827430c6da8c5974` | 99 | 2 | 2/2 | 2 | 415900 | 796800 | 15820400 | 181800 | 3587400 |
| refused-member-v1 | `9d7528ccd484cdd5e5df3810a8f8d7888a2632a59b74006bc5ca2f1da3427204` | 37 | 1 | 1/1 | 1 | 225500 | 337600 | 7696300 | 111400 | 1420300 |

Logical observations matched the fixed cases: empty set did no member work;
the singleton cases produced one write per successful origin; the valid
multi-member case produced two writes and two model calls; and the refused
member case produced one refusal without a write. The long fixture produced
three spans, which is the pinned tokenizer's observed shape for its 3231-byte
input and is now the fixed expectation.

Embedding was the largest measured stage in every non-empty case and was
236296100 ns of the multi-span case. Slicing was the next largest stage there
at 13982200 ns; record preparation and commit were 338200 ns and 1770900 ns.
This selects tokenization/inference-boundary analysis as the next investigation
while keeping record allocation and transaction work as measured alternatives.

A second warm-cache run reproduced every logical count and fingerprint. Its
multi-span embedding stage was `243217100` ns, close to the first run's
`236296100` ns, while the valid multi-member commit stage moved from
`3587400` ns to `91183200` ns. The repeat is therefore evidence that stage
timings are useful for locating work but require repeated samples before a
small optimization can be called a timing improvement.

## Iteration 3: Record and Transaction Disposition

Date: 2026-08-30
Status: complete

The warning-free post-change add-only sample measured record preparation and
commit as follows:

| Case | Spans | Embedding calls | Record ns | Commit ns | Owned logical high-water bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| short-singleton-v1 | 1 | 1 | 99000 | 1663600 | 1665 |
| multi-span-singleton-v1 | 3 | 1 | 334900 | 1472400 | 7959 |
| valid-multi-member-v1 | 2 | 2 | 180700 | 3391600 | 1682 |
| refused-member-v1 | 1 | 1 | 88800 | 1320600 | 1661 |

The store test suite passed all 16 record, checksum, persistence, and
transaction tests. Record preparation is a small part of the measured add
path, and the current record bytes and checksum behavior provide no evidence
for changing its private layout. The per-document batch and per-origin write
shape is explicit in the add-only matrix: the three-span singleton uses one
embedding call and one write, while the two-member case uses two calls and two
writes. A later run also observed a `91183200` ns commit stage for the
two-member case, so commit timing remains diagnostic and is not a basis for
grouping transactions.

Decision: make no record or transaction implementation change in this
iteration. Keep the current private record format, one-document embedding
batch, and one-origin commit; retain grouped persistence and cross-document
embedding as contract-sensitive alternatives rather than silently adopting
them.

## Iteration Log

| Iteration | Date | Change | Workload | Observation | Decision |
| --- | --- | --- | --- | --- | --- |
| 0 | 2026-08-30 | No code change | Existing mixed v1 runner | Add work is mixed with search, provenance, neighbours, and delete | Build the add-only matrix first |
| 1 | 2026-08-30 | Add-only matrix and feature-gated stage diagnostics | Five fixed local cases | Matrix passes; embedding dominates the multi-span case | Investigate passage tokenization first; keep cross-document batching and grouped commits as contract-sensitive alternatives |
| 2 | 2026-08-30 | Skip redundant post-slice passage validation in `Store::add` | Five fixed local cases | Logical counts and fingerprints unchanged; multi-span embedding median moved from 239.8 ms to 219.5 ms in local samples | Keep the behavior-preserving change; collect more samples before making a small timing claim |
| 3 | 2026-08-30 | Record and transaction disposition | Five fixed local cases plus 16 focused store tests | Record stage is sub-millisecond; commit remains variable and semantically per-origin | Keep the current record and transaction boundaries; do not group writes without a reviewed contract change |

## Iteration 2: Skip Redundant Post-Slice Validation

Date: 2026-08-30
Status: complete

The pinned fastembed implementation tokenizes its string inputs internally and
does not accept the encoding produced by `Slice`. The add path therefore keeps
one tokenizer pass in `Slice`, but no longer repeats the limit validation pass
in `Embed` before fastembed's own inference tokenization. Direct internal test
calls retain the validated `passages` path; `Store::add` uses the explicitly
named `passages_from_slice` path, and query embedding remains validated.

The embedding-stage samples below are raw nanoseconds from the warm-cache
add-only command. Pre-change has two samples from Iteration 1; post-change has
four samples, including the warning-free run after the test-only helper scope
was corrected.

| Case | Pre-change samples | Post-change samples | Pre median ns | Post median ns |
| --- | --- | --- | ---: | ---: |
| short-singleton-v1 | `9421600, 10735600` | `9400200, 9921400, 11008000, 9041000` | 10078600 | 9660800 |
| multi-span-singleton-v1 | `236296100, 243217100` | `217898100, 212189500, 223095500, 221696200` | 239756600 | 219497150 |
| valid-multi-member-v1 | `15820400, 19262300` | `16038300, 15428200, 16657500, 15470700` | 17541350 | 15733250 |
| refused-member-v1 | `7696300, 9528700` | `7984500, 7970100, 8004900, 7542700` | 8612500 | 7977300 |

Every post-change case reproduced the existing fingerprint and exact logical
counts: source bytes, spans, embedding calls and vectors, writes, member
outcomes, and zero reads. The focused embedding suite passed all seven tests,
including direct over-limit refusal, query validation, and the model-limit
boundary. No Grimoire description, static cost report, or v1 artifact field
changed.

The stage readings support keeping this change, but the sample sizes and the
earlier commit-stage spread are too small for a portable timing conclusion.
The next measurement should either repeat this comparison under a fixed
sampling protocol or move to record preparation and contract-sensitive
batching, while preserving the current per-document and per-origin contract.
