# Repair Plan v0

Repair Plan v0 is a read-only planning API for agents. It does not edit source
files, invoke a model, create branches, or open pull requests. It turns current
package diagnostics and missing validation evidence into structured tasks.

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
