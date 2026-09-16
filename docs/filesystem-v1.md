# Filesystem v1 evidence contract

Issue `#1443` defines the production filesystem boundary. The checked
`axiom.filesystem.v1` snapshot is a target-neutral, fail-closed contract for
typed paths, metadata, traversal, binary file resources, atomic replacement,
and secure temporary files and directories. It is evidence for the intended
surface, not a claim that the full runtime exists.

## Honest current boundary

The current `std/fs.ax` module exposes root-scoped UTF-8 helpers for reading,
writing, appending, creating, replacing, and removing files and directories,
plus `file_exists` and `file_size`. Generated-Rust and direct-native runtimes
bound read and write sizes, canonicalize the effective filesystem root, and
deny known parent-traversal and static symlink escapes. Most pathname
operations still canonicalize or inspect a path and then open that pathname;
they are not descriptor-anchored and do not prove safety against a parent
replacement between validation and use. The descriptor-relative direct-native
atomic-replace path is narrower evidence and must not be generalized to the
other operations or backend.

That floor remains `static_spike` and `partial`. It does not expose typed paths,
binary handles, deterministic directory traversal, seek, flush/fsync,
generation-checked lifecycle ownership, or secure temporary resources. The
generated-Rust `replace_file` helper uses a same-directory rename, but its
temporary name is predictable and it does not prove exclusive creation,
restrictive permissions, file sync, or directory sync. Direct-native replace
does use a descriptor walk, exclusive unpredictable temporary creation,
restrictive permissions, file sync, rename, and directory sync on supported
hosts. The contract still keeps overall `atomic_replace` and
`secure_temporary_resources` false until every supported backend qualifies
the full rule set.

The compile-time evaluator still dispatches filesystem reads, writes,
directory operations, and replacement while evaluating known programs. Current
`runtime_effects_only` evidence is therefore false. Issue `#1434` must remove
that effectful fallback and build-purity tests must prove that check and build
cannot execute package filesystem effects before this claim can be promoted.

## Contract boundary

Paths retain their runtime origin and effective scoped root. Join, normalize,
parent/name/extension, relative/absolute policy, metadata, file type,
canonicalization, and directory enumeration have target-neutral semantics.
Directory ordering converts backslashes to `/` only for typed Windows-origin
paths; POSIX-origin backslashes remain literal filename characters. It removes
`.` components, resolves `..` only while it remains inside the scoped root,
and then compares normalized path, origin, and raw path by Unicode scalar
sequence. The origin/raw suffix makes the key injective when a Windows path
and a valid POSIX spelling normalize to the same visible path. It performs no
Unicode normalization and no case folding. Composed and decomposed spellings
therefore remain distinct, case remains significant, and metadata or host
enumeration order cannot affect the result. The checked directory fixture
contains concrete Windows-style separators, dot components, composed and
decomposed Unicode, and case-sensitive vectors.

Authority is denied by default and partitioned into `read`, `write`,
`metadata`, `traversal`, and `temporary`. The published operation matrix binds
every pathname, current helper, open, and I/O operation to exactly one of
those grants; close, seek, flush, and fsync retain and revalidate the grant and
access mode captured by the opened handle. A missing or wrong grant denies
before allocation or host I/O. Delegation may narrow roots and operations but
may not broaden either. The current `fs` and `fs:write` grants are recorded as
an incomplete migration floor rather than treated as all five production
authorities.

Binary file handles report requested and completed byte counts, permit partial
I/O, and reject both unbounded requests and finite requests above 1,048,576
bytes before allocation or host I/O. They expose start/current/end seek origins, and
separate buffered flush from durable fsync. Handle generation, lifetime,
idempotent close, and drop cleanup compose with `axiom.runtime_lifecycle.v1`.

Atomic replacement must create an exclusive unpredictable temporary file in
the destination directory, apply restrictive permissions, sync file contents,
rename without exposing partial content, sync the directory, and clean up
abandoned resources. Rename is the commit point: failures before rename
preserve the old destination; a directory-sync failure after rename reports
`committed_durability_uncertain` because the new destination is visible but
its crash durability is not proven. Secure temporary files and directories use
the same scoped authority, no-follow behavior, and lifecycle cleanup. The
production target keeps parent and root identity descriptor-anchored through
the operation so validation/use races fail closed.

Normative Filesystem v1 effects are runtime-only. Parse, check, and build may
inspect declared source inputs but may not execute package filesystem
operations. This is a target requirement, not current implementation evidence;
the current evaluator exception above keeps readiness partial.

## Fixtures and validation

Thirteen fixtures cover the current scoped-text floor, typed paths,
deterministic traversal, separately authorized partial binary reads and writes,
durable atomic replacement, secure temporary resources, traversal escape,
symlink swapping, predictable temporary names, authority partition denials,
unbounded I/O, and finite oversize I/O. Each fixture
has an exact kind, evidence tier, operation, authority set, outcome, and
assertion set. Target fixtures define promotion requirements; only
`scoped-text-floor` and `traversal-escape` are credited as current evidence.
Negative fixtures must produce `denied` without allocation or a host operation.
Promotion to `runtime_complete` additionally requires every fixture reference
to carry runtime-backed evidence; target-only fixtures cannot qualify a runtime.

Run the focused contract and regressions with:

```bash
python3 scripts/ci/check-filesystem-v1.py
python3 scripts/ci/test-check-filesystem-v1.py
bash scripts/ci/run-filesystem-v1-behavioral-tests.sh
```

The fast PR lane runs both commands. Production readiness remains partial until
issues `#1425`, `#1426`, `#1434`, and `#1438` supply the dependent runtime,
capability, error, and lifecycle foundations and runtime-backed fixtures pass
on supported targets.
