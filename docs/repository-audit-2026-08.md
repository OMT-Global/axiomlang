# Repository Audit — 2026-08

Point-in-time functionality and security review of the stage1 toolchain and the
repository's own CI/governance surface.

- Audited commit: `aa310639`
- Method: direct source reading plus executed validation commands. Every claim
  below was traced in code or reproduced by running something; assertions taken
  only from documentation are marked as such.
- This is an observation record, not a roadmap. Roadmap authority stays with
  [`roadmap-status.md`](roadmap-status.md) and
  [`production-language-roadmap.md`](production-language-roadmap.md).

## Summary

The governance and evidence discipline in this repository is stronger than the
implementation it governs, which is the correct order for a project at this
stage. The evidence-tier vocabulary is doing real work: the readiness ledger
refuses to call shipped surfaces production-qualified, and the numbers below are
not flattering to the project, which is the point.

The two defects worth acting on are both in the repository's own CI rather than
in the compiler, and both share a failure mode: **a control that reads as
implemented while not being in effect.** Neither was caught by the extensive
self-test suite, because both live in the gap between what the self-tests assert
and what the workflow actually does.

## Verified sound

These were checked specifically because a plausible reading of the docs suggested
they might be broken. They are not.

### Build effect-purity (#1434) is closed

`roadmap-status.md` and `production-language-roadmap.md` both still describe the
compile-time evaluator hole as the first product and self-hosting blocker — the
backend evaluating user code in the compiler process, with host dispatchers for
filesystem, environment, process, network, and clock effects. That description is
stale. `8ffb3adb` (#1485) closed it:

- `cranelift_backend/static_output_purity.rs:10` — `allows_static_output_evaluation`
  is fail-closed. It refuses outright if **any** known capability is enabled, and
  structurally rejects loops, closures, and `await` rather than trying to prove
  termination by evaluation.
- `cranelift_backend.rs:445` guards the **only** non-test callsite of
  `collect_output_program` (`:448`) and returns `runtime_lowering_required()` on
  refusal. The remaining callsites at `:17627`, `:18714`, `:18749`, `:18789`, and
  `:18832` are all `#[cfg(test)]` or inside `#[test]` functions; `:17637` is the
  definition.
- The effectful evaluator entry points (`eval_expr_effectful`, `eval_call_effectful`,
  `eval_cli_call`, `eval_extern_call`, `eval_async_call`, `run_function_body`) have
  no callers outside `cranelift_backend`, so no crate-internal path reaches them
  around the guard.

The roadmap documents should be reconciled against this. Routing agents at a
closed blocker wastes dispatch, and the ledger's stated reconciliation date is
2026-07-09.

### Package signature verification

`package_trust.rs` uses pure-Rust `ed25519-dalek` with `verify_strict` at
`package_trust.rs:1485` — not the dynamically loaded OpenSSL described below.
`verify_strict` is the correct choice; it rejects small-order keys and
non-canonical signatures that the permissive API accepts.

### Capability declaration hardening

- Manifest structs use `#[serde(deny_unknown_fields)]`, so a mistyped capability
  key is an error rather than a silently ignored grant.
- `unsafe_unrestricted` (`manifest.rs:291`) is not an escape hatch. It is a
  derived transparency flag in the capability SBOM, set when `env` is granted
  unrestricted or `process` is granted without a command allowlist
  (`manifest.rs:560-566`). Broad grants are surfaced rather than hidden.

### CI privilege model

- No `pull_request_target` and no `workflow_run` triggers, so the classic
  pwn-request pattern is absent.
- Workflow permissions are least-privilege (`contents: read` by default). The
  Claude workflow holds `pull-requests: write` and `issues: write` but only
  `contents: read`, so it cannot push.
- All actions are SHA-pinned with dated comments.
- Every self-hosted job that checks out PR head is gated on
  `github.event.pull_request.head.repo.full_name == github.repository`, so fork
  PRs do not execute on the persistent runners.
- `ci-gate` runs with `if: always()` and no fork guard, and is correspondingly
  hardened: base-ref checkout, `persist-credentials: false`, and an explicit
  `# SECURITY:` comment explaining the constraint.

### `unsafe` surface is smaller than it looks

A naive `grep` reports 253 `unsafe` occurrences, but most are inside emitted Rust
source strings in `codegen.rs`. Actual `unsafe` blocks, functions, and impls
number 41, concentrated in dynamic OpenSSL symbol loading
(`codegen.rs`, `cranelift_backend/host_crypto.rs`) and `registry.rs`.

## Findings

### 1. CI never ran the `axiomc` bin target — `main` was red under green checks

Tracked as #1542; fix proposed in #1544 (open, unmerged as of the audit date).

`make stage1-test` failed on `main`:

```
tests::help_describes_supported_stage1_workflows
  main.rs  asserts help contains "Pack and publish a stage1 package into a local registry tree"
  main.rs  help text now reads  "Pack, authenticate, and publish a stage1 package into a local registry tree."
```

Introduced by #1519 (`c2f73e7f`), which reworded the clap doc comment without
updating the assertion. Two further registry descriptions had drifted earlier.

The reason nothing caught it: CI enumerates cargo test targets by allowlist, and
the bin target is in none of them.

| Lane | Command | Covers |
| --- | --- | --- |
| `pr-fast-ci.yml` `full-lib-suite` | `cargo test -p axiomc --lib` | library target only |
| `run-fast-checks.sh`, `run-toolchain-supply-chain.sh`, `run-toolchain-qualification.py` | `--test <name>` / `--lib <filter>` | named integration targets |
| *(none)* | `--bin axiomc` | **115 tests, never run** |

`--lib` also excludes `tests/*.rs`, which is why those are enumerated separately.
`Makefile:173` runs `cargo test` with no target filter, so **local validation was
strictly stronger than CI** — an inversion that makes "CI is green" a weaker
statement than "I ran the tests."

`main.rs` is roughly 11.9k lines and holds the entire CLI argument and help
surface, which is the most user-facing contract in the repository.

### 2. The `.trusted-ci` checkout was pinned to PR head, not base

Tracked as #1543, fixed in #1545.

`pr-fast-ci.yml` checked `.trusted-ci` out at `github.event.pull_request.head.sha`
and then executed `.trusted-ci/scripts/ci/run-fast-checks.sh` from it on a
persistent self-hosted runner. The script-vs-data split that the two-checkout
pattern exists to provide was not in effect for `fast-checks`.

`fbb4724c` ("Add string_contains conformance coverage", #1405 — an unrelated
feature PR) flipped `base.sha` to `head.sha`, silently undoing the hardening from
#1211. The correct pattern remained in `validate-secrets` and `ci-gate`, so the
file contained both the right and the wrong version of the same idea.

Impact was bounded by the fork gate, so external contributors were never able to
reach it. It still mattered because this repository's working model is
agent-authored same-repo branches, which pass that gate by construction.

Secondary effect while head-pinned: a PR's own fast-check script changes took
effect in its own CI run, inverting the documented base-pinned behaviour and
masking the base-vs-data path bug class tracked in #1359.

The fix adds a workflow self-test assertion, proved by negative test rather than
by passing: reverting the pin to `head.sha` makes
`scripts/ci/test-pr-fast-ci-workflow.sh` exit 1.

### 3. Both defects share a root cause worth naming

An unrelated PR silently reverted a deliberate hardening decision, and no gate
noticed. This is the same failure mode recorded for monolith decomposition, where
file moves reverted newer merged behaviour (#1340). The mitigation that worked
here — encode the invariant as an executable assertion next to the thing it
protects, and prove the assertion fails when the invariant is violated — is
cheaper than review vigilance and should be the default for any security-relevant
CI property.

### 4. Documentation drift

- The #1434 descriptions in `roadmap-status.md` and `production-language-roadmap.md`
  are stale, as detailed above.
- `docs/production-language-readiness.json` lists `formatter_v1`, `lsp_v1`, and
  `documentation_v1` at `currentTier: syntax_only` with evidence lists that predate
  #1491, #1503, and #1502. The `partial` status is defensible because the target is
  `production_qualified`, but the evidence arrays no longer point at the files that
  implement these surfaces.

## Readiness, quantified

From `docs/production-language-readiness.json` at the audited commit, via
`make production-language-readiness-validate` (structure passes; readiness is
red by design):

```
52 rows total, 39 required for production
  required rows meeting their target tier:  3 / 39
  status (required):  implemented 3 | partial 24 | blocked 12

currentTier across all 52 rows:
  syntax_only 33 | static_spike 16 | runtime_complete 2 | production_qualified 1
```

The three rows at target are `native_scalar_runtime`, `local_package_workspace`,
and `package_trust_v1`.

Read plainly: the contract-and-schema layer is far ahead of the executable layer.
Twenty of the 39 required rows are still `syntax_only`, including `executable_mir`,
`runtime_lifecycle`, `ownership_resource_analysis`, and the runtime collection
rows that most other work depends on.

## Structural and forward-looking risks

**Monolith concentration.** `make stage1-compiler-source-monoliths` passes its
ratchet, but the shape it reports is worth stating directly: 80 files,
127,366 lines, of which the top 7 files hold 55.9%. `cranelift_backend.rs` alone
is 20,078 lines. A green ratchet means "not getting worse," not "getting better,"
and the compiler source migration (#1468-#1479) is gated on decomposition
actually progressing.

**Capability enforcement depends on static literals.** Per
[`stage1-net-socket-policy.md`](stage1-net-socket-policy.md), `net` bind and peer
arguments must be static `host:port` string literals checked against the manifest
allowlist. Enforcement is currently airtight partly because dynamic endpoints do
not exist. #1447 (`network_authority_v2`) will need a genuinely different
enforcement model — runtime authority checks rather than compile-time literal
matching — and that transition is the point at which the capability system is
most likely to develop a hole.

**Runtime crypto loads OpenSSL dynamically.** `cranelift_backend/host_crypto.rs`
resolves `libcrypto` via `dlopen` and transmutes symbols to typed function
pointers. The library search list is a hardcoded absolute-path allowlist
(`host_crypto.rs:748-765`), which is materially safer than name-based `dlopen`
since it is not influenced by `LD_LIBRARY_PATH`. This affects the user-program
crypto surface only; package trust does not depend on it.

## Coverage limits of this audit

Stated so the next reviewer knows where to start rather than repeating work.

- **Least covered:** `bounded_executor.rs` and `transactional_workspace.rs` (the
  agent containment boundary) and `package_resolver.rs` (#1524, merged during the
  audit window). Initial reading suggests the bounded executor is plan-and-verify
  with no process spawning, and that scope checks reject absolute paths and `..`
  components, but this was not traced to the depth the other subsystems received.
  Given that this subsystem is the sandbox for unattended agent authoring, it
  deserves a dedicated review.
- **Not attempted:** constructing Axiom source to probe borrow-checker soundness,
  differential testing of the Cranelift backend, and fuzzing the parser or manifest
  reader against malformed input.
- **Not assessed:** whether the enumerated CI test allowlist has gaps beyond the
  bin target. The same audit technique — compare `make`-invoked scope against
  CI-invoked scope — should be run for the other Make targets.
