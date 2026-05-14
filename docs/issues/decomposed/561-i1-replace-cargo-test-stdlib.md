---
parent: 561
title: "Phase-I.1: replace `cargo test` with `axiomc test` for the stdlib"
labels: [area:stdlib, roadmap, lane:ares, risk:high, status:needs-human-approval, phase-i]
depends_on: [560-h3-axiomc-test-properties]
---

Part of #561. The stdlib's own tests should run via `axiomc test` (using the property runner from Phase-H.3), not `cargo test`.

## Scope

- Re-express the stdlib's existing test coverage as `.ax` `property fn` clauses (or `assert_eq`-style unit tests if a property doesn't fit).
- `axiomc build std/testing.ax` produces the test runner binary.
- `axiomc run` of that binary reports `N/N properties passed` deterministically.
- Remove the stdlib-specific Rust tests once their AxiOM equivalents pass.

## Acceptance

```bash
axiomc build std/testing.ax   # produces axiom-test-runner
axiomc run axiom-test-runner  # prints "property 12/12 passed"
```

- CI lane that runs `cargo test` for the stdlib is replaced by `axiomc test`.
- All stdlib coverage is in AxiOM; no Rust test cases remain under `stage1/stdlib/`.

## Depends on

- Phase-H.3 (the property runner must work).

## Out of scope

- Compiler-internal test suite migration — Phase-J.
- Conformance corpus migration — Phase-I.2.
