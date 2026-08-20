# Iteration and Loop Control v1

Iteration and Loop Control v1 defines backend-neutral `for`, iterator, `break`,
and `continue` semantics for arrays, immutable and mutable slices,
runtime-sized sequences, deterministic maps, Unicode text scalars, and
statically selected user iterators. Storage layout, register choices, host
container types, and generated-source details are not language semantics.

The machine-readable target and current evidence floor are stored in
`stage1/compiler-contracts/snapshots/iteration-control-v1.json` and validated
against
`stage1/compiler-contracts/schemas/axiom.iteration_control.v1.schema.json`.
Run the contract and mutation gates with:

```bash
make stage1-iteration-control-v1
make stage1-iteration-control-v1-test
```

These gates do not claim that `for` is implemented. They preserve the useful
runtime `while`, `break`, and `continue` evidence while requiring `for` to keep
failing closed until the complete protocol, ownership, inspection, and
direct-native proof matrix exists.

This is a non-closing prerequisite slice. Its issue linkage is `Refs #1442`;
the issue remains open until the direct-native qualification matrix is actually
executed and accepted.

## Protocol operations

An iterable source is evaluated exactly once before iterator creation. The
target-neutral protocol has three semantic operations:

- `into_iter(source, mode)` creates one iterator with a collection kind,
  ownership mode, and deterministic order;
- `next(iterator)` advances once and returns `Some(item)` or permanent `None`;
  and
- `drop(iterator)` releases the iterator and every remaining ownership or
  resource obligation exactly once.

Iteration that can fail models failure explicitly in its item type, such as a
result-bearing item. The loop construct does not add hidden exceptions. An
unsupported source or mode is diagnosed before the body executes and before
the source is mutated.

The supported modes are shared borrow, exclusive borrow, and move. Shared
iteration yields a shared element borrow. Exclusive iteration yields one
exclusive element borrow scoped to the body execution. Move iteration consumes
the source and yields owned elements. Non-Copy elements are never silently
copied.

## Order and loop control

Arrays, slices, and runtime sequences use ascending index order. Text uses
Unicode scalar order, not byte or host-code-unit order. Map iteration uses the
deterministic order declared by the associative-collection contract; bucket or
address order is forbidden. A static user iterator makes order part of its
implementation contract.

`break` exits the nearest enclosing loop. `continue` drops the current
iteration binding and advances the nearest enclosing iterator. Normal end,
`break`, `continue`, function return, propagated error, and cancellation run
the current binding and iterator cleanup required by lexical scope exactly
once. Nested loops hold independent iterator state.

A cleanup failure replaces a successful normal end, `break`, `continue`, or
function return with an error. An already-propagating error or cancellation
remains primary and cleanup failures are attached as ordered secondary
failures. This precedence is deterministic; no cleanup failure is discarded
and no cleanup action runs twice.

Iteration v1 is pull-based. One consumer request performs one `next` operation
and yields at most one item. Prefetch is zero, at most one item is outstanding
per iterator, and the producer cannot advance until the current item is
released or transferred. These bounds apply to nested and fallible iterators.

## Mutation during iteration

Shared iteration rejects element and structural mutation while the iterator is
live. Exclusive iteration may update the current element through its exclusive
borrow, and the write is visible in the source collection without changing its
length, capacity, deterministic order, or generation. Any operation that can
relocate or resize a borrowed source is rejected before mutation. Map insert,
remove, clear, or rehash is rejected while any iterator is live. Every rejected
attempt is atomic: element bytes, length, capacity, order, and generation are
unchanged.

Move iteration owns the remaining source state. Moving the iterator invalidates
the old binding; using it fails with `ownership.iterator_moved`. Moving an
element transfers its cleanup obligation to the loop body. A borrow may not
escape its iterator or source owner and fails with `ownership.borrow_escape`.
Moving or dropping a borrowed source is rejected before invalidation, and a
stale generation must fail before `next` or cleanup side effects.

## Static user iterators

User-defined iteration is statically selected. The implementation declares its
source type, item type, ownership mode, deterministic order, and protocol
operations. Dynamic iterator dispatch is outside v1 and fails with
`iteration.dynamic_dispatch_unsupported`; no implicit vtable or host iterator
may fill the gap.

## Semantic IR and inspection

Syntax, HIR, MIR, and Intent IR preserve explicit `For`, `IteratorBegin`,
`IteratorNext`, `LoopBreak`, `LoopContinue`, and `LoopExit` evidence. A backend
may lower those nodes to control-flow blocks, but a public inspection surface
must not report `for` as a source-level `while` desugaring.

Inspection exposes collection kind, iterator identity, protocol operation,
deterministic order, element ownership, loop-control edge, runtime origin,
semantic node identity, source provenance, and target support. It never exposes
host addresses, storage buckets, or backend container names.

## Structured target gaps

The files under
`stage1/compiler-contracts/fixtures/iteration-control-v1/` freeze the required
control-exit, mutation/ownership, deterministic-order/backpressure, and runtime
receipt cases. They are target-gap contracts with
`executable_proof: false`; declarations, fixture records, source markers, and
test names are not runtime proof.

## Build-once/run-many receipt

Runtime qualification requires a fail-closed direct-native receipt. It records
the artifact identity, path, supported target, compiler identity, source
digest, and SHA-256 of the built bytes; exactly one successful build event; and
at least two strictly ordered post-build runs of that same artifact. Every run
records its runtime origin, concrete inputs and input digest, stdout and stderr
digests, exit status, timestamps, artifact hash, and
`rebuild_observed: false`.

The same artifact must observe environment, file, HTTP, prior-function-result,
and stdin values after the build. A compiler-known value, static collection
projection, static fixture substitution, declaration, marker, or rebuild per
run fails with `language.iteration_control_not_qualified`. The receipt contract
is currently `required_unimplemented` and `proof_executed: false`.

## Trusted CI bootstrap boundary

PR Fast CI executes the checker and its Python self-test from the pull request
base checkout. Pull-request-head files are supplied only as data through the
explicit `--root "$repo_root"` argument and are read with descriptor-relative,
no-follow, nonblocking, bounded UTF-8 reads. The first PR that introduces this
checker cannot execute its newly authored checker from the head under the
trusted label; its base does not contain the checker yet. That bootstrap
limitation is expected. Local focused validation covers the introducing PR,
and subsequent PRs use the landed base-pinned checker. Executing PR-authored
code to make the first trusted check green is forbidden.

## Current evidence and readiness

The current compiler parses and lowers `while`, `break`, and `continue`, rejects
loop control outside `while`, executes runtime loop bodies with nested index
traversal and mutable-slice writes, and rejects `for` with a stable gap
diagnostic. It does not yet parse `for`, materialize iterator protocol types,
represent iterator nodes through every semantic IR, or run the required
collection and ownership matrix directly.

Production therefore remains `blocked` at `syntax_only`. Global readiness is
false and every iteration completion boolean remains false. Promotion requires
all collection kinds and modes, nested loop control, deterministic map and text
order, invalid mutation/move/borrow/dynamic-dispatch diagnostics, explicit
semantic IR nodes, runtime-origin sensitivity from stdin, files, environment,
HTTP, and prior function results, and build-once/run-many proof on each
supported target. Until then the cutover diagnostic is
`language.iteration_control_not_qualified`.
