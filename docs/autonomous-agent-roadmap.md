# Autonomous Agent Execution Roadmap

This roadmap defines the path from Axiom's current agent-facing inspection and
planning surfaces to safe unattended code authoring. The governing GitHub
umbrella is [#1417](https://github.com/OMT-Global/axiomlang/issues/1417).

"Unattended" means an agent can complete approved, bounded work without
continuous human steering. It does not mean unrestricted authority. Repository
policy, capability boundaries, independent review, evidence, and stop
conditions remain authoritative.

## Shipped Foundations

Axiom already provides the read-only and verification foundations needed by an
executor:

- semantic declarations and agent-facing graph/effect/artifact inspection;
- evidence reports, invariant verification, and semantic diff;
- intent-to-artifact provenance and decision records;
- structured repair plans with allowed files and required evidence;
- issue-to-PR, CI, review, and conversation delivery signals;
- backend target contracts and deterministic artifact generators.

These surfaces describe work and its proof. They do not yet authorize or
execute a complete coding transaction.

## Target Loop

The target unattended loop is:

```text
approved issue/spec
  -> typed task contract
  -> isolated transactional workspace
  -> bounded edit
  -> semantic impact + evidence plan
  -> verification on the exact head
  -> PR + independent agent review
  -> policy-gated merge or escalation
  -> post-delivery verification and rollback when required
```

Every transition emits versioned, deterministic evidence. A missing or
ambiguous transition stops the loop rather than being inferred.

## Milestones

### A0: Canonical semantic state — #1418

Emit complete Intent IR for real packages so planning, verification, repair,
artifact generation, and semantic diff consume the same graph. Partial views
must identify their omissions explicitly.

### A1: Typed authority — #1419

Compile an approved issue or specification into a task contract containing
scope, allowed files, required and forbidden capabilities, evidence, budgets,
dependencies, rollback, delivery permissions, and stop/escalation conditions.
The executable contract and its fail-closed boundary are defined in
[Agent Task Contract v0](agent-task-contract-v0.md). Authority includes the
approved source revision and digest; it is preserved rather than reconstructed
from issue prose.

### A2: Transactional containment — #1420

Execute from an exact base SHA in an isolated branch/worktree. Enforce path,
command, network, credential, and external-mutation policy; preserve user-owned
dirty work; checkpoint changes; and support crash-safe rollback.
The normative policy, audit, and recovery rules are defined in
[Transactional Workspace v0](transactional-workspace-v0.md).

### A3: Impact-aware proof — #1421

Map before/after Intent IR and semantic drift to required positive, negative,
schema, artifact, security, and performance evidence. Unknown impact broadens
the suite or blocks execution; it never silently reduces validation.
The versioned plan, evidence-result, and exact-head verdict contracts are
defined in [Verification Planner v0](verification-planner-v0.md).

### A4: Bounded execution — #1422

Implement the Repair Executor v0 design as a general task/repair engine. Start
with dry-run and deterministic repairs, then allow policy-gated assisted patch
proposals. Enforce retry, time, token, and cost budgets with explicit terminal
states.

### A5: Independent delivery — #1423

Manage the PR lifecycle with exact-head CI evidence, conflict and review-comment
repair, and a reviewer identity that did not author or push the change. Merge is
allowed only when repository policy explicitly permits it; self-approval and
force-push remain prohibited.

### A6: Evaluation and promotion — #1424

Gate each autonomy level with adversarial tasks measuring semantic correctness,
false greens, scope escape, evidence selection, review catch rate, rollback,
appropriate escalation, retries, time, and cost. The suite must include tasks
whose correct result is to stop.

## Composition Order

1. A0 and A1 establish semantic truth and authority.
2. A2 establishes the containment boundary.
3. A3 establishes the proof contract.
4. A4 may then perform local writes.
5. A5 may add external delivery mutations only after separate policy approval.
6. A6 gates promotion at every stage and prevents capability completion from
   being mistaken for trustworthy autonomy.

## Autonomy Levels

| Level | Allowed behavior | Promotion evidence |
| --- | --- | --- |
| 0 — Observe | Inspect, report, and propose without writes. | Schema-valid inspection and plans. |
| 1 — Safe local | Deterministic, reversible local edits in an isolated transaction. | Containment, rollback, and focused evidence. |
| 2 — Review-gated | Assisted implementation, commit, and PR updates on an authorized branch. | Exact-head evidence plus independent review. |
| 3 — Policy delivery | Merge-capable orchestration for repositories that explicitly permit independent agent approval. | Delivery-controller and evaluation gates; no policy exceptions. |
| 4 — Forbidden unattended | Credentials, legal/product decisions, irreversible data changes, protection changes, self-approval, or unbounded execution. | Human authorization cannot be inferred or delegated by the executor. |

## Safety Invariants

- GitHub issues remain the source of execution authority.
- Authoring and approving identities are always distinct.
- Allowed files, commands, capabilities, budgets, and delivery permissions
  cannot widen themselves.
- Secret values never enter model context, reports, commits, or artifacts.
- Protected branches cannot be pushed directly or force-pushed.
- Terminal success requires fresh evidence tied to the exact delivered SHA.
- Conflicting evidence, policy denial, unknown impact, or an exhausted budget
  stops and escalates.
- Rollback is a planned transaction with evidence, not an unreviewed destructive
  command.

## Readiness Gate

The final roadmap should expose a machine-readable
`axiom.agent_autonomy.readiness.v0` report. It remains `ready: false` until:

- every milestone has executable evidence rather than documentation alone;
- an end-to-end fixture completes the full target loop;
- adversarial containment and false-green thresholds pass;
- independent review is proven for the exact author head;
- rollback and crash recovery succeed; and
- repository policy explicitly permits the requested delivery autonomy.

The gate is evidence, not permission. Product, legal, security, credential,
release, and irreversible decisions remain human-owned unless a narrower policy
explicitly states otherwise.
