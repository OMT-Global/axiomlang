---
parent: 562
title: "Phase-J.3: Rust bootstrap is no longer needed"
labels: [area:tooling, roadmap, lane:pheidon, risk:high, status:needs-human-approval, phase-j]
depends_on: [562-j2-property-test-gate]
---

Part of #562. `axiomc check → build → run` produces native binaries with no Rust dependency. The compiler's own test suite runs via `axiomc test --properties`.

## Scope

- The user-facing toolchain entrypoint is `axiomc`; the Rust crate that today implements `axiomc` becomes an implementation detail (or is removed, depending on the direct-backend status).
- The compiler bootstraps itself through `axiomc` — no `cargo` step required to produce a working compiler binary.
- `make stage1-test` reduces to `axiomc test --properties` plus the proof workloads.

## Acceptance

- A fresh checkout can build a working `axiomc` binary using only a previously-shipped `axiomc` snapshot — no `cargo` invocation needed.
- All compiler-internal coverage is in AxiOM property form.

## Depends on

- Phase-J.1 and J.2.
- (Likely) #105 direct native backend lineage — see #105 decomposition.

## Out of scope

- Doc generator and LSP server — Phase-K and Phase-L.
