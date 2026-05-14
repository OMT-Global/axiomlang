---
parent: 562
title: "Phase-J.2: `cargo test` is replaced by `axiomc test` for the stdlib"
labels: [area:tooling, roadmap, lane:pheidon, risk:high, status:needs-human-approval, phase-j]
depends_on: [562-j1-compiler-test-suite-axiom]
---

Part of #562. Remove `cargo test` from the stdlib's verification path. `axiomc test` is the only test runner the stdlib uses.

## Scope

- `cargo test --manifest-path stage1/Cargo.toml` no longer runs stdlib-internal tests; those run via `axiomc test`.
- The Makefile's `stage1-test` target invokes `axiomc test` for stdlib coverage and keeps `cargo test` only for the compiler's Rust-internal scaffolding (which J.3 will remove).
- 100+ properties from J.1 run on every CI build.

## Acceptance

- `make stage1-test` no longer exits non-zero when the Rust-side stdlib tests are removed.
- The CI summary records `properties passed: N/N` from `axiomc test --properties`.

## Depends on

- Phase-J.1.

## Out of scope

- Removing the Rust bootstrap entirely — Phase-J.3.
