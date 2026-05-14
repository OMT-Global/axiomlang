---
parent: 561
title: "Phase-I.2: conformance corpus as AxiOM property tests"
labels: [area:tooling, roadmap, lane:ares, risk:high, status:needs-human-approval, phase-i]
depends_on: [561-i1-replace-cargo-test-stdlib]
---

Part of #561. The 63-program conformance corpus becomes a set of AxiOM property tests rather than externally driven `.ax` files.

## Scope

- Convert each `stage1/conformance/pass/*` and `stage1/conformance/fail/*` fixture into an AxiOM property that the test runner executes directly (or wraps the fixture invocation in an AxiOM property).
- `axiomc test --conformance` compiles and runs each fixture.
- Fixture failures surface through the AxiOM property error format from Phase-H.1, not via `cargo test` panic captures.
- Stable result counts emitted in the JSON envelope (parity with `tests::conformance_corpus_reports_stable_results`).

## Acceptance

```
axiomc test --conformance
# 63/63 properties passed
```

- The conformance manifest still drives the fixture list, but the runner is AxiOM.
- A negative fixture's `expected-error.json` is consumed by the AxiOM property as input data.

## Depends on

- Phase-I.1 (stdlib already migrated).

## Out of scope

- Compiler-internal tests — Phase-J.
- New conformance fixtures — independent.
