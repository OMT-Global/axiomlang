---
parent: 562
title: "Phase-J.1: compiler's own test suite as AxiOM"
labels: [area:tooling, roadmap, lane:pheidon, risk:high, status:needs-human-approval, phase-j]
depends_on: [561-i3-property-fn-first-class]
---

Part of #562. The compiler's internal tests (borrow checker, type system, capability gates) are expressed as AxiOM properties.

## Scope

- New `main_test.ax` that contains 100 properties proving:
  - `property fn owned_string_cannot_be_borrowed_after_move()`
  - `property fn borrow_checker_excludes_aliasing_mutables(input: [int]): bool`
  - `property fn cap_gate_denies_unlisted_env()`
  - … and so on, covering type system, capability gates, borrow checker, and effect system.
- Each property compiles via `axiomc build` and runs via `axiomc test`.
- The properties cross-validate the Rust-implemented compiler: a property whose body the AxiOM language can express must agree with the compiler's own answer.

## Acceptance

- 100 compiler-internal properties pass under `axiomc test --properties`.
- Failures during compiler iteration are visible as AxiOM property errors, not Rust unit-test panics.

## Depends on

- Phase-I.3 (property-fn is first-class).

## Out of scope

- Replacing `cargo test` invocation — J.2.
- Removing the Rust bootstrap — J.3.
