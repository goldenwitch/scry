# Personas

This document records the target workflows and their stated needs. The target
workflows are Research, Developer, and Analyst. They use the same corpus and
MCP contract; they are not separate products.

## Shared needs

- Search corpus material by meaning.
- Receive passage text with an origin-bearing handle.
- Retrieve passages adjacent to a useful hit.
- Retain the source identity and passage location for material used in an
  answer.
- Use the same corpus through the supported agent harnesses.

## Target workflows

| Workflow | Stated use | Additional needs |
| --- | --- | --- |
| Research | Make a collection of papers available to an agent for search. | Add multiple paper origins to one corpus; refresh a paper by adding its origin again. |
| Developer | Work with material available from a development workspace through an agent harness. | Get started from Claude Code, VS Code, or Codex; make local workspace material available as corpus origins; avoid a harness-specific workflow that changes corpus or handle semantics. |
| Analyst | Aid legal analysis over a large corpus. | Support local URI forms for corpus origins, including network share origins; define the operating regime for a large legal corpus. |

## Open requirements

- Research needs transparent PDF ingestion that preserves the required source
  information. See [pdf-ingestion.md](pdf-ingestion.md).
- Developer setup needs a release package and harness-level automated checks.
  See [packaging.md](packaging.md) and [harness-support.md](../harness-support.md).
- Analyst setup needs accepted local URI forms, including network share
  semantics, and measurements for a large legal corpus.
