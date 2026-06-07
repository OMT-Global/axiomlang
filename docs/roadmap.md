# Roadmap

This file tracks the Rust compiler line under `stage1/` and the emerging
agent-native semantic layer above it. New implementation work should target the
Rust-only `axiomc` workflow unless a linked issue explicitly defines another
target.

For the agent-native direction, see [vision.md](vision.md). For the boundary
between Rust implementation details and Axiom semantics, see
[rust-bootstrap-boundary.md](rust-bootstrap-boundary.md). For the planned
self-hosted compiler package layout, see
[axiom-compiler-source-layout.md](axiom-compiler-source-layout.md). For backend
and implementation-language positioning, see
[positioning/implementation-languages.md](positioning/implementation-languages.md).

For issue-level roadmap disposition, current execution scope, and deferred
ecosystem work, see the [Roadmap Status Ledger](roadmap-status.md).
For agent-native semantic-layer canonical issues and duplicate handling, see
the [Agent-Native Roadmap Ledger](roadmap-agent-native-ledger.md).

The Python `stage0` interpreter and bytecode VM are retired as supported
implementation surfaces; see
[Python Exit VM Disposition](python-exit-vm-disposition.md) and the
[Python Exit Parity Gate](python-exit-parity-gate.md).

## Completed Foundations

- Package manifests with `axiom.toml` and `axiom.lock`.
- Syntax, HIR, MIR, and a backend-driven native build pipeline with preparatory seam work for later native-backend expansion, as part of #105 rather than completion of it.
- Package-local modules, local path dependencies, and workspace member
  selection.
- Native `check`, `build`, `run`, `test`, and `caps` commands.
- Capability-gated runtime surfaces for clock, environment, filesystem,
  network, process, and crypto access.
- A Rust-run conformance corpus under `stage1/conformance`.

## Current Focus

- Expand the conformance corpus for negative semantic coverage, capability
  denials, module visibility, and cross-package behavior.
- Improve diagnostics with richer spans, notes, and stable machine-readable
  error codes.
- Introduce the agent-native semantic lane schema-first and fixture-backed:
  Intent IR, effect graph, evidence model, artifact plan, provenance, and
  structured repair plans.
- Continue the agent-grade compiler milestone in
  [stage1-agent-grade-compiler.md](stage1-agent-grade-compiler.md).

## Longer-Term Work

- Direct backend replacement for the generated-Rust path.
- Formatter, benchmark harness, doc generator, publisher, and LSP support.
- Service-grade async and I/O runtime surfaces.
- Backend target interfaces for code, service contracts, policy bundles,
  infrastructure modules, documentation, and runbooks.
