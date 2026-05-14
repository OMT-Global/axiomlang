---
parent: 105
title: "Direct native backend C: replace `rustc` invocation as the default"
labels: [stage1, post-threshold, phase-b, area:runtime, lane:daedalus]
depends_on: [105-b-mir-to-native]
---

Part of #105. Flip the default backend to cranelift once parity is demonstrated, leaving the generated-Rust path opt-in for fallback.

## Scope

- `axiomc build` (no flag) lowers through cranelift.
- `--backend rust` remains accepted for at least one release for fallback / comparison.
- Cache-key metadata in `axiomc build --json` distinguishes the backend so cache hits don't cross paths.

## Acceptance

- `make stage1-test stage1-smoke` passes with cranelift as default.
- Benchmark fixture in `stage1/benchmarks/` shows the new median compile time and is committed.

## Depends on

- 105-b-mir-to-native.

## Out of scope

- Removing the generated-Rust backend entirely — 105-d.
