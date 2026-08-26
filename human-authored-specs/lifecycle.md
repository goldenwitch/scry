# Lifecycle

This is the order scry passes through, from a human decision to a tool result
in an editor. It separates four things that otherwise look like one operation:
the contract, the implementation, validation of the implementation, and the
host that invokes it.

The detailed contract belongs to the document named by each stage. This file
owns the ordering and the boundary between stages.

## 1. Author the jobs

Read [initial-spec.md](initial-spec.md). It fixes the three jobs:

1. Search a corpus and return passages with handles.
2. Add and delete documents by origin.
3. Exchange a handle for provenance.

The initial spec also rules out a fourth job. No implementation begins before
the nouns and the jobs are named.

## 2. Fix the vocabulary

Read [vocabulary.md](vocabulary.md). It gives one name to each noun, projection,
verb, and failure. This is where the public answers are fixed: `open`, `add`,
`delete`, `search`, `neighbours`, and `provenance`.

A question not answered here remains open. A decision made later is recorded in
the implementation stage that needs it rather than silently added to this one.

## 3. Fix the architecture

Read [architecture.md](architecture.md). It assigns each decision to one part,
fixes the state boundary, and defines the delivery order. The library is the
first delivered stage. MCP is the second. A separate human-facing command is
not planned.

The architecture is a contract about ownership and flow. It does not replace
the executable tests or the work graphs.

## 4. Build the library

Execute [library.vine](../library.vine). Its dependency edges order the work:
primitive types and the model come before slicing and embedding; those parts
come before storage and verbs; the verbs come before the library checkpoint.

The library's runtime construction order is:

```text
Embed::load(cache)
    -> Slice::new(embed)
    -> Store::open(store, embed.model())
```

The store receives the model already loaded by the embed seam. It does not load
one itself. The library's public caller then holds the two seams and invokes the
five verbs on `Store`.

## 5. Validate the library

The library checkpoint is valid only when both behavior and boundary checks
pass:

```text
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo deny check
```

The unit tests validate parts. `crates/scry/tests/suite.rs` validates the
composition through the public dependency-facing API. The checkpoint is the
thing that authorizes the next delivery; a passing part test alone does not.

## 6. Define and build MCP

Read [mcp.md](mcp.md) and execute [mcp.vine](../mcp.vine). The MCP adapter is a
separate workspace member, so it can depend only on the public `scry` API. Its
startup sequence is:

```text
command arguments
    -> Embed::load(cache)
    -> Slice::new(embed)
    -> Store::open(store, embed.model())
    -> JSON-RPC stdio loop
```

The process boundary then has two phases:

```text
MCP session:
    initialize
    -> notifications/initialized
    -> tools/list
    -> tools/call

Tool call:
    MCP arguments
    -> public scry verb
    -> structured result or tool error
```

A handle returned by `search` crosses the boundary as `origin`, `start`, `end`,
and hexadecimal `digest`. `provenance` and `neighbours` rehydrate it and let
the library decide whether it is valid, `Stale`, or `Gone`.

The process-level end-to-end test must spawn the actual binary and perform
`initialize`, discovery, `add`, `search`, handle exchange, `provenance`,
`neighbours`, `delete`, and `Gone`. This proves the protocol and library are
connected without reaching into private store operations.

## 7. Register MCP in VS Code

The workspace registration is [`.vscode/mcp.json`](../.vscode/mcp.json). It is
host configuration, not part of the MCP protocol and not a Chat prompt. It tells
VS Code to run:

```text
cargo run --quiet --manifest-path ${workspaceFolder}/Cargo.toml
    --package scry-mcp --
    --store ${workspaceFolder}/scry.redb
    --cache ${userHome}/.cache/scry-model-cache
```

The editor-side sequence is:

```text
.vscode/mcp.json
    -> VS Code starts scry-mcp
    -> VS Code sends initialize
    -> VS Code sends tools/list
    -> VS Code caches the five discovered tools
```

Starting or discovering an MCP server does not require a Chat message. In
particular, `workbench.action.chat.open` with a query is a Chat API; it creates a
Chat prompt and is not the way to start an MCP server. The MCP server exposes
`tools`, not `prompts`.

The current editor evidence checks that the server reaches `Running` and that
VS Code reports `Discovered 5 tools`. That is discovery validation. It is
separate from validating a model-generated Chat request.

## 8. Invoke a tool from Chat

Only after VS Code has discovered the tools does a Chat request enter the
picture. A user or agent writes a request in Chat, and the model may choose a
registered tool:

```text
Chat request
    -> model selects scry tool
    -> VS Code sends tools/call to scry-mcp
    -> scry-mcp invokes the public library
    -> VS Code renders the tool result
```

This is the final host-level check. It should observe an actual `tools/call` for
one of `add`, `search`, `provenance`, `neighbours`, or `delete`, followed by the
expected result. The repository currently has a spawned-process MCP test and
VS Code discovery evidence; it does not claim that a Chat-generated tool call
has been captured by an automated test.

## 9. Future delivery

A human-facing command is not part of the current lifecycle. It begins only
when it has its own contract, implementation record, and executable acceptance
check. Mentioning it in a delivery list does not start that stage.
