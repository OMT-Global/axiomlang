# Transactional Workspace v0

Transactional Workspace v0 is the containment boundary between an approved
`axiom.agent_task.v0` contract and a future bounded executor. Every transaction
starts from an exact base commit in a transaction-owned branch and worktree;
the convenience API may use a detached transaction worktree. The
source checkout is evidence, not scratch space: pre-existing user changes are
never copied, overwritten, cleaned, reset, staged, or committed.

The two machine-readable contracts are:

- `axiom.execution_policy.v0`, the immutable authority compiled before any
  workspace mutation; and
- `axiom.execution_transaction.v0`, the durable transaction state and audit
  journal used for inspection, recovery, and rollback.

Their schemas and representative fixtures live under `stage1/schemas` and
`stage1/json-fixtures/execution-transaction`.

## Prepare Before Mutation

Preparation resolves the requested base revision to a full SHA, creates the
requested transaction-owned branch (or an explicitly detached worktree),
and records the initial checkpoint. A transaction must reject a reused or dirty
isolated worktree. An executor must canonicalize the worktree root and the
nearest existing ancestor of every requested path before granting access.

Paths are project-relative authority. Absolute paths, `..` traversal, NULs,
duplicate separators, and canonical paths outside the isolated root fail
closed. Symlinks are never followed for authority decisions. The same check
applies to reads, writes, chmod, rename source and destination, and deletion.
Protected paths cannot be edited in v0.

## Capabilities And External Effects

Portable v0 does not execute external commands. It records every request as a
denial even when a program name appears in the policy, because a program-name
allowlist cannot contain its filesystem, subprocess, environment, credential,
or network effects. A future executor may consume the schema's argument-vector,
capability, host, and broker-reference bounds only after it supplies a verified
OS sandbox backend. Until then commands, network, ambient credentials, and
external mutations fail closed and no secret value enters process context.

Protected-branch pushes, force-pushes, self-approval, and policy edits are hard
denials. Delivery mutations require both task authority and a separately
authorized pre-delivery checkpoint. Local rollback does not infer authority to
revert a delivered commit or other external state.

## Checkpoint, Abort, And Recovery

A durable exact-base checkpoint is recorded before mutation, and a pending
effect marker is persisted before every write, delete, rename, or chmod. The
journal assigns monotonically increasing sequence numbers and is persisted
atomically before the corresponding effect. On a local failure, rollback
restores the isolated workspace to its exact-base checkpoint,
removes only transaction-owned untracked files, and verifies that the source
checkout fingerprint is unchanged.

After interruption, an inspector validates the state SHA-256, transaction
identity, exact base/HEAD relationship, worktree identity, and journal sequence.
Resume is allowed only when no filesystem effect was pending at interruption.
Otherwise the only local action is rollback. Delivered changes require a
separately authorized revert path and are never silently reset.

## Deterministic Audit

The audit records the exact base SHA and policy digest, ordered checkpoints,
canonical reads and writes with content digests, command argument vectors,
capabilities and network hosts, exit codes, output digests, artifacts, rollback
result, and recovery state. Stable identifiers derive from approved inputs;
timestamps, hostnames, absolute machine paths, raw command output, environment
contents, credential material, and secret values are excluded. Identical
approved inputs and observations serialize to identical JSON bytes.

The security test matrix covers traversal, symlink escape, out-of-scope chmod,
rename and deletion, command and network escalation, ambient credentials,
protected delivery operations, dirty-source preservation, failure rollback,
and interrupted transaction recovery. These are rejection tests, not warnings.
