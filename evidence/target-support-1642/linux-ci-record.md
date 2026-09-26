# Linux x86-64 hosted-CI record (ordinary ubuntu-24.04 runs only)

Repo: OMT-Global/axiomlang. Branch: pheidon/axiom-1642-target-evidence-20260925.
All runs below are standard GitHub-hosted `ubuntu-24.04`; no self-hosted runners
exist or were registered; no macOS jobs exist in any workflow.

## Run 36206489859 — push-event workflow parse failure (zero jobs)

- Trigger: push of H1 (`91ec57036c32e9873149281c582d0c413beb9dbc`) to the branch.
- Conclusion: failure with **zero jobs** — GitHub rejected the workflow file at
  parse time: `Unrecognized named-value: 'runner'` (job-level `env:` referenced
  `runner.temp`, which is not an allowed context there; same rejection surfaced
  synchronously as HTTP 422 on the first dispatch attempt).
- Consumed no job capacity. Fixed by H1b
  (`f5ae3590f9d59a61d11f8e54a1cee2f82bdb44c0`): removed the redundant job env;
  the evidence script itself defaults `CARGO_TARGET_DIR` to
  `$RUNNER_TEMP/axiom-target-evidence-linux-x86-64` at runtime (identical
  on-runner behavior).

## Run 1 — dispatch 36206756482 at H1b (f5ae3590…)

- URL: https://github.com/OMT-Global/axiomlang/actions/runs/36206756482
- Trigger: `workflow_dispatch` with `evidence_pr_sha=f5ae3590f9d59a61d11f8e54a1cee2f82bdb44c0`
  (approval-gated exact-PR-SHA path; dispatch requires write access).
- Jobs: Detect Extended Validation Scope **success**; Validate Secrets **success**;
  Fast Checks **success**; Target Support Evidence (linux-x86-64) **failure**;
  Extended Checks cancelled (by author, to conserve the ≤3 CI budget after the
  failure was root-caused); gate not reached.
- Evidence-job failure cause: `target-support-evidence: checkout contains
  untracked or ignored inputs` — the CLI's own interpreter wrote
  `scripts/ci/__pycache__/json_schema_v1.cpython-312.pyc` at import time, and the
  (correct) fail-closed purity gate
  (`git status --porcelain=v1 --untracked-files=all --ignored=matching`) rejected
  the dirtied checkout. Root cause proven by detached-clone repro at H1b
  (`!! scripts/ci/__pycache__/` appears after `validate`), and by the controlled
  contrast with the macOS lane where `PYTHONDONTWRITEBYTECODE=1` made the
  identical check pass. This was a latent defect in the original design (it would
  have failed every continuous main-ref run).
- Fix: H1c (`4f2e329f18f251f1dd806a0309370bc8fcad04e1`) sets
  `sys.dont_write_bytecode = True` before the local import and adds a subprocess
  regression test (`test_validate_cli_never_writes_bytecode_cache_into_the_checkout`).
  Clean repro at H1c: `validate` rc 0 and empty purity status.

## Run 2 — dispatch 36207965390 at H1c (4f2e329f…)

- URL: https://github.com/OMT-Global/axiomlang/actions/runs/36207965390
- Trigger: `workflow_dispatch` with `evidence_pr_sha=4f2e329f18f251f1dd806a0309370bc8fcad04e1`.
- Terminal conclusions: Detect Extended Validation Scope **success**; Validate
  Secrets **success**; **Fast Checks success**; **Extended Checks success**
  (complete qualification suite on hosted ubuntu-24.04 at the exact head);
  Target Support Evidence (linux-x86-64) **failure**; Extended Validation Gate
  **failure** (consequence of the evidence job, by design).
- Evidence-job failure cause (sole): the job inherited the August pin
  `RUST_VERSION: '1.90.0'` from the original self-hosted-runner design, while
  current main's locked dependency tree requires rustc ≥ 1.93.0. Every cargo
  invocation exited 101 immediately:
  `error: rustc 1.90.0 is not supported by the following packages:
  cranelift-assembler-x64@0.132.0 requires rustc 1.93.0 … wasmtime-internal-core@45.0.0 requires rustc 1.93.0`.
  All preceding stages passed on the runner: dispatch-input validation, exact-SHA
  checkout (`ref: inputs.evidence_pr_sha || github.sha`), toolchain install,
  C-compiler provisioning, `cargo fetch --locked`, and the checkout purity gate
  (H1c fix effective). The job's fail-closed `failed` evidence JSON was produced
  and uploaded as artifact `target-support-linux-x86-64-4f2e329f18f251f1dd806a0309370bc8fcad04e1`
  (honest failure record: all build checks `failed`, `evidence_status=failed`).
- Fix: H1d (`ff5e549ea3fe238204c1e12a34a39606eb80e412`) drops the stale pin and
  uses the same stable dtolnay toolchain action as fast-checks/extended-checks
  (which both build this tree green on ubuntu-24.04 in this very run); the
  workflow test now fails closed if any `RUST_VERSION` pin reappears. H1c→H1d is
  workflow/test-only: no product-code delta.

## Run 3 — PR CI at H2 (this head)

- Triggered by opening this PR; the required `CI Gate` fast suite on hosted
  ubuntu-24.04 at the exact final head. Run id and terminal result are recorded
  in the PR conversation and the author checkpoint
  (`reports/axiom-1642-reauthor-20260925/`) because this file is committed before
  that run exists. Under the revised contract this is the pre-merge Linux
  exact-head assurance; the artifact-JSON contract is post-merge-continuous plus
  the maintainer-dispatchable approval-gated path documented in
  `docs/target-support-v1.md`.

## Budget

CI runs consumed by this work: 36206756482 (run 1), 36207965390 (run 2), and the
PR CI at H2 (run 3) — within the ≤3 budget. The zero-job parse-failure entry
36206489859 consumed no job capacity. No further runs may be triggered by the
authoring lane; maintainers may dispatch the approval-gated path at will.
