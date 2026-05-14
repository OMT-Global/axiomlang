---
parent: 560
title: "Phase-H.2: `std/testing.ax` — first self-hosted stdlib module"
labels: [area:stdlib, roadmap, lane:daedalus, risk:high, status:needs-human-approval, phase-h]
depends_on: [560-h1-property-clause]
---

Part of #560. Implement `std/testing.ax` in AxiOM so that `assert_true`, `assert_eq`, and `property` are stdlib surface defined in the language itself, not the Rust compiler.

## Scope

- `std/testing.ax` defines `assert_true(value: bool)`, `assert_eq<T>(left: T, right: T)`, and `property` clause helpers.
- `axiomc check` compiles `std/testing.ax` without any Rust runtime dependency.
- `axiomc build` of a program importing `std/testing.ax` produces a working binary.
- `axiomc run` of that binary prints the expected output for one passing assertion and exits non-zero on a failing assertion.

## Acceptance

- New stdlib file at `stage1/stdlib/std/testing.ax` compiles through `check → build → run`.
- Integration test in `stage1/crates/axiomc/src/lib.rs` confirms a small `.ax` program using `assert_eq` and `property` builds.
- No new Rust dependency added to the host runtime.

## Depends on

- Phase-H.1 (`property` clause must be recognized first).

## Out of scope

- The `axiomc test --properties` runner that actually executes properties — H.3.
- Property quantification or shrinking semantics.
