# Proposal: Set Upsert for Corpus Ingestion

Status: proposed, second draft

This proposal changes `add` from a one-origin operation into a set operation
in the next major version. It chooses the public contract and the behavior at
each existing ownership boundary. It is intended to be implementable and
testable; an accepted version must propagate these decisions to the
authoritative product documents and work graphs.

## Contract

`add` accepts a canonical set of origins and one TTL shared by the operation.
It upserts each member independently:

```text
add(origins, ttl) -> AddReport
```

In the Rust library, the shape is:

```rust
pub fn add(
    &self,
    embed: &mut Embed,
    slice: &Slice,
    origins: &BTreeSet<Origin>,
    ttl: Duration,
) -> AddReport
```

`Origin` gains an ordering consistent with its canonical spelling so the set
and every report have one deterministic order. A singleton set is the one-item
case of this operation; there is no separate public `add_one` operation.

The operation is a batch upsert, not exact-set replacement. A member replaces
the document held under its origin. An origin absent from `origins` is neither
read nor changed. The operation never enumerates the corpus and never deletes
an omitted origin.

The set is unique after origin normalization. Callers constructing a
`BTreeSet<Origin>` cannot submit the same canonical origin twice. The MCP
adapter parses and normalizes every string before it starts the operation;
literal duplicates and different spellings that normalize to one origin are
folded into one member. The report contains one item for each canonical
member, in canonical origin order.

TTL is shared by the set. Every document stores that TTL, while each document
retains the `fetched_at` captured immediately before its own fetch. There is no
per-member TTL field and no default TTL.

An empty set is a successful no-op with an empty report. It opens no write
transaction and does no model or network work.

## Report and Failure Algebra

The operation returns a report rather than an outer `io::Result`, because a
set operation must preserve the outcomes already reached when a later member
stops the operation. The report has one result per canonical input member:

```rust
pub struct AddReport {
    pub items: Vec<AddResult>,
}

pub struct AddResult {
    pub origin: Origin,
    pub outcome: AddOutcome,
}

pub enum AddOutcome {
    Upserted,
    Refused(AddRefusal),
    Failed(io::Error),
    Uncertain(io::Error),
    NotAttempted,
}
```

The accessors and visibility may be refined during implementation, but this
state model is fixed:

- `Upserted` means the document was prepared and its write transaction
  committed successfully.
- `Refused` carries the existing `NotFound(origin)` or `NotText(origin)`
  decision. It is a member result, not a set failure, and processing continues
  with the next member.
- `Failed` means slicing, embedding, or record encoding failed before a write
  transaction could make the member visible. The previous document for that
  origin remains untouched.
- `Uncertain` means the store write phase returned an error. The operation
  makes no claim about whether that origin's replacement became visible. This
  is deliberately distinct from `Failed`; a caller must not silently retry an
  uncertain write as though the origin were known to be absent.
- `NotAttempted` means a preceding `Failed` or `Uncertain` outcome halted the
  operation. It is not a refusal and no work was started for that member.

Any `Failed` or `Uncertain` outcome halts the operation. Earlier successful
upserts remain committed. Later members are `NotAttempted`. No failure is
swallowed, and no later member is attempted in the hope that a damaged model
or store will recover by accident.

The existing `AddRefusal` taxonomy remains unchanged. In particular,
`fetch` continues to translate absent files, failed connections, HTTP failures,
and timeouts into `NotFound`, and non-UTF-8 or over-cap bytes into `NotText`.
The set operation does not expose HTTP status codes, client errors, or retry
classes as new refusal variants.

Raw strings are not a library input. An invalid origin is therefore not an
`AddOutcome`; it is a request validation failure at the MCP boundary, detected
for every member before any member is processed. This keeps invalid identity
out of the core API and guarantees that one malformed string cannot produce a
partially applied request.

## Pipeline and Ownership

The set operation is orchestration around the existing one-document pipeline:

```text
for origin in canonical origins:
    fetch(origin) -> slice -> embed(document chunks) -> store.replace(document)
```

The loop is sequential and processes one origin at a time. `Embed::passages`
continues to batch all chunks belonging to one document. Chunks from different
documents are not flattened into one inference batch. This preserves the
current mutable `Embed` owner, keeps the mapping from embeddings to documents
obvious, and bounds ingestion memory to one document's text and embeddings
plus the input set and report metadata.

