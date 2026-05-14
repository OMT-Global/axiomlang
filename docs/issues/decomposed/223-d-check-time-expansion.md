---
parent: 223
title: "Declarative macros D: expand before type-check in `axiomc check`"
labels: [phase-a, area:lang, lane:daedalus]
depends_on: [223-c-recursive-expansion-depth-cap]
---

Part of #223. Wire macro expansion into the existing `axiomc check` pipeline so that expanded code participates in type-check, borrow-check, and exhaustiveness analysis.

## Scope

- Expansion happens after parse but before HIR lowering.
- Errors thrown by expanded code report both the expansion site and the macro's definition site.
- The `axiom.stage1.v1` JSON envelope includes a `macro_expansions` field (or its own sub-schema) listing each expanded site for tooling.

## Acceptance

- A user program that uses a `vec!`-style macro builds successfully and emits the expected runtime values.
- A type error inside the expanded body produces a diagnostic with the macro call-site marked.
- `make stage1-test stage1-smoke` includes a macro-bearing fixture.

## Depends on

- 223-c (recursive expansion).

## Out of scope

- Proc macros — separate roadmap item.
