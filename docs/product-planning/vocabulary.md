# Vocabulary

Every word scry uses, and nothing else. One name per thing, one thing per name.

These productions define types. A particular value is written with a leading
`$`, so `corpus` is a shape and `$corpus` is the one scry holds.

## Nouns

```ebnf
corpus     ::= document*
document   ::= origin text fetched_at ttl span*
origin     ::= path | url
chunk      ::= origin span
span       ::= start end
handle     ::= chunk digest
passage    ::= handle text
hit        ::= passage score
provenance ::= chunk fetched_at ttl
model      ::= name dimension limit
embedding  ::= model vector
vector     ::= float*
limit      ::= tokens
window     ::= tokens
```

`origin` is the document's identity; adding the same origin replaces the
document.

`text` is text: UTF-8, strictly, since reading anything else would be converting
it. scry indexes what it is given, so `add` refuses bytes it cannot read.

`start` and `end` are byte offsets into `text`. Only `slice` constructs them, so
they fall on character boundaries by construction rather than by checking.

`span*` partitions the document's text — contiguous, non-overlapping, covering
— so every offset belongs to exactly one `chunk`, and the partition is a fact
about the document rather than something a slicer must reproduce.

`digest` is a hash of the chunk's text.

A `passage` is what an agent reads: text, and the `handle` that vouches for it.
`query` is the text searched with, `score` the cosine of it against a passage,
and `count` how many are wanted.

`model` is the embedder. A `vector` is `dimension` floats, normalised, so cosine
is a dot product. An `embedding` is a vector frozen to the `model` that produced
it: vectors from different models share no space, so the model is not a fact
about an embedding but part of what one is. `limit` is the most input the model
accepts.

`window` is the chunk size `slice` targets, and it cannot exceed `limit`. `slice`
cuts every `window` of tokens and by no other rule. Both count `tokens` as the
model counts them, including whatever the tokenizer adds, while a `span` counts
offsets into `text`. Only the model's tokenizer relates the two, so `slice`
counts in tokens and cuts at offsets — and a chunk too large to embed cannot be
constructed.

## Projections

Derived, so they are not nouns. One that names `$corpus` reads state; one that
does not is pure.

```
text($corpus, chunk)       = document($corpus, chunk.origin).text[chunk.span]
neighbours($corpus, chunk) = chunks of the same origin, adjacent to chunk.span
expired(document)          = now > document.fetched_at + document.ttl
```

## Verbs

```ebnf
open       : path model   -> corpus | Incompatible(path)
add        : origin ttl   -> () | NotFound(origin) | NotText(origin)
delete     : origin       -> ()
search     : query count  -> hit*
neighbours : handle count -> passage* | Stale(origin) | Gone(origin)
provenance : handle       -> provenance | Stale(origin) | Gone(origin)
```

Every verb but `open` is called on `$corpus`. All of them read it; only `add`
and `delete` write it. `model` is supplied by the embed seam; `Store::open`
recognises it but does not load one.

`ttl` is supplied by every `add`; there is no default. `expired` is advisory —
nothing is excluded from `search`, and nothing refetches itself. Whoever holds a
`provenance` computes it, from the `fetched_at` and `ttl` in it. It is a
different claim from `Stale`: one says the material has aged, the other says
this text is no longer there.

`delete` on an origin holding nothing is a no-op, as `add` replaces without
asking what was there.

`Stale` says the text at that span is no longer the text the handle was cut
from. `Gone` says the document is no longer held. `NotFound` says there are no
bytes at that origin, and `NotText` that the bytes there are not something scry
will index. Each refuses rather than answer wrongly, and each has its own name
because each is repaired differently.

`Incompatible` is not a refusal but an end. The store was written by another
model or another layout, and this build cannot use it: no verb runs, and nothing
an agent does will change that. A terminal state earns a word precisely because
nothing proceeds from it.

Those five are the whole of the vocabulary's failures. A refusal is a decision
scry makes and an agent can act on; a disk that will not read is weather, and
weather is not a word here.

## Unwritten

- `text` is stored, so a local file exists that no noun names. If an agent needs
  a whole document, the remedy is a verb, not a noun.
- A document converted before `add` has a path for its `origin`, so its source
  url lives outside scry.
