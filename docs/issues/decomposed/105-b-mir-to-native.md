---
parent: 105
title: "Direct native backend B: MIR-to-native lowering for full language surface"
labels: [stage1, post-threshold, phase-b, area:runtime, lane:daedalus]
depends_on: [105-a-cranelift-spike]
---

Part of #105. Extend the direct backend so it can lower the entire MIR surface that the Rust backend handles today (ownership moves, borrows, slices, enums, generics, async runtime calls).

## Scope

- Each MIR opcode maps to a Cranelift IR sequence.
- Heap layout for `String`, `Vec<T>`, `HashMap<K,V>`, etc. mirrors the layout produced by the generated-Rust backend so existing FFI assumptions hold.
- Capability host calls (env / fs / net / process) lower through a thin C ABI shim.

## Acceptance

- `axiomc build --backend cranelift` succeeds for every fixture in `stage1/conformance/pass/`.
- Output binaries pass the same stdout/stderr golden checks as the generated-Rust path.

## Depends on

- 105-a-cranelift-spike (the backend crate exists).

## Out of scope

- Default backend switch — 105-c.
- Removing the generated-Rust path — 105-d.
