# PRDs

## Loams Web port

- [Rust port plan v2](loams-web-port.md): scope, architecture decisions, implementation tickets and release gates.
- [Inspection report](loams-web-inspection.md): upstream behavior, integration findings and verification limits.
- [Source inventory](loams-web-source-inventory.json): pinned source hashes, module contracts and static test labels.
- [Rust preview guide](../docs/src/content/docs/loams-web.md): runnable backend,
  OpenAPI and TypeScript generation, commands and current limits.
- [Conformance workbench](../conformance/README.md): traceability ledger,
  portable cases and executable observation adapter.

## Upstream Spindle convention

Spindle's PRDs, one a piece of work, decided as Hypha's `PRD.md` says. A
PRD is deleted once its work lands; this table says where each of its facts
goes then.

| A fact | Its owner once the work lands |
|---|---|
| A module's contract | its `.mli`, in odoc markup |
| What users can do, and how | the documentation site's page for it, in `docs/src/content/docs/`, its programs in `examples/` |
| What a contributor must not break | *Rules that must hold* in `AGENTS.md` |
| Which packages there are and what each links | *Layout* and *Changes and releases* in `AGENTS.md`, and `dune-project` |
| What changed for a user | `CHANGES.md`, under `## Unreleased` |
| Why it changed, and what was weighed | the commit message |
