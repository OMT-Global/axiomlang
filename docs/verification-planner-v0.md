# Verification Planner v0

Verification Planner v0 deterministically maps the semantic impact between two
complete Intent IR snapshots to the evidence required for one exact delivered
head. It is a read-only planning and evaluation surface: it does not run tests,
edit a checkout, grant capabilities, waive evidence, or deliver a change.

## Contract

The planner consumes before and after `axiom.intent_ir.v0` documents plus the
established `axiom.semantic_diff.v0` report and the source and delivered commit
SHAs. The CLI recomputes graph changes and rejects a diff that omits or invents
node or edge drift. It emits `axiom.verification_plan.v0`, whose
bindings retain both graph IDs, both snapshot digests, and both commit SHAs.
Every semantic change records its stable ID, change kind, semantic node kind,
semantic ID, package-relative source path when known, and classified impact.

Evidence requirements use these closed categories:

- `positive` proves intended behavior;
- `denial` proves prohibited capability or effect paths remain denied;
- `regression` protects public and dependency-facing behavior;
- `schema` validates changed machine-readable contracts;
- `artifact` detects generated-output drift;
- `security` exercises security-sensitive boundaries;
- `performance` compares a performance-sensitive change with its baseline.

A changed capability, effect, axiom, dependency, public contract, schema, or
artifact target always produces required evidence. A localized implementation
change produces positive evidence. An impact the planner cannot classify is
reported in `coverage.unknown_impacts` and receives the conservative evidence
suite; uncertainty can broaden or block a plan but cannot reduce it. Plans with
semantic changes but no evidence requirements are invalid; identical snapshots
may produce an explicitly empty plan.

The machine-readable contracts are:

- `stage1/schemas/axiom-verification-plan-v0.schema.json`
- `stage1/schemas/axiom-verification-results-v0.schema.json`
- `stage1/schemas/axiom-verification-verdict-v0.schema.json`

All three reject undeclared fields. IDs and requirement order are derived from
canonical semantic input, so identical inputs and SHA bindings serialize to
identical bytes.

```bash
axiomc verification-plan before.json after.json \
  --diff semantic-diff.json --project . \
  --source-head <source-sha> --delivered-head <current-head> --json
```

The CLI resolves `--project` HEAD independently and rejects a stale or invented
`--delivered-head`. Library callers must supply an independently observed head
from their delivery controller; repeating an untrusted result field is not a
freshness proof.

## Exact-head evaluation

An evidence producer returns an `axiom.verification_results.v0` envelope. The
envelope and each result repeat the plan digest, source head SHA, and delivered
head SHA. Evaluation rejects stale or cross-plan evidence, duplicate evidence
IDs, unknown IDs, and a caller-provided delivered SHA that differs from the
plan. Evidence digests bind the observed result without embedding logs or
secrets in the contract.

`axiom.verification_verdict.v0` is terminally successful only when all of the
following hold:

1. every planned requirement has exactly one result;
2. every result is schema-valid and bound to the plan and exact heads;
3. no unplanned or duplicate evidence result is present;
4. every required result is `passed`.

The verdict exposes deterministic `missing`, `duplicate`, `invalid`, and
`failed` requirement-ID lists. A non-empty list makes the verdict rejected.
Neither evaluator nor schema treats an absent result as success.

## Fixture and quality surface

`stage1/json-fixtures/verification-planner` covers localized implementation,
public API, capability escalation, schema, generated-artifact, performance, and
unknown-impact changes. The unknown fixture is adversarial: it proves that an
unmapped node kind broadens evidence instead of silently producing a smaller
suite. Mapping and adversarial tests form the initial quality measurement; the
same fixture dimensions feed the autonomy evaluation suite governed by #1417
and #1424.

This planner composes the complete Intent IR from #1418, the bounded task
contract from #1419, and the transactional boundary from #1420. The bounded
executor in #1422 may run only the evidence a valid plan requires and must use
the exact-head verdict as its terminal verification gate.
