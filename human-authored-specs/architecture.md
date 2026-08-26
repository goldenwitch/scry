# Architecture

Structure the vocabulary already forces. Where it forces nothing, the fork is
named rather than taken.

## Delivery

The ordered project and runtime map is [lifecycle.md](lifecycle.md). The
library checkpoint is complete and the next delivery is the served tool as MCP;
a separate human-facing command has no contract or implementation yet.

## State

`$corpus` is the only state: one `document` per `origin`, and beside each one a
cached `embedding` per span.

The embeddings are derived: `add` computes them, and each names the `model` that
produced it, so a corpus met by a different model is recognised rather than
silently mis-scored.
`handle`, `hit` and `provenance` are values in flight, never held.

## Parts

| Part | Shape | Owns |
| --- | --- | --- |
| fetch | `origin -> text fetched_at` | reading a path or url; refusing bytes that are not text; the moment it happened |
| store | `$corpus` | one document per origin, its spans, and an embedding per span |
| slice | `model window text -> span*` | where boundaries fall; counting tokens, cutting at offsets |
| embed | `text -> embedding` | binding a model to a vector; nothing else sees either |
| handle | `chunk text -> digest` | minting and verifying; the only place text is hashed |

`origin` is normalised when it is constructed, and there is one constructor, so
every verb that names an origin — `delete` included — gets the same key.
Normalisation is idempotent, so an origin read back out of the store passes
through that same constructor and lands where it started.

`fetch` reads a path with the standard library and a url with ureq — synchronous
and pure Rust, like redb and fastembed, so nothing here needs an async runtime.
scry does the fetching, so an agent never holds the network's problems; it holds
`NotFound` or a document.

`embed` is fastembed with bge-small-en-v1.5: 384 dimensions, a 512-token
`limit`, and an instruction prefix on queries but not on passages, so it is told
which it is doing. It normalises the vector inside the embedding it returns, so
cosine is a dot product everywhere downstream and no other part has to remember.
Its `limit` bounds `window`, so `slice` cannot hand it text it will not accept.
The model is loaded by `Embed::load` before `Store::open`, not on first search;
the store receives the model that embed produced.

`window` is 256 — half of `limit`, so a chunk's embedding stays on one subject.

`store` is redb — one file, ACID, pure Rust. Its path and model are constructor
arguments: an absent path means create, a present path means open, and after the
first run the file is its own configuration. Its layout is private. The path
back to bytes runs through
`origin`, into the world, not through our file, so nothing outside scry ever
reads it and it stays ours to change.

## Flows

```
add(origin, ttl)      fetch -> slice -> embed -> store.replace
delete(origin)        store.remove
search(query, count)  embed -> scan $corpus -> handle.mint -> hit*
neighbours(handle, n) handle.verify -> adjacent spans -> handle.mint -> passage*
provenance(handle)    store.document -> handle.verify -> provenance | Stale | Gone
```

Only `add` and `delete` write, and `add` is the only verb that reaches the
network.

Every handle an agent holds was minted from text the store held at that moment,
and every handle it hands back is verified against the text there now.

## What the vocabulary rules out

- **One store.** There is no second place state lives, so nothing needs a lock,
  a marker, or a generation counter to stay agreed with anything else. A handle
  carries the digest of its own text, so even a corrupted read surfaces as
  `Stale` at the point of use rather than as a plausible wrong answer.
- **No handle table.** A handle is `origin`, `span`, and a hash of the text at
  that span — all derivable at mint time from the document itself.
- **No background process.** `stale` is a comparison performed when someone
  asks. Nothing expires on a timer, nothing refetches itself.
- **No routing layer.** There are no collections to dispatch between.
- **No duplicated text.** A chunk is `(origin, span)`, so its text is projected
  from the document rather than stored a second time.
- **No enumeration.** No verb lists the corpus, so an agent deletes origins it
  remembers. `search` walks the store to scan it; nothing hands that walk to a
  caller.