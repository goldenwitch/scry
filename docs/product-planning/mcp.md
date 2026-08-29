# MCP delivery

The full project and runtime order is [lifecycle.md](lifecycle.md). The library
is the first delivery and the MCP server is the next. A separate human-facing
command has no contract or implementation in this repository yet.
The MCP server is the caller-facing process boundary for the library.

## Process

The binary is `scry-mcp`. It requires two arguments:

```text
scry-mcp --store PATH --cache PATH
```

`PATH` names the redb corpus file and `cache` names the pinned model files.
Both are explicit so the server has no hidden working-directory state. The
server loads `Embed` from `cache`, builds `Slice` from that seam, and opens the
store with the model returned by `Embed::model`. An incompatible store or a
startup error ends the process before an MCP session begins.

## Transport

The server speaks JSON-RPC 2.0 over newline-delimited JSON on stdin and stdout.
It writes no logs to stdout. It supports MCP protocol version `2025-06-18`, the
`initialize` request, the `notifications/initialized` notification, `ping`,
`tools/list`, and `tools/call`. Tool use before initialization is rejected.

The server advertises exactly five tools, one for each public library verb:

| Tool | Arguments | Successful structured content |
| --- | --- | --- |
| `add` | `origins: string[]`, `ttl_seconds: integer >= 0` | `{ status: ..., items: [...] }` |
| `delete` | `origin: string` | `{}` |
| `search` | `query: string`, `count: integer >= 1` | `{ hits: [...] }` |
| `neighbours` | `handle: handle`, `count: integer >= 1` | `{ passages: [...] }` |
| `provenance` | `handle: handle` | `{ chunk: ..., fetched_at: ..., ttl_seconds: ... }` |

The server accepts no additional tool arguments. `ttl_seconds` may be zero;
the library receives it as a `Duration` without a default being invented.

### Batch add

`add` requires exactly this argument object:

```json
{
  "origins": ["path-or-url", "another-path-or-url"],
  "ttl_seconds": 3600
}
```

`origins` may be empty. The server parses every string before invoking the
library, normalizes each origin, and folds duplicates into one canonical set.
One invalid origin rejects the whole tool call before any member is fetched or
written. A valid request never enumerates or deletes an origin omitted from
the set.

The structured add result is:

```json
{
  "status": "complete",
  "items": [
    { "origin": "...", "status": "upserted" },
    {
      "origin": "...",
      "status": "refused",
      "error": { "kind": "not_found | not_text", "message": "..." }
    }
  ]
}
```

Items are in canonical origin order. The other item statuses are `failed`,
`uncertain`, and `not_attempted`. `failed` and `uncertain` carry an error with
kind `io`; `not_attempted` has no error. A result containing only `upserted`
and `refused` items has `status: "complete"` and `isError: false`. A result
containing `failed`, `uncertain`, or `not_attempted` has `status: "halted"` and
`isError: true`, while retaining every item in `structuredContent`.

Refused members do not stop the sequential operation. A failed preparation or
a failed store write phase stops later members; earlier committed members
remain visible. Each successful member has its own per-origin commit and
`fetched_at`, while the supplied ttl is shared by all successful members.

The process remains synchronous. A client disconnect or lost response does
not cancel work or roll back committed members, and a retry is a new set
upsert rather than an assumption about the lost report.

## Handle wire form

A handle crosses MCP as an object:

```json
{
  "origin": "...",
  "start": 0,
  "end": 10,
  "digest": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}
```

`start` and `end` are byte offsets. `digest` is the lower-case hexadecimal
SHA-256 digest of the text at that span. The server accepts either hex case and
always emits lower case. Rehydration rejects only a reversed span or malformed
origin/digest; the library verbs decide whether the claim is `Stale` or `Gone`.

Passages contain `handle` and `text`. A provenance result contains a `chunk`
with `origin`, `start`, and `end`, plus `fetched_at` as `{ unix_seconds,
nanoseconds }` and the supplied `ttl_seconds`.

## Tool errors

A tool execution error is an MCP result with `isError: true`, a text content
item, and structured content of this form:

```json
{
  "error": {
    "kind": "not_found | not_text | stale | gone | invalid_input | io | serialization",
    "message": "..."
  }
}
```

The named refusal kinds are the library's decisions. `add` reports its member
refusals inside a complete structured result rather than as a tool error.
`invalid_input` covers a caller argument or origin that cannot be represented
as a library value; those failures happen before any add member starts. `io`
covers weather at the process boundary and appears on halted add items when a
member preparation or write fails. A malformed JSON-RPC request is a protocol
error, not a tool result.