The set operation does not introduce an async runtime, a worker pool, or
parallel fetches. The MCP server already owns one mutable embedder and runs
one synchronous tool call at a time; concurrent ingestion would require a new
resource and cancellation model rather than merely improving throughput.

`fetch` remains the owner of path and URL reads. The set operation does not
accept or retain an HTTP client, does not promise connection pooling, and does
not add retries or backoff. Each member receives the existing per-origin
64 MiB read cap and five-minute URL timeout. A private fetch implementation
may later reuse a client without changing this contract, but client lifetime
is not a public set concern.

There is no aggregate origin, byte, chunk, or wall-clock limit in this
version. Sequential processing prevents the set from staging all document
contents at once; the caller is responsible for dividing a very large corpus
into finite requests. The operation remains synchronous, so its worst-case
URL time is proportional to the number of URL members. This is an explicit
tradeoff for keeping the set free of hidden scheduler, queue, and cancellation
state.

## Persistence and Visibility

Each successful member uses one redb write transaction, as the current
`Store::replace` operation does. The transaction boundary is per origin, not
per set:

1. Fetch, slice, and embed without changing the corpus.
2. Encode and validate the complete document record.
3. Commit one replacement for that origin.
4. Record `Upserted` and continue.

An observer may see a committed prefix while the set is running. There is no
set generation, marker, staging table, or rollback of earlier members. A
refusal does not open a write transaction. A pre-write processing failure
leaves the old member untouched. A store write error halts and reports
`Uncertain`, because the public contract will not pretend to know the result
of a failed write phase.

The implementation must preserve the distinction between pre-write record
encoding errors and write-phase errors even though the current private
`replace` method returns one `io::Result`. The internal store seam may split
preparation from commit or return a private phase-bearing error; it must not
flatten those states before the set operation builds its report.

The on-disk record layout does not gain set metadata. Existing compatible redb
stores remain readable; the major-version break applies to the library and
MCP APIs, not to a new migration requirement for records.

## Refreshes and Handles

Every submitted origin is fetched and re-embedded, even when the origin is
already held. A successful refresh replaces text, spans, embeddings,
`fetched_at`, and TTL. The operation does not compare source bytes first and
does not make a no-op optimization part of the contract.

An origin can have only one member in a set, so the final state never depends
on processing the same source twice. If the fetched text at a span is
unchanged, an old handle for that span remains valid. If the text at that span
changes, that handle is `Stale`. An upsert does not produce `Gone`; `Gone`
remains the result of deleting the document. A refused, failed, uncertain, or
not-attempted member does not intentionally remove its old document, though
the uncertain case does not promise what the failed write left behind.

## MCP Contract

The `add` tool changes to one strict argument object:

```json
{
  "origins": ["path-or-url", "another-path-or-url"],
  "ttl_seconds": 3600
}
```

Both fields are required. `origins` is an array of strings and may be empty.
`ttl_seconds` is an integer greater than or equal to zero. Additional fields
are rejected. The server parses every origin, constructs the canonical set,
and only then invokes the library. One invalid origin rejects the request
before any write; it does not appear as an item in an otherwise valid report.

The successful structured result is:

```json
{
  "status": "complete",
  "items": [
    { "origin": "...", "status": "upserted" },
    {
      "origin": "...",
      "status": "refused",
      "error": { "kind": "not_found", "message": "..." }
    }
  ]
}
```

The other item statuses are `failed`, `uncertain`, and `not_attempted`.
`failed` and `uncertain` include an error with kind `io`; `not_attempted` has
no error because no member-level operation ran. A report containing only
`upserted` and `refused` items has `status: "complete"` and `isError: false`.
A report containing `failed`, `uncertain`, or `not_attempted` items has
`status: "halted"` and `isError: true`, while retaining the complete partial
report in `structuredContent`.

Invalid arguments, invalid origins, and unknown tools continue to use the
existing MCP tool-error path and do not return an add report. JSON-RPC
protocol errors remain distinct from tool results.

The synchronous server loop remains synchronous. There is no cancellation
tool, background job, progress stream, or operation token in this version. If
the client disconnects or loses the response, work already in progress may
finish and commit its per-origin prefix. A retry is a new set upsert: it
refetches every member, and the caller must use the returned report from that
retry rather than assume the lost report's prefix.

