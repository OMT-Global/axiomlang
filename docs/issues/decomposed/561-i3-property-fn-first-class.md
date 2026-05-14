---
parent: 561
title: "Phase-I.3: property tests as first-class AxiOM constructs"
labels: [area:stdlib, roadmap, lane:ares, risk:high, status:needs-human-approval, phase-i]
depends_on: [560-h1-property-clause]
---

Part of #561. Promote `property fn` from "parsed clause" to a fully type-checked, ownership-verified language construct so that an LLM writing a property is fully validated.

## Scope

- `property fn <name>(input: T)` participates in the borrow checker and ownership rules like any other function.
- `axiomc check --properties` runs all property clauses against the strategy-table inputs and reports failures with type-rich diagnostics.
- The test runner binary produced by `axiomc build std/testing.ax` is itself an AxiOM binary built by the AxiOM compiler.

## Acceptance

- A `property fn` that borrows from its input checks under the borrow rules; an aliasing-mutable borrow fails with the existing borrow diagnostic.
- `axiomc check --properties` is a recognized command flag and is documented in `docs/stage1.md`.
- A failing property with a captured-by-reference state produces a diagnostic that includes the failing input and the borrow region.

## Depends on

- Phase-H.1 (property clause exists).
- Phase-I.1 / I.2 (other stdlib + conformance pieces).

## Out of scope

- Compiler-internal tests — Phase-J.
