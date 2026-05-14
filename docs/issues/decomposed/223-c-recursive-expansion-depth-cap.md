---
parent: 223
title: "Declarative macros C: recursive expansion with bounded depth"
labels: [phase-a, area:lang, lane:daedalus]
depends_on: [223-b-hygiene-model]
---

Part of #223. Allow macros to invoke other macros (including themselves) with a hard recursion-depth cap to prevent runaway expansion.

## Scope

- Default depth cap of 64 levels.
- `--macro-recursion-limit N` flag overrides per invocation.
- A macro that recurses past the cap fails with a diagnostic that lists the invocation chain.

## Acceptance

- Conformance pass fixture: a macro that recursively builds a 16-element tuple expands successfully.
- Conformance fail fixture: a macro that recurses unconditionally hits the cap and produces a deterministic diagnostic.

## Depends on

- 223-b (hygiene).

## Out of scope

- `axiomc check` integration — 223-d.
