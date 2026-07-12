# Repair Plan v0

Repair Plan v0 is a read-only planning API for agents. It does not edit source
files, invoke a model, create branches, or open pull requests. It turns current
package diagnostics and missing validation evidence into structured tasks.

Closed-loop repair is intentionally out of scope for this command. The proposed
successor boundary is documented separately in
[Repair Executor v0](repair-executor-v0.md), which requires owner approval
before any executor implementation can land.

## Command

```bash
axiomc repair-plan <path> --json
```

The command exits zero when it can produce a plan, even if the package has
failing checks. A broken manifest or unreadable project can still return the
normal stage1 JSON error envelope when no useful plan can be produced.

## Task Shape

Each task includes:

- `id`: stable task id within the report.
- `reason`: diagnostic code, diagnostic kind, or `missing_evidence`.
- `target_node`: package-scoped semantic node or diagnostic node.
- `allowed_files`: files the agent may inspect or edit for this task.
- `required_evidence`: evidence expected after repair.
- `diagnostics`: normalized stage1 diagnostics that triggered the task.

## Scope

V0 covers:

- package diagnostics from `axiomc check`,
- source spans and files when the diagnostic provides them,
- missing unit-test evidence when a package checks but has no tests.

Future versions can attach capability, effect, axiom, evidence, and artifact
node ids once those semantic graph surfaces are fully merged.

## Agent Task Contract Compatibility

[Agent Task Contract v0](agent-task-contract-v0.md) can represent a Repair Plan
v0 task as a constrained `repair` task. The conversion preserves the repair
task id, reason, target node, diagnostics, allowed files, and required evidence.
It may add narrower authority, budgets, rollback, terminal conditions, and
delivery denials supplied by an explicitly approved specification, but it may
not widen the repair plan's file or evidence boundary.

Repair Plan v0 remains a diagnostic-driven planning API. It is not itself proof
of approval and does not grant execution or delivery permission.
