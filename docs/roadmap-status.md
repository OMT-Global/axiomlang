# Roadmap Status Ledger

This is the issue-facing status layer for broad roadmap, self-hosting,
agent-native, and governance work. GitHub issues remain the source of execution
authority; this ledger records which issue family is current and which gates
define completion.

## Control Plane

- `project.bootstrap.yaml` is the source of truth for repository governance,
  reviewers, CODEOWNERS, required checks, environments, runner policy, and home
  profile sync.
- `docs/bootstrap/onboarding.md` records PR, validation, environment, and runner
  expectations.
- `docs/stage1.md` describes the supported Rust-hosted `axiomc` workflow.
- `docs/roadmap-agent-native-ledger.md` resolves overlapping issue families.
- Machine-readable readiness reports, not prose checkboxes, determine technical
  status.

## Current Technical State

- Python stage0 is retired from the supported toolchain.
- Direct-native Cranelift is the supported user-program backend; generated Rust
  is not a supported CLI backend.
- The direct-native ABI, six official command surfaces, full-lib triage, compiler
  package-boundary checks, and live Rust-exit gate are green.
- The compiler itself is not self-hosting. It remains predominantly Rust,
  self-hosting language readiness is red, and no genesis snapshot is pinned.
- Semantic inspection, evidence, verification, repair planning, provenance,
  semantic diff, decisions, target contracts, and artifact generators are
  shipped foundations. Full real-package Intent IR emission remains open.

## Active Roadmap Families

| Family | Canonical issues | Status | Execution rule |
| --- | --- | --- | --- |
| Backend exit for user programs | #1124, #1191, #1255, #731 | Complete | Keep the readiness gates green; do not reopen historical generated-Rust work without a regression. |
| Host exit / self-hosting | #565, #721 | Active, early | Treat #565 as the thesis and #721 as the final Class 3 gate. |
| Compiler source decomposition | #1254 | Active | Continue shrinking Rust monoliths along AxiOM package boundaries under the enforced ratchet. |
| Self-hosting language/backend gaps | #1366, #1425, #1426 | Ready for planning | Runtime-sized collections and the string/slice parameter ABI need executable direct-native evidence. |
| Compiler-scale AxiOM proof | #1427 | Blocked on language/ABI leaves | Turn the two remaining language-readiness proof rows green from a real multi-package workload. |
| Snapshot bootstrap | #1428 | Human-gated release work | Pin the genesis snapshot and prove offline build/test, no Cargo after genesis, and fixpoint evidence. |
| Unattended agent coding | #1417-#1424 | Ready for staged planning | Follow `docs/autonomous-agent-roadmap.md`; do not skip semantic authority, containment, evidence, or independent review. |
| Repeatable autonomy validation | #1430 | Ready for implementation | Make the compiler-property fast check portable, repeatable, parallel-safe, and self-cleaning on macOS/BSD `mktemp`. |
| Repository branch hygiene | #1164 | Narrow maintenance remainder | Preserve protected/historical branches while reducing the remaining remote-branch inventory. |

## Readiness Commands

```bash
make stage1-direct-native-runtime-abi
make rust-exit-command-surface-coverage
make rust-exit-readiness-github
make self-hosting-language-readiness-github
make snapshot-bootstrap-readiness
make stage1-compiler-source-monoliths
```

Expected current outcomes:

- direct-native ABI, command surface, Rust exit, and monolith ratchet: green;
- self-hosting language readiness: red until #1425-#1427 are executable;
- snapshot bootstrap: red until #1428 pins and proves the chain.

## Issue Lifecycle Rules

- Close completed leaf and umbrella issues when their acceptance criteria have
  merged evidence; do not leave finished work masquerading as backlog.
- Reopen or replace an issue when its docs still assign unfinished canonical
  work to it.
- Broad roadmap issues must point to current leaf contracts rather than serving
  as free-roaming work queues.
- A blocked readiness row must name the issue that can actually make it green;
  do not point a prerequisite only at its own final umbrella.
- Any semantic or autonomy feature needs docs, schema or schema delta, fixtures,
  validation commands, Rust-capture analysis, and an explicit autonomy class.
- No agent may approve its own PR. Merge-capable autonomy requires independent
  review and explicit repository policy under #1423.

## Reconciliation Date

This ledger was reconciled against live GitHub state and executable readiness
reports on 2026-07-09. Refresh both sources before making a closure or release
decision.
