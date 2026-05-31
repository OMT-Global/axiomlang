# Repair Executor v0

Repair Executor v0 is the proposed closed-loop successor to
[Repair Plan v0](repair-plan-v0.md). It consumes a repair-plan report, applies
only approved task-local edits, reruns the required evidence, and refreshes
delivery signals before a task can be marked resolved.

This document is a design contract only. It does not approve an auto-fixer,
does not add code, and does not change the Repair Plan v0 read-only boundary.
Executor implementation requires owner sign-off on this design or a later
accepted revision.

## Goals

- Close the loop from `repair-plan` task to bounded edit, evidence rerun, and
  fresh delivery-state recheck.
- Make every executor action auditable by recording the plan input, selected
  task, files touched, evidence commands run, and fetched PR/check state.
- Reject task execution when the requested edit escapes the task's
  `allowed_files`.
- Preserve repository guardrails: no force-push, no auto-merge, and no second
  required status check beyond `CI Gate`.
- Provide a dry-run mode that reports proposed file changes and evidence
  commands without writing files or invoking delivery mutations.

## Non-Goals

- Replacing `axiomc repair-plan`.
- Bypassing review, code owners, or branch protection.
- Creating a general autonomous coding agent.
- Adding hosted dashboard state.
- Automatically merging PRs after evidence passes.

## Inputs

The executor input is a Repair Plan v0 JSON report:

```bash
axiomc repair-plan <path> --json
```

Each executable task must include:

- `id`
- `reason`
- `target_node`
- `allowed_files`
- `required_evidence`
- `diagnostics`

The executor may also consume delivery-signal evidence when available: PR head
SHA, `CI Gate` status, review state, conversation state, and the timestamp of
the fetch. Missing delivery-signal evidence does not block dry-run, but it
prevents a terminal `resolved` status.

## Model Invocation Boundary

Repair Executor v0 has two acceptable operating modes. The selected mode must
be recorded in the execution report.

`deterministic` mode applies only built-in, rule-based repairs. It never invokes
a model. A task is skipped when no deterministic repair exists.

`assisted` mode may ask a model to propose a patch, but the executor remains the
enforcer. The model receives only the selected task, diagnostics, relevant
context from `allowed_files`, and the required evidence contract. The executor
must reject any proposed write outside `allowed_files`, any command outside the
required evidence set or an explicitly approved local validation set, and any
unauthorized delivery mutation.

Delivery mutation is a separate policy boundary from model invocation. The
default boundary is local-only: the executor may write files and run evidence,
but it may not commit or push. A publish-capable run is allowed only when owner
approval and repo policy explicitly authorize it for the selected repair-plan
task. In that mode, the executor may create a normal commit for the bounded edit
and push that commit to the PR branch with a non-force push after required
evidence passes. The model never decides whether publishing is allowed.

No implementation may silently switch between these modes.

Mode and delivery boundaries are therefore independent:

- local-only deterministic or assisted runs may reach `edited`,
  `evidence_running`, `evidence_failed`, or `delivery_recheck_failed`, but not
  `resolved` when a PR branch must advance;
- publish-capable deterministic or assisted runs may reach `resolved` only
  after the executor-created commit is pushed, fresh delivery state is fetched
  for that exact head SHA, and review/mergeability signals are satisfactory;
- dry-run never reaches `resolved` because it intentionally performs no write,
  commit, or push.

## Execution States

Task states are:

- `planned`: task exists in the repair plan and has not been selected.
- `dry_run`: executor produced a proposed patch and evidence plan without
  writing.
- `edited`: executor wrote files confined to `allowed_files`.
- `evidence_running`: required evidence is being rerun after the edit.
- `evidence_failed`: evidence completed but at least one required item failed.
- `delivery_recheck_failed`: evidence passed, but fresh CI/review state is
  unavailable or unsatisfied.
- `resolved`: edits are present, required evidence passed, and delivery signals
  were freshly rechecked after the latest edit.
- `rejected`: task could not execute because a safety invariant would be
  violated.

Only `resolved` is terminal success. A successful edit without fresh evidence
and delivery recheck is not resolved.

## File Boundary Enforcement

Before any write, the executor canonicalizes every target path relative to the
package root and checks it against the task's `allowed_files`.

