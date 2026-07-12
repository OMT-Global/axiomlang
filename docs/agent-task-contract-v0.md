# Agent Task Contract v0

Agent Task Contract v0 is the read-only authority boundary between an approved
issue or specification and an executor. It compiles an explicitly approved,
machine-readable task specification into a deterministic
`axiom.agent_task.v0` contract. It does not edit files, invoke a model, run a
declared command, create a branch, publish a commit, or mutate delivery state.

## Command

```bash
axiomc task-contract <spec.json> --project <path> --json
```

The specification and the resulting contract are validated against:

- `stage1/schemas/axiom-agent-task-spec-v0.schema.json`
- `stage1/schemas/axiom-agent-task-v0.schema.json`

Paths in successful output are normalized relative to the project root and use
`/` separators. Output ordering and generated identifiers are stable, so two
invocations over identical bytes and project state produce identical bytes.

## Authority Is Data, Not An Inference

The input must carry an immutable authority record identifying the repository,
governing issue, source revision and digest, and the explicit approval state,
approver, and approval method. The compiler rejects unapproved, ambiguous, or
internally inconsistent authority. It never treats free-form issue prose,
repository access, an agent assignment, or a prior approval as permission.

The compiler also never infers permission to widen:

- allowed files or semantic nodes;
- commands or capabilities;
- credential or environment access;
- time, token, retry, or cost budgets; or
- commit, push, pull-request, review, merge, release, or deployment mutations.

Unknown fields fail schema validation. Conflicting required and forbidden
commands or capabilities, missing scope, contradictory acceptance criteria,
unsupported irreversible actions, invalid dependency graphs, and delivery
permissions above the approved autonomy class fail closed.

## Contract Sections

Every successful contract preserves the approved specification's:

- objective and immutable authority;
- task kind (`feature` or `repair`);
- affected semantic node ids and allowed project-relative files;
- required and forbidden capabilities and commands;
- acceptance criteria and required evidence;
- dependency and precondition graph;
- autonomy and risk classification;
- time, token, retry, and cost budgets;
- rollback checkpoints and rollback commands;
- success, stop, and escalation conditions; and
- explicit delivery permissions.

The delivery object records hard denials for self-approval, force-push,
protected-branch direct push, and irreversible actions. Those fields are always
`false` in v0 and cannot be overridden by a higher autonomy class.

Lists are normalized and deduplicated only where order has no semantic meaning.
Acceptance and evidence order are preserved because they can define execution
order. Normalization may make authority narrower or reject it; it cannot make
authority broader.

## Feature And Repair Fixtures

The checked-in feature specification authorizes a bounded documentation and
source change, local validation, and review-gated pull-request delivery. Its
expected contract proves deterministic normalization and project-relative
paths.

The checked-in repair specification carries a Repair Plan v0 task as a
constrained subtype. Its diagnostic trigger, target node, allowed files, and
required evidence are preserved losslessly. Repair compatibility does not grant
execution: the task contract remains read-only and supplies authority to the
separately governed executor.

## Failure Contract

Invalid specifications exit non-zero. JSON mode returns a deterministic error
envelope and no executable contract. Validation tests cover missing or
ambiguous authority, missing scope, conflicts, irreversible actions, scope
widening, invalid or unbounded budgets, unresolved dependencies, and delivery
permissions that exceed the autonomy class.

These are contract errors, not warnings. An executor must not attempt to repair
or fill in a rejected specification.
