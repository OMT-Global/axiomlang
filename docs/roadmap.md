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
- Syntax, HIR, MIR, and a direct-native Cranelift build pipeline for supported
  user programs.
- Package-local modules, local path dependencies, and workspace member
  selection.
- Native `check`, `build`, `run`, `test`, `doc`, `lsp`, and capability commands.
- Capability-gated runtime surfaces for clock, environment, filesystem,
  network, process, and crypto access.
- Formatter, benchmark, documentation, local publishing, registry validation,
  and package inspection surfaces.
- A checked conformance corpus and property/evidence gates.
- Agent-facing semantic declarations, inspection, evidence, verification,
  repair plans, provenance, semantic diff, decision records, target contracts,
  and OpenAPI/policy/SQL/OpenTofu/runbook generators.
- Direct-native runtime ABI and Rust-exit command-surface readiness for user
  programs; generated Rust is no longer a supported CLI backend.

## Current Focus

- Complete host exit rather than redoing backend exit: decompose the Rust
  compiler (#1254), close the compiler-workload language/ABI gaps (#1425 and
  #1426), prove a compiler-scale AxiOM package and command surface (#1427), and
  prove the Cargo-free snapshot chain (#1428) before the final #721 decision.
- Emit complete Intent IR for real packages (#1418) so the semantic APIs share
  one canonical graph rather than partial, command-specific views.
- Advance the [Autonomous Agent Execution Roadmap](autonomous-agent-roadmap.md)
  (#1417 and #1419-#1424) from typed authority through transactional execution,
  impact-aware evidence, independent review, delivery, and recovery.
- Keep diagnostics, conformance, capability-denial, security, and performance
  evidence expanding with every behavior change.

## Longer-Term Work

- Additional native targets and production-grade async/I/O runtime surfaces.
- Hosted registry and ecosystem services after local package, trust, and
  provenance contracts remain stable under real use.
- Richer semantic generators and verifiers driven from complete Intent IR.
- Policy-scoped unattended maintenance across multiple packages and
  repositories after the single-repository autonomy evaluation gate is green.
