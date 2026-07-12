# Bounded Executor v0

Bounded Executor v0 is the local, transactional execution engine governed by
[#1422](https://github.com/OMT-Global/axiomlang/issues/1422). It consumes an
approved `axiom.agent_task.v0` contract, an exact transaction, and a typed
repair request. It does not infer authority from issue prose or diagnostics.

The executor is deliberately not a delivery controller. It may produce a
reviewable local candidate, evidence, and an audit report. It cannot approve,
merge, force-push, edit policy, or silently acquire a capability. External
delivery remains separately governed by #1423.

## Versioned contracts

- `axiom.executor_request.v0` binds the task, transaction, operation, proposal,
  verification input, and inherited budget ceilings.
- `axiom.executor_report.v0` records the deterministic state transition,
  effective authority, candidate digest, cumulative budget and retry use,
  evidence verdict, escalation, and rollback result.
- `axiom.executor_resume.v0` identifies an interrupted report and transaction
  by digest and supplies no new authority. Resume ceilings must be equal to or
  narrower than the original remaining budget.

All three schemas reject undeclared fields and have typed runtime parsers. An
executor request is converted only after its task, transaction id, policy
digest (the v0 `transaction_digest` field), base, budgets, retry policy, and
optional proposal match the supplied runtime objects. A
resume request is consumed by recovery and must match the sealed report,
transaction journal, remaining budgets, event sequence, and candidate. Reports
use sorted collections and stable identifiers so equal inputs produce
byte-identical dry-run output.

## Authority intersection

Effective authority is the intersection of the task contract, embedded repair
task, workspace policy, and typed proposal. The executor rejects, rather than
clips and continues, any proposal that widens allowed files, commands,
capabilities, budgets, or delivery permissions. Patch targets are canonicalized
again immediately before mutation; traversal, absolute paths, symlink escape,
rename escape, chmod escape, and policy-file mutation fail closed.

An assisted proposal is data. It must carry a known operation, target path,
preimage digest, and replacement payload. Diagnostic text is never executed or
interpreted as a command. The executor alone validates and applies proposals.

## State machine

The ordered states are `planned`, `dry_run`, `edited`, `evidence_failed`,
`verification_passed`, `resolved`, `rolled_back`, `rejected`, `escalated`, and
`interrupted`. Only `resolved` is terminal success. Dry-run never mutates and
never resolves. An interruption is inspectable and resumable only
when the task, transaction, policy, journal, candidate, and remaining budgets
still match their recorded digests.

Every transition is appended to a hash-chained audit. In addition to the event
chain, the report carries a domain-separated HMAC over every serialized
authority and state field. The report contains only the seal key id and MAC;
the recovery key is runtime authority and is never serialized. Recovery rejects
a missing, stale, wrong-key, or caller-recomputed unkeyed seal. A resume
continues its sequence, attempt counts, and cumulative resource use; it cannot
reset them.
When an effect may have occurred but cannot be proven, resume is rejected and
rollback is required.

## Retry and stop policy

Failures are classified as `code`, `evidence`, `environment`, `conflict`,
`policy`, or `unknown`. Code, evidence, environment, and conflict failures may
retry only when that exact cause appears in the sealed `approved_causes` retry
policy and the global ceiling remains. The requested set can only narrow the
signed task contract capabilities `executor.retry.code`,
`executor.retry.evidence`, `executor.retry.environment`, and
`executor.retry.conflict`; it cannot create retry authority. An absent retry
policy permits no retry. Policy and unknown cannot appear in the approved set
and are always non-retryable. Identical failure fingerprints stop on their
second occurrence.
Contradictory evidence, an invalid proposal, authority mismatch,
scope escape, preimage mismatch, exhausted time/token/cost/retry budget, or an
unsupported irreversible action stops and escalates.

## Evidence and success

Evidence must be produced after the latest candidate and bound to its digest.
The executor first asks the transactional workspace to read the exact proposal
path from the delivered Git commit. The delivered head must descend from the
transaction base, contain the exact candidate bytes, include the proposal path,
and change no path outside the workspace write scope. Only after that trusted
proof does the executor create and seal a candidate binding across candidate
digest, delivered head, transaction id, policy digest, and verification-plan
digest. Callers cannot construct this binding. Delivery must present its exact
binding digest; changing any member invalidates it.
Every required evidence id appears exactly once. Missing, stale, duplicate,
schema-invalid, contradictory, or failing evidence prevents success. A
previously green check cannot prove a changed candidate. Evidence regression
rolls the transaction back when rollback is safe; otherwise it leaves a clear
non-terminal report requiring operator action. It never reports resolved.

Terminal success additionally requires an unchanged authority digest, an
inspectable successful transaction, no unresolved escalation, and authenticated
delivery-provider evidence when the task contract requires delivery. Provider
evidence is MAC-authenticated with a runtime-only trust key and records provider
id, exact candidate binding, fetch epoch, required-check result, independent
review result, and conversation resolution. The trusted provider enforces
maximum age against the runtime system clock; callers cannot provide the time
used for freshness validation or replace this evidence with aggregate `fresh`
or `satisfied` booleans. Self-review and auto-merge are outside this engine.

## Fixture corpus

The contract corpus covers deterministic success, rejected scope escape,
evidence regression with rollback, bounded identical retry, interruption and
resume without budget reset, and explicit rollback. Integration tests validate
every runtime report against the strict schemas and assert that failure paths
cannot serialize as success.
