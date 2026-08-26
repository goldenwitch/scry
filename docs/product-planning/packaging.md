# Packaging

This document records the current packaging state and the requirements for
plug-and-play use. It does not choose a release or installer design.

## Current facts

- The MCP binary is named `scry-mcp`.
- The current setup builds and starts it from a source checkout through Cargo.
- Startup requires explicit `--store` and `--cache` paths.
- Startup loads the pinned embedding model before the MCP session begins.
- The repository contains a workspace-level VS Code configuration.
- The repository does not contain a release executable, installer, package
  manager formula, or generated Claude Code or Codex configuration.
- The corpus is a local `redb` file and the model assets are held in a caller-
  supplied cache directory.

## Stated requirements

- Developers and researchers can start scry from Claude Code, VS Code, or Codex
  without writing a custom MCP adapter.
- Each supported harness has a getting-started instruction path.
- The same corpus and model-cache choices can be represented in each harness.
- The installation path does not make a user understand Cargo in order to use
  a packaged scry server.
- A source-checkout path remains available for development.

## Open items

- Which executable artifacts and operating-system targets are released.
- Whether model assets are downloaded on first start, installed separately, or
  bundled with a package.
- Where the default corpus and model-cache paths live on each platform.
- How a user selects a corpus when more than one workspace or share is in use.
- Whether harness configuration is generated, checked in, or entered by the
  user.
- How upgrades preserve model compatibility with an existing corpus.
- Which packaging and installation checks run in CI for Claude Code, VS Code,
  and Codex.