## Entropy Constraints

The implementation has one path and one policy for each boundary:

- one public `add` operation over a unique canonical origin set;
- one shared TTL, with no optional per-member override;
- one canonical processing and result order;
- one sequential orchestration owner;
- one per-origin transaction boundary;
- refusals continue, weather halts;
- all members receive exactly one outcome, including unattempted members;
- no public HTTP client, retry, scheduler, cancellation, or corpus enumeration;
- no new writer that bypasses fetch and embedding;
- no set metadata in the store.

The `Uncertain` outcome is the one state that cannot be eliminated by a type:
a failed persistence operation may not reveal whether its commit became
visible. It is explicit so callers do not collapse it into either success or
known absence.

## Test Contract

Tests must measure the chosen contract, not merely that an embedding batch can
complete.

### Set orchestration

- a singleton set upserts one origin and returns one `Upserted` item;
- multiple valid origins are processed in canonical order and all become
  searchable;
- an empty set returns a complete empty report and performs no write;
- equivalent normalized origins produce one member and one result;
- every result is in canonical origin order;
- a mixed valid, `NotFound`, and `NotText` set continues through all members,
  stores the valid member, leaves refused existing documents untouched, and
  returns `status: complete` semantics;
- a synthetic `Failed` one-member result halts the orchestration and marks all
  later members `NotAttempted`;
- a synthetic `Uncertain` one-member result has the same halt behavior;
- earlier `Upserted` members remain committed after a later halt.

The set loop should be separated from the concrete one-document pipeline by a
private testable function or equivalent internal seam. The seam is not a new
public abstraction: it lets unit tests supply deterministic one-member
outcomes and prove ordering, continuation, and halting without inducing model
or disk accidents.

### Document, store, and handle behavior

- a member is not replaced until all of its spans and embeddings are ready;
- pre-write processing or encoding failure leaves the old record untouched;
- a write-phase failure is reported as `Uncertain` and stops later work;
- each successful member is visible after its own commit, while later members
  can remain absent during the operation;
- the shared TTL is written to every successful document and fetched times are
  per-member;
- same-content refresh preserves a handle for an unchanged span;
- changed text makes the affected old handle `Stale`, while the still-held
  document does not produce `Gone`;
- delete remains the separate operation that can produce `Gone`.

### MCP

- the advertised schema is exactly `origins` plus `ttl_seconds`;
- all invalid origins are rejected before the first add attempt;
- duplicate and normalized-equivalent origins produce one canonical item;
- mixed refusal results retain origin identity and serialize with
  `isError: false` and `status: complete`;
- halted reports retain successful, failed, and not-attempted items with
  `isError: true` and `status: halted`;
- the real process test indexes multiple local files, searches the resulting
  corpus, and exercises a mixed refusal report;
- a client disconnect or lost response is documented as non-cancellation, not
  inferred to mean rollback.

### Existing seams

The current fetch tests remain the authority for the per-origin cap, timeout,
and refusal mapping. Existing embedding tests remain the authority for
batching a document's chunks. New tests should not flatten those concerns into
the set loop. Store tests should cover the per-origin transaction boundary,
and the private one-member seam should make processing and persistence failure
classification deterministic.

## Required Documentation Changes

Acceptance requires these current surfaces to move together:

- [`initial-spec.md`](../docs/product-planning/initial-spec.md): describe add
  as adding a set of origin-identified documents and retain replacement
  semantics;
- [`vocabulary.md`](../docs/product-planning/vocabulary.md): replace the
  singular add grammar and define the report outcomes without turning weather
  into new refusal variants;
- [`architecture.md`](../docs/product-planning/architecture.md): show the
  sequential set flow, per-origin commit visibility, and retained
  no-enumeration rule;
- [`mcp.md`](../docs/product-planning/mcp.md): define the new strict input,
  report, and `isError` rules;
- [`library.vine`](../library.vine) and [`mcp.vine`](../mcp.vine): add work
  nodes for the set contract, implementation, failure tests, and MCP evidence;
- library unit tests, integration tests, and the MCP process test: establish
  every outcome and boundary above.

The proposal is ready for implementation planning when those documentation
changes can be mechanical consequences of this contract rather than new
semantic decisions.