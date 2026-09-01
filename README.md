# scry

scry is an MCP server that gives agents a persistent semantic corpus. It makes
local and remote text available for grounded work without requiring the source
material to be copied into every prompt.

## What the product is

- A local, persistent collection of source material for an agent.
- Meaning-based retrieval across that collection.
- Relevant passages that retain their source origin.
- A path from an answer back to the material it relies on.
- Deliberate source refresh and removal without changing the original files or
  URLs.

The corpus is persistent and freshness is explicit. scry records when source
material was collected and the requested lifetime for that material; it does
not refetch sources in the background.

The connected MCP harness discovers the server's available capabilities and
their input and output shapes at runtime. This README covers the product and
server setup; capability details belong to MCP discovery.

## CI benchmark baseline

The [CI workflow](.github/workflows/ci.yml) checks the pinned [v1 benchmark
artifact](benchmarks/baseline-v1.json) on pushes and pull requests. These are
algorithmic work and logical-resource reference values, not machine-speed
scores or elapsed-time claims:

| Measure | Baseline value |
| --- | ---: |
| Static MatMul work | `688,914,432` MACs |
| Static FMA work | `1,377,828,864` FMA FLOPs |
| Sliced spans | `10` |
| Embedding calls / vectors | `3 / 11` |
| Redb writes / reads | `3 / 4` |
| Add members: upserted / refused | `3: 2 / 1` |
| Owned logical-byte high-water | `13,343 bytes` |

The runtime rows come from the mixed v1 workload, which also exercises search,
neighbours, provenance, and delete. The add-only diagnostic matrix and its
interpretation are recorded in [performance.md](performance.md).

## Solve common problems

### Compare a collection of papers

Make the paper text available to your agent, then ask it to compare the papers
using supporting passages and source origins rather than a summary alone.

HTML and text pages work today. Direct PDF URLs are not currently supported;
extract a PDF to UTF-8 text or Markdown first.

### Understand an unfamiliar codebase

Make the relevant source and documentation available to your agent, then ask
questions about behavior, configuration flow, or the relationship between
components. Ask for the supporting passages and their source origins.

### Check evidence before relying on it

Ask for the passages behind an answer and require the agent to check their
source origins before using them in a report, decision, or implementation.

## Get started

The current setup runs the local `scry-mcp` server from a source checkout with
Cargo.

### Requirements

- Windows, macOS, or Linux.
- Rust and Cargo.
- An MCP-capable agent harness.
- Network access for the first model download and for remote text origins.

Clone the repository:

```sh
git clone https://github.com/goldenwitch/scry.git
cd scry
```

The server uses a local `scry.redb` file for the corpus and a user cache for
the embedding model. The first start builds the server and downloads the model
assets when they are not already cached.

Connect the server to your harness using the [harness setup guide](docs/harness-support.md).
The repository includes a ready-to-use [VS Code MCP configuration](.vscode/mcp.json).

### Start it manually

Run this from the repository root to see startup errors:

```sh
cargo run --quiet --package scry-mcp -- --store ./scry.redb --cache "$HOME/.cache/scry-model-cache"
```

Startup loads the embedding model before the MCP session begins. Check that
Rust and Cargo are installed, the cache directory is writable, and the corpus
file was created by a compatible scry build.

## Links

- [Contributing](CONTRIBUTING.md)
- [Harness setup](docs/harness-support.md)
- [VS Code MCP configuration](.vscode/mcp.json)
- [Benchmark interpretation](docs/benchmarks.md)
- [License](LICENSE)
