---
parent: 105
title: "Direct native backend D: remove the generated-Rust backend"
labels: [stage1, post-threshold, phase-b, area:runtime, lane:daedalus]
depends_on: [105-c-replace-rustc-invocation]
---

Part of #105. After cranelift has been the default for a release with no parity escapes, delete the generated-Rust path.

## Scope

- Remove `stage1/crates/axiomc/src/codegen.rs` Rust-emitter helpers; remove `--backend rust` flag.
- Update docs (`docs/stage1.md`, `docs/package.md`) to describe the single native backend.
- Remove `rustc` from the toolchain bootstrap once `axiomc` itself is built via Phase-J.3.

## Acceptance

- `cargo test --manifest-path stage1/Cargo.toml -p axiomc` has no codegen-rs Rust-emitter tests.
- The CI matrix no longer installs `rustc` for the AxiOM compile lane (it still needs rustc to build `axiomc` itself, until Phase-J.3 removes that need too).
- Release notes call out the removal and reference the last release that supported `--backend rust`.

## Depends on

- 105-c-replace-rustc-invocation (cranelift was default for at least one release).
- Phase-J.3 closes the loop on the bootstrap.
