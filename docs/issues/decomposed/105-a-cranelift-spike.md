---
parent: 105
title: "Direct native backend A: Cranelift integration spike"
labels: [stage1, post-threshold, phase-b, area:runtime, lane:daedalus]
---

Part of #105. Stand up a minimal Cranelift-based backend that can emit one well-defined AxiOM program to a native object file, in parallel with (not replacing) the generated-Rust path.

## Scope

- New `stage1/crates/axiomc-backend-cranelift` crate behind a `--backend cranelift` flag.
- Lower `axiomc build stage1/examples/hello` through Cranelift to a native object file.
- Link the object using the host's system linker; `axiomc run` of the resulting binary prints the expected stdout.

## Acceptance

- `axiomc build stage1/examples/hello --backend cranelift` produces a working binary.
- `--backend rust` (the default) remains the production path; the cranelift backend is opt-in.
- Compile-time of the hello-world example via cranelift is measured and recorded in a benchmark fixture.

## Out of scope

- Full MIR lowering — separate sub-issue.
- Replacing `rustc` invocation as the default — separate sub-issue.
- Removing the generated-Rust path — separate sub-issue.