The executor rejects the task when:

- a path is absolute and outside the package root;
- a path contains traversal that escapes the package root;
- a symlink resolves outside an allowed file or directory;
- a patch creates, deletes, renames, or chmods a file not covered by
  `allowed_files`;
- a model response, deterministic rule, or human-provided patch attempts to
  widen `allowed_files`.

The executor may read files outside `allowed_files` only when they are part of
repo policy or validation context, such as `AGENTS.md`, `CLAUDE.md`,
`project.bootstrap.yaml`, or the PR template. Those reads must be listed in the
execution report and cannot become writes.

## Dry-Run Contract

Dry-run is the default mode for the first implementation. It must:

- parse the repair plan;
- select one or more executable tasks;
- produce a reviewable unified diff or explain why no patch can be proposed;
- list the evidence commands that would run after applying the patch;
- list the delivery signals that would be fetched;
- write no source files;
- create no commits, branches, comments, reviews, or pull requests.

Dry-run output is evidence for review only. It cannot close an issue or mark a
task resolved.

## Evidence Rerun

After an edit, the executor runs the task's `required_evidence` in the order
declared by the repair plan unless the plan declares the evidence order
independent. Evidence output is captured with command, exit code, start time,
end time, and a path to any generated report.

The executor must fail the task when required evidence is missing, cannot be
run, exits non-zero, or produces a schema-invalid report.

Optional local checks may run before required evidence, but they cannot replace
the required evidence listed in the repair plan.

## Delivery Recheck

For PR-backed repairs, the executor must fetch delivery state after the latest
edit commit or after a dry-run patch is intentionally not committed.

A PR-backed repair that advances the branch requires publish-capable mode. After
evidence passes, the executor may push exactly the commit it created for the
selected task to the PR branch's configured upstream. It must reject publishing
when the upstream branch has advanced, when the push would require force, when
the task has no owner-approved publish policy, or when the branch is not the PR
head. It must not push to `main`, protected release branches, or any branch not
named by the active PR.

Fresh delivery state includes:

- latest PR head SHA;
- `CI Gate` or its required check source;
- review decision;
- unresolved conversation count;
- whether the PR is mergeable or conflicted;
- fetch timestamp.

The executor reports `delivery_recheck_failed` when signals are stale, missing,
red, blocked by unresolved conversations, or tied to an older head SHA.

The executor may request review or comment with evidence only when explicitly
run in a publish-capable mode approved by repo policy. It must not approve its
own PR and must not merge.

## Output Report

The execution report should be versioned and deterministic:

```json
{
  "schema": "axiom.repair-executor.v0",
  "command": "repair-executor",
  "mode": "deterministic",
  "task_id": "repair-1",
  "state": "resolved",
  "allowed_files": ["src/main.ax"],
  "touched_files": ["src/main.ax"],
  "evidence": [],
  "delivery_signals": [],
  "rejections": []
}
```

The report must be stable enough for agents and PR reviewers to diff. Future
implementation should add a JSON Schema before a CLI command ships.

## Safety And Governance

- `CI Gate` remains the single required PR status check.
- One non-author approval plus code-owner review remains the merge policy for
  `main`.
- Stage and production environments remain reviewer-gated.
- No executor mode may force-push, self-approve, dismiss reviews, alter branch
  protection, or auto-merge.
- Publish-capable mode may only perform a normal push of the executor-created
  repair commit to the active PR branch after evidence passes and owner policy
  authorizes publishing.
- No executor mode may copy auth state, sessions, caches, or machine-local
  secrets into a branch.
- All file writes must be attributable to a repair-plan task id.

## Acceptance For Implementation

Executor code can start only after an owner accepts this design or a follow-up
revision. The first implementation should ship in this order:

1. JSON Schema for `axiom.repair-executor.v0`.
2. Dry-run parser and task classifier.
3. Deterministic patch proposal for one narrow diagnostic class.
4. File-boundary rejection tests.
5. Evidence rerun capture.
6. Delivery recheck capture.
7. Publish-capable mode, if separately approved.

Any implementation PR must keep the model boundary explicit and must include a
fixture proving that writes outside `allowed_files` are rejected.
