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
For the dependency-ordered path to production CLI, worker, HTTP service, and
self-hosted compiler workloads, see the
[Production Language Roadmap](production-language-roadmap.md).

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
- Direct-native object emission and Rust-exit command-surface structure for
  user programs; generated Rust is no longer a supported CLI backend. Runtime
  truth beyond the supported native lowering subset remains gated by #1434.

## Current Focus

- Restore runtime truth first: prohibit effectful compile-time replay and fail
  closed on unsupported native lowering (#1434), define executable MIR (#1437),
  and establish the runtime lifecycle ABI (#1438).
- Execute the [Production Language Roadmap](production-language-roadmap.md)
  (#1432) in dependency waves from runtime values and ownership through serious
  CLI/worker/service capabilities, productized tooling, and final workloads.
- Complete host exit after the production foundation: close the
  compiler-workload gaps (#1425-#1427), migrate compiler source packages in
  AxiOM (#1468-#1475 and #1478-#1479), prove the Cargo-free snapshot chain
  (#1428), and reserve the final #721 decision for the exact release candidate.
- Use complete Intent IR emission for real packages and workspaces (#1418) so
  semantic APIs share one canonical graph rather than partial,
  command-specific views.
- Advance the [Autonomous Agent Execution Roadmap](autonomous-agent-roadmap.md)
  (#1417 and #1419-#1424) from typed authority through transactional execution,
  impact-aware evidence, independent review, delivery, and recovery.
- Keep diagnostics, conformance, capability-denial, security, and performance
  evidence expanding with every behavior change.

## Longer-Term Work

- Additional native targets beyond Linux x86-64 and macOS arm64 after #1455.
- Hosted registry service ergonomics after trusted resolution, signatures,
  lockfiles, and vendoring are qualified by #1458 and #1459.
- Richer semantic generators and verifiers driven from complete Intent IR.
- Policy-scoped unattended maintenance across multiple packages and
  repositories after the single-repository autonomy evaluation gate is green.
