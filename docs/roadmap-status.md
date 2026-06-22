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
- `docs/rust-exit-readiness.md` is the readiness matrix for removing Rust,
  Cargo, generated Rust, and `rustc` from the supported toolchain.
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
- `make rust-exit-readiness` as a non-blocking evidence gate while Rust exit
  issues remain open.
- Bootstrap governance defined in `project.bootstrap.yaml` and
  `docs/bootstrap/onboarding.md`.

The following are explicitly outside the current agent-grade bar unless a new
issue scopes them with testable acceptance criteria and owner approval:

- Hosted package registry service design or operation.
- Hosted package upload workflows beyond local static registry publishing.
- Hosted registry trust roots and package-authenticity services beyond local
  static publishing, npm audit signatures when a root `package-lock.json`
  exists, cargo-vet, and the stage1 SBOM gate.
- Direct-native backend replacement.
- Post-agent-grade ecosystem services.

## Issue Disposition

| Issue | Status | Disposition | Evidence and rationale |
| --- | --- | --- | --- |
| [#264](https://github.com/OMT-Global/axiomlang/issues/264) Roadmap parity and agentic-native lead | Complete with this ledger | Close as completed when this PR lands | The broad roadmap is now represented by `docs/roadmap.md`, this ledger, and the AG0-AG5 execution contract. Future work should use scoped child issues rather than keeping the umbrella issue open as an implicit backlog. |
| [#263](https://github.com/OMT-Global/axiomlang/issues/263) Hosted package registry | Deferred outside current bar | Keep open until implemented or formally descoped | A hosted registry depends on publish, signed packages, trust roots, and service ownership. The current repo has no registry service and the agent-grade bar explicitly excludes registry publishing. |
| [#245](https://github.com/OMT-Global/axiomlang/issues/245) `axiomc publish` and package registry | Implemented as local static registry publishing | Close as completed when this PR lands | `axiomc publish` now validates the lockfile, packs a deterministic `package.axp`, writes an `axiom-hmac-sha256-v1` sidecar bound to a required `--signing-key`, and stages releases for `axiomc registry-index`. Hosted registry operation remains covered by #263. |
| [#248](https://github.com/OMT-Global/axiomlang/issues/248) Lockfile integrity and signed packages | Implemented as the stage1 supply-chain gate | Close as completed when this PR lands | `make supply-chain` runs the pinned cargo-vet policy under `stage1/supply-chain`, verifies locked offline Cargo metadata, performs a locked release build with deterministic path/time inputs, verifies signed npm packages when a root `package-lock.json` exists, and emits `stage1/target/sbom/stage1.spdx.json`. `docs/supply-chain.md` records the operator contract and the workflow skips Node setup when no npm lockfile exists, preserving the signed-package check without forcing unused Node extraction on self-hosted runners. Hosted registry trust roots remain covered by #263. |
| [#561](https://github.com/OMT-Global/axiomlang/issues/561) Phase-I compiler test suite in AxiOM | Complete as the shipped property-test gate | Close as completed when this PR lands | The active Phase-I child slices are closed: #712 routes stdlib verification through `axiomc test --properties`, #714 runs the conformance corpus as property-mode evidence, and #715 makes property functions first-class AxiOM constructs. `docs/stage1.md`, `scripts/ci/run-stdlib-property-checks.sh`, `scripts/ci/run-compiler-property-checks.sh`, `make stage1-test`, `make stage1-stdlib-test`, `make stage1-compiler-property-test`, and `make stage1-conformance` are the current operator evidence. Remaining Cargo/Rust-bootstrap removal stays with #719 and #721. |
| [#927](https://github.com/OMT-Global/axiomlang/issues/927) Rust exit native backend parity readiness matrix | Implemented as the initial readiness gate | Close as completed when this PR lands | `docs/rust-exit-readiness.md` and `docs/rust-exit-readiness.json` define the blocked backend/bootstrap matrix, while `scripts/ci/check-rust-exit-readiness.sh` and `make rust-exit-readiness` emit `axiom.rust_exit.readiness.v1` JSON. The gate is expected to fail until #928, #929, #693, #694, #930, #931, #932, #562, #563, and #564 are closed. |
| [#940](https://github.com/OMT-Global/axiomlang/issues/940) Rust exit: self-hosted HIR, ownership, and capability analysis | Implemented as the `compiler.hir` package boundary | Close as completed when this PR lands | `docs/axiom-compiler-source-layout.md` assigns typed declarations, name resolution, imports, capability checks, ownership and borrow validation, and property clauses to `compiler.hir`. `docs/compiler-hir-ownership-capability.md` documents the AxiOM-neutral API boundary, `stage1/compiler-contracts/snapshots/hir-ownership-capability.json` records the issue #940 fixture contract, and `scripts/ci/check-hir-boundary.py` validates source-correlated ownership, capability, and property fixtures without Rust-captured semantic wording. The closure gates are `make stage1-hir-boundary`, `make stage1-hir-boundary-test`, and `make stage1-compiler-property-test`. |
| [#101](https://github.com/OMT-Global/axiomlang/issues/101) AG5.3 proof workload fixtures | Open, blocked | Keep open | The issue requires CLI, worker, and HTTP service proof workloads. AG4.3 HTTP server support remains open, so the HTTP service fixture cannot honestly close yet. |
| [#102](https://github.com/OMT-Global/axiomlang/issues/102) AG5.4 CI closure | Open, blocked | Keep open | CI can only make proof workloads blocking after #101 exists. This remains blocked on AG5.3 and AG4.3. |
| [#397](https://github.com/OMT-Global/axiomlang/issues/397) Runtime: HTTP server runtime surface | Implemented as the current stage1 HTTP server slice | Close as completed when this PR lands | `std/http.ax` exposes loopback-only `listen`/`accept`/`respond`, `serve_once(bind, body)`, and route-shaped `fixed_route(path, body)` / `serve(bind, route, max_requests)` over `http_server_*`, `http_response_write`, `http_serve_once`, and `http_serve_route`; `std/http_async.ax` adds async-gated bounded route serving. Capability-denied programs fail before native execution via the shared `net` or `async` gates, and Rust integration coverage proves deterministic local request handling plus non-loopback bind rejection. |
| [#243](https://github.com/OMT-Global/axiomlang/issues/243) `axiomc bench` | Already shipped | Close as completed | `axiomc bench` is implemented in `stage1/crates/axiomc/src/main.rs`, documented in README and `docs/stage1.md`, and has a checked-in fixture at `stage1/examples/benchmarks`. |
| [#244](https://github.com/OMT-Global/axiomlang/issues/244) `axiomc doc` | Already shipped | Close as completed | `axiomc doc` generates Markdown and HTML docs from source doc comments in `stage1/crates/axiomc/src/main.rs` and is documented in README and `docs/stage1.md`. |
| [#247](https://github.com/OMT-Global/axiomlang/issues/247) stage1 REPL | Already shipped | Close as completed | `axiomc repl` is implemented in `stage1/crates/axiomc/src/main.rs`, includes JSON ready output, and is documented as a bootstrap-grade toolchain command in `docs/stage1.md`. |
| [#786](https://github.com/OMT-Global/axiomlang/issues/786) Backend target interface v0 | Defined as docs and schema | Close as completed when this PR lands | `docs/backend-target-interface-v0.md` describes target classes, the target contract shape, diagnostics, and mappings for the current generated-Rust backend and the direct-native backend roadmap. `stage1/schemas/axiom-target-v0.schema.json` is the machine-readable shape with a fixture at `stage1/examples/target_smoke/targets.json`. No backend implementation changes are required by v0. |

## Reopening Rule

Deferred registry and hosted publish work should remain open until the
agent-grade bar is met or until an explicit owner decision creates an ecosystem
milestone. A scoped implementation issue must name:

- the prerequisite milestone it depends on;
- the concrete `axiomc` user workflow it enables;
- the verification gate it will add or update;
- whether `project.bootstrap.yaml` needs governance, environment, or runner
  changes.
