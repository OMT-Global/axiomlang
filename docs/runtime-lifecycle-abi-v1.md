# Runtime Lifecycle ABI v1

Runtime Lifecycle ABI v1 defines the target-neutral ownership and cleanup
contract used by Axiom before a backend can claim allocation, aggregate, or
capability-resource support. It is a semantic contract: an implementation may
choose its own storage representation, but it may not silently weaken the
ownership, cleanup, or failure rules below.

The machine-readable contract is
`stage1/compiler-contracts/snapshots/runtime-lifecycle-v1.json` and validates
against `stage1/compiler-contracts/schemas/axiom.runtime_lifecycle.v1.schema.json`.

Validate it with:

```bash
make stage1-runtime-lifecycle-v1
python3 scripts/ci/test-check-runtime-lifecycle-v1.py
```

## Values and ownership

Every lifecycle-relevant value has one of three ownership modes:

- `owned` has exactly one current owner and one outstanding cleanup obligation;
- `borrowed` has an explicit owner and a lexical borrow extent; and
- `shared` is copyable only when its declared type permits copying without a
  cleanup obligation.

`move` transfers an owned value and its cleanup obligation, invalidating the
source place. `copy` is valid only for a copyable type and creates no extra
cleanup obligation. `clone` invokes the type's declared clone operation and
creates a distinct owned value with its own cleanup obligation. A mutable
borrow is exclusive; no move, drop, or conflicting borrow is valid until the
borrow extent ends. A backend must diagnose a violation rather than infer a
new ownership mode.

## Allocation and aggregate destruction

`allocate` creates an owned allocation with a requested layout class and an
allocation effect. `resize` replaces the allocation only after the operation
succeeds; on failure, the original owner and allocation remain valid. An
allocation failure is an explicit control result, never an implicit null-like
value. Allocation cannot be treated as compile-time evaluation of an effectful
program.

An aggregate owns each owned child it contains. Dropping an aggregate destroys
its owned children in reverse declaration or insertion order, then releases the
aggregate allocation. Recursive destruction uses the same rule at every level.
Each cleanup obligation is discharged exactly once, even when a child cleanup
reports an error. A cycle or an ownership escape that prevents a deterministic
cleanup path is rejected unless a separately declared shared-resource contract
proves its release rule.

## Scope exit, defer, and errors

Every scope exit has one of these reasons: `normal_return`, `early_return`,
`error_return`, `panic_unwind`, or `cancellation`. On every reason, cleanup
runs in this order:

1. Execute eligible deferred actions in last-in-first-out order.
2. Drop owned locals in reverse introduction order.
3. Propagate the original exit reason, unless cleanup produces a documented
   fatal lifecycle diagnostic.

Deferred actions may observe values captured by their declaration, but must not
extend a borrow or capability authority after the enclosing scope ends.
Panic/unwind and error-return paths use the same cleanup obligations as normal
return paths; neither is permitted to skip an owned value or resource handle.

## Capability-resource handles

A resource handle is opaque, owned, and capability-scoped. It may move, but it
may not copy, outlive its capability authority, or be used after close/drop.
`close` is an explicit, single-discharge-checked lifecycle operation: a second close
is a `lifecycle.double_close` diagnostic, and a later use is
`lifecycle.resource_use_after_close`. When an owned handle leaves scope without
an explicit close, its registered close operation runs as part of drop. A
backend must not expose a raw host handle through inspection output.

## Backend declaration and inspection

A backend declares the lifecycle feature ids it supports, its allocation
failure model, and the diagnostics it can emit. Missing support must produce
`backend.unsupported_lifecycle_feature` with feature id, backend id, operation
id, and source span. It must not fall back to an undocumented host ownership or
allocator behavior.

Inspection records, for each operation, its allocation effect, ownership
transfer, borrow extent, outstanding cleanup obligations, resource authority,
and source provenance. Inspection reports symbolic resource identities only;
they never include host addresses, host handles, or capability secrets.

## Conformance

The contract fixture includes positive and negative coverage for normal return,
early return, error return, panic/unwind with defer, nested aggregates,
allocation failure, move/clone/copy distinction, borrow extent, resource close,
leak prevention, double free, use after free, and capability-resource escape.
Native leak or sanitizer evidence is an implementation-stage requirement; this
contract establishes the target-neutral behavior it must prove.

## Migration boundary

Semantic MIR v1 supplies the executable nodes and provenance that refer to this
contract. Lifecycle ABI v1 does not prescribe aggregate layout, a backend data
structure, an allocator implementation, or an ownership-analysis algorithm.
Those implementation slices remain separate from this accepted contract.
