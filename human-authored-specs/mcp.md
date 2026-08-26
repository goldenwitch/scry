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
| `add` | `origin: string`, `ttl_seconds: integer` | `{}` |
| `delete` | `origin: string` | `{}` |
| `search` | `query: string`, `count: integer >= 1` | `{ hits: [...] }` |
| `neighbours` | `handle: handle`, `count: integer >= 1` | `{ passages: [...] }` |
| `provenance` | `handle: handle` | `{ chunk: ..., fetched_at: ..., ttl_seconds: ... }` |

The server accepts no additional tool arguments. `ttl_seconds` may be zero;
the library receives it as a `Duration` without a default being invented.

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

The named refusal kinds are the library's decisions. `invalid_input` covers a
caller argument that cannot be represented as a library value, and `io` covers
weather at the process boundary. A malformed JSON-RPC request is a protocol
error, not a tool result.
