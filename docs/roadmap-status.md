# Roadmap Status Ledger

This ledger is the issue-facing status layer for broad roadmap, ecosystem, and
governance work. It keeps the executable plan grounded in the current
`project.bootstrap.yaml` control plane and the stage1 execution docs instead of
letting large roadmap issues become implicit work queues.

## Control Plane

- `project.bootstrap.yaml` is the source of truth for repo governance,
  reviewers, CODEOWNERS, required checks, environments, runner policy, and home
  profile sync.
- `docs/bootstrap/onboarding.md` records the PR body, validation, environment,
  and runner expectations that new roadmap PRs must follow.
- `docs/stage1.md` is the short status page for the supported Rust-only
  `axiomc` workflow.
- `docs/stage1-agent-grade-compiler.md` is the execution contract for the
  AG0-AG5 compiler track.
- GitHub issues remain the source of record for agent execution work. Broad
  roadmap issues should either point to a current execution issue, be closed
  only when already shipped, or remain open until their prerequisite milestone
  is active.

## Current Execution Bar

The current execution bar is the stage1 agent-grade compiler track. Work is in
scope when it advances one of these active contracts:

- Rust-only `axiomc` behavior under `stage1/`.
- `make stage1-test`, `make stage1-conformance`, and `make stage1-smoke`.
- The AG4/AG5 proof path in `docs/stage1-agent-grade-compiler.md`.
- Bootstrap governance defined in `project.bootstrap.yaml` and
  `docs/bootstrap/onboarding.md`.

The following are explicitly outside the current agent-grade bar unless a new
issue scopes them with testable acceptance criteria and owner approval:

- Hosted package registry service design or operation.
- Hosted package upload workflows beyond local static registry publishing.
- Signed third-party packages, SBOM emission, and registry trust roots beyond the stage1 archive-signature sidecar.
- Direct-native backend replacement.
- Post-agent-grade ecosystem services.

## Issue Disposition

| Issue | Status | Disposition | Evidence and rationale |
| --- | --- | --- | --- |
| [#264](https://github.com/OMT-Global/axiom/issues/264) Roadmap parity and agentic-native lead | Complete with this ledger | Close as completed when this PR lands | The broad roadmap is now represented by `docs/roadmap.md`, this ledger, and the AG0-AG5 execution contract. Future work should use scoped child issues rather than keeping the umbrella issue open as an implicit backlog. |
| [#263](https://github.com/OMT-Global/axiom/issues/263) Hosted package registry | Deferred outside current bar | Keep open until implemented or formally descoped | A hosted registry depends on publish, signed packages, trust roots, and service ownership. The current repo has no registry service and the agent-grade bar explicitly excludes registry publishing. |
| [#245](https://github.com/OMT-Global/axiom/issues/245) `axiomc publish` and package registry | Implemented as local static registry publishing | Close as completed when this PR lands | `axiomc publish` now validates the lockfile, packs a deterministic `package.axp`, writes an `axiom-signature-v1` sidecar, and stages releases for `axiomc registry-index`. Hosted registry operation remains covered by #263. |
| [#248](https://github.com/OMT-Global/axiom/issues/248) Lockfile integrity and signed packages | Deferred outside current bar | Keep open until implemented or formally descoped | Stage1 lockfiles are deterministic for local path graphs, but signed packages, SBOMs, and offline verification require a registry and trust model that do not exist in the current execution scope. |
| [#101](https://github.com/OMT-Global/axiom/issues/101) AG5.3 proof workload fixtures | Open, blocked | Keep open | The issue requires CLI, worker, and HTTP service proof workloads. AG4.3 HTTP server support remains open, so the HTTP service fixture cannot honestly close yet. |
| [#102](https://github.com/OMT-Global/axiom/issues/102) AG5.4 CI closure | Open, blocked | Keep open | CI can only make proof workloads blocking after #101 exists. This remains blocked on AG5.3 and AG4.3. |
| [#243](https://github.com/OMT-Global/axiom/issues/243) `axiomc bench` | Already shipped | Close as completed | `axiomc bench` is implemented in `stage1/crates/axiomc/src/main.rs`, documented in README and `docs/stage1.md`, and has a checked-in fixture at `stage1/examples/benchmarks`. |
| [#244](https://github.com/OMT-Global/axiom/issues/244) `axiomc doc` | Already shipped | Close as completed | `axiomc doc` generates Markdown and HTML docs from source doc comments in `stage1/crates/axiomc/src/main.rs` and is documented in README and `docs/stage1.md`. |
| [#247](https://github.com/OMT-Global/axiom/issues/247) stage1 REPL | Already shipped | Close as completed | `axiomc repl` is implemented in `stage1/crates/axiomc/src/main.rs`, includes JSON ready output, and is documented as a bootstrap-grade toolchain command in `docs/stage1.md`. |

## Reopening Rule

Deferred registry, publish, and supply-chain work should remain open until the
agent-grade bar is met or until an explicit owner decision creates an ecosystem
milestone. A scoped implementation issue must name:

- the prerequisite milestone it depends on;
- the concrete `axiomc` user workflow it enables;
- the verification gate it will add or update;
- whether `project.bootstrap.yaml` needs governance, environment, or runner
  changes.
