# Harness support

This document records the MCP harness support matrix. The target harnesses are
Claude Code, VS Code, and Codex.

## Support record

For each harness, the support surface includes:

- instructions for getting started;
- a configuration that starts the local `scry-mcp` stdio server;
- a check that the harness discovers and invokes the five tools; and
- an automated integration check where the harness provides a usable CI
  surface.

These are support requirements, not separate scry products. The corpus model,
tool names, handles, and provenance semantics remain the same across harnesses.

| Harness | Target | Current setup | Current automated coverage |
| --- | --- | --- | --- |
| Claude Code | Yes | Source-checkout command below; no checked-in Claude configuration | None |
| VS Code | Yes | Checked-in `.vscode/mcp.json`; discovery and Chat use manually verified | MCP process test; host steps manually verified |
| Codex | Yes | Source-checkout command below; no checked-in Codex configuration | None |

## Common server command

The current source-checkout server requires explicit paths:

```text
cargo run --quiet --manifest-path <repo>/Cargo.toml
    --package scry-mcp --
    --store <repo>/scry.redb
    --cache <model-cache>
```

Replace the placeholders with absolute paths. The first start builds the
workspace and loads the pinned model assets. A packaged executable is not yet
available.

## Claude Code

From a project using the repository, register the local stdio server with
project scope:

```text
claude mcp add --transport stdio scry --scope project -- cargo run --quiet --manifest-path "<repo>/Cargo.toml" --package scry-mcp -- --store "<repo>/scry.redb" --cache "<model-cache>"
```

Check the configuration and connection with:

```text
claude mcp list
claude mcp get scry
```

## VS Code

The repository includes [`.vscode/mcp.json`](../.vscode/mcp.json). Open the
repository in VS Code, allow the `scry` server to start, and use the MCP server
list to confirm that it is running and that five tools were discovered.

The checked-in configuration runs Cargo with the workspace corpus at
`${workspaceFolder}/scry.redb` and the model cache at
`${userHome}/.cache/scry-model-cache`.

## Codex

Register the local stdio server with the Codex CLI:

```text
codex mcp add scry -- cargo run --quiet --manifest-path "<repo>/Cargo.toml" --package scry-mcp -- --store "<repo>/scry.redb" --cache "<model-cache>"
```

Check the configuration with:

```text
codex mcp list
```

The server should appear in the list with the five scry tools.

## Automated checks

[`crates/scry-mcp/tests/e2e.rs`](../crates/scry-mcp/tests/e2e.rs) starts the
actual binary and drives `initialize`, discovery, `add`, `search`, handle
exchange, `provenance`, `neighbours`, `delete`, and the resulting `Gone`
response over stdio. It is the current process-level integration check and can
be run with:

```text
cargo test --workspace --all-targets
```

The repository's GitHub Actions workflow runs this process-level check, along
with formatting and Clippy, on pushes and pull requests. It does not start
Claude Code, VS Code, or Codex. Headless harness-level integration checks remain
separate support work.

The current packaging state and unresolved release choices are recorded in
[packaging.md](product-planning/packaging.md).
