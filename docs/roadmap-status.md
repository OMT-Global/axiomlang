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
- The direct-native ABI and Rust-exit reports are structurally green for their
  declared rows, but the backend can execute unsupported programs in the
  compiler process and emit replay binaries. #1434 therefore blocks any broad
  runtime-complete, production, or self-hosting claim.
- The production-language ledger currently has 52 capability rows: 2 of 39
  production-required rows meet their target evidence tier. The remaining
  rows are intentionally `partial` or `blocked`, not missing from the plan.
- The compiler itself is not self-hosting. It remains predominantly Rust,
  the AxiOM compiler source migration has not begun, self-hosting language
  readiness is red, and no genesis snapshot is pinned.
- Semantic inspection, evidence, verification, repair planning, provenance,
  semantic diff, decisions, target contracts, and artifact generators are
  shipped foundations. Full real-package Intent IR emission remains open.

## Active Roadmap Families

| Family | Canonical issues | Status | Execution rule |
| --- | --- | --- | --- |
| Production language umbrella | #1432-#1467, #1476-#1477, #1481 | Active, dependency ordered | Execute from `docs/production-language-roadmap.md`; do not skip evidence tiers or dependency/human gates. |
| Runtime truth and executable semantics | #1434, #1436-#1440 | First critical path | Remove effectful compile-time replay, then land MIR, lifecycle, runtime values, aggregates, and ownership evidence. |
| Backend exit for user programs | #1124, #1191, #1255, #731 | Structurally shipped; runtime-truth correction open | Keep generated Rust outside the supported CLI while #1434 closes the evaluator/replay hole. |
| Host exit / self-hosting | #565, #721 | Active, early | Treat #565 as the thesis and #721 as the final Class 3 gate. |
| Compiler source decomposition | #1254 | Active | Continue shrinking Rust monoliths along AxiOM package boundaries under the enforced ratchet. |
| Self-hosting language/backend gaps | #1366, #1425-#1427, #1434, #1436-#1440, #1476-#1477 | Blocked on runtime foundation | Require build-once/run-many evidence with runtime-origin values and effects. |
| Compiler source migration | #1468-#1475, #1478-#1479 | Blocked on compiler-scale proof | Port by accepted package boundaries with Rust coexistence and rollback; never infer migration from boundary fixtures. |
| Compiler-scale AxiOM proof | #1427 | Blocked on runtime foundation | One emitted binary must process distinct runtime inputs without rebuild or compile-time effects. |
| Snapshot bootstrap | #1428 | Human-gated release work | Pin the genesis snapshot and prove offline build/test, no Cargo after genesis, and fixpoint evidence. |
| Unattended agent coding | #1417-#1424 | Ready for staged planning | Follow `docs/autonomous-agent-roadmap.md`; do not skip semantic authority, containment, evidence, or independent review. |
| Repeatable autonomy validation | #1430 | Ready for implementation | Make the compiler-property fast check portable, repeatable, parallel-safe, and self-cleaning on macOS/BSD `mktemp`. |
| Repository branch hygiene | #1164 | Narrow maintenance remainder | Preserve protected/historical branches while reducing the remaining remote-branch inventory. |

## Readiness Commands

```bash
make stage1-direct-native-runtime-abi
make production-language-readiness-validate
make production-language-readiness
make production-language-readiness-github
make rust-exit-command-surface-coverage
make rust-exit-readiness-github
make self-hosting-language-readiness-github
make snapshot-bootstrap-readiness
make stage1-compiler-source-monoliths
```

Expected current outcomes:

- production-language ledger validation: green; readiness: red with 2 of 39
  required rows at target;
- direct-native ABI, command surface, Rust exit, and monolith ratchet:
  structurally green, but not a substitute for #1434 runtime truth;
- self-hosting language entry readiness: red until the runtime foundation,
  #1425-#1427, #1476-#1477, and #1481 are executable; compiler-source ownership
  is a subsequent migration/final-host-exit gate, not an entry prerequisite;
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
