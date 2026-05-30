# Verifier v0

Verifier v0 joins declared axioms, semantic capability evidence requirements,
observed evidence, and target evidence contracts into one deterministic verdict.
It does not prove invariants formally, invoke a model, edit files, or change
runtime capability gates.

## Command

```bash
axiomc verify <path> --json
```

The command exits zero only when every declared axiom is `verified` and every
declared target evidence requirement has at least one `passing` evidence item.
It exits non-zero when an axiom is `unverified`, an axiom is `violated`, or a
target requirement is `missing` or `failing`.

## Axiom Verdicts

- `verified`: every evidence item required by a capability preserving the
  axiom has observed `passing` evidence.
- `unverified`: the axiom has no backing evidence, references undeclared
  evidence, or declared evidence has no observed passing item.
- `violated`: at least one backing evidence item is observed as `failing`.

The JSON report emits `verified_by` edges from each axiom to its backing
evidence node. Non-verified axioms also emit `violates` edges from structured
diagnostics to the affected axiom.

## Target Contracts

If `<path>/targets.json` exists and follows Backend Target Interface v0,
`axiomc verify` checks each target contract's `evidence_requirements` against
the observed evidence report. A requirement is `passing` when at least one
observed evidence item of that type is passing.

## Schema

The JSON envelope is described by
`stage1/schemas/axiom-verify-v0.schema.json`.

## Boundaries

Verifier v0 is evidence-backed, not theorem-proved. It is a gate over explicit
semantic declarations and objective evidence already available to the package.
