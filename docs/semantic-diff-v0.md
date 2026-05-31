# Semantic Diff v0

Semantic Diff v0 compares two Intent IR snapshots and reports product-facing
semantic drift. It operates on `axiom.intent_ir.v0` JSON, not on source text,
AST, HIR, MIR, or generated backend code.

## Command

```bash
axiomc semantic-diff <old.json> <new.json> --json
```

The command is advisory in v0. It emits a deterministic report and exits zero
when both snapshots parse, even when the report contains breaking changes.

## Change Classes

- Added `Capability`, `Effect`, or `RuntimeSurface`: `breaking`
- Removed `Capability`, `Effect`, `Axiom`, `Artifact`, or `RuntimeSurface`:
  `breaking`
- Modified `Capability`, `Effect`, `Axiom`, `Artifact`, or `RuntimeSurface`:
  `breaking`
- Added `Function`, `Type`, `Module`, or other non-effect nodes: `additive`
- Removed implementation-shape nodes such as `Function`, `Type`, or `Module`:
  `informational`
- Added, removed, or modified Intent IR edges: `breaking`

Edge changes include relationship drift such as `requires`, `preserves`,
`allows_effect`, and `verified_by`. V0 reports those entries with `node_kind:
Edge` plus `edge_kind`, `edge_from`, and `edge_to` fields so relationship-only
snapshot drift is visible even when the node set is unchanged.

The report is sorted by severity, node kind, node id, and change type so
unordered node sets produce stable output.

## Path To Gating

Future PR policy can require a human-readable migration note for undeclared
breaking changes without adding another required CI status check. V0 keeps the
diff inspectable and deterministic first.

## Schema

The JSON envelope is described by
`stage1/schemas/axiom-semantic-diff-v0.schema.json`.
