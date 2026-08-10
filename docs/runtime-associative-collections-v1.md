# Runtime Associative Collections v1

Runtime Associative Collections v1 defines the target-neutral contract for
runtime maps and sets. It deliberately describes semantic behavior rather than
prescribing a host-language container or storage layout. The contract is
accepted as a design boundary; runtime implementation remains blocked on the
lifecycle, runtime-sized storage, and ownership contracts.

The machine-readable contract is
`stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json`
and validates against
`stage1/compiler-contracts/schemas/axiom.runtime_associative_collections.v1.schema.json`.

Validate it with:

```bash
make stage1-runtime-associative-collections-v1
python3 scripts/ci/test-check-runtime-associative-collections-v1.py
```

## Semantic contract

- `map` stores one value per semantic key. Inserting an existing key replaces
  its value without changing that key's iteration position.
- `set` stores each semantic key at most once. Re-inserting an existing key is
  idempotent.
- v1 keys are `bool`, `int`, and `text`. Equality is type-aware and semantic;
  a text key is never equal to an integer or boolean key with a similar printed
  form.
- Hashing is deterministic for the same key value and contract version. Hash
  output is an implementation detail and is not exposed through inspection.
- Iteration is deterministic insertion order. Removing and re-inserting a key
  gives it a new position; replacement preserves its existing position.
- Lookup, insertion, replacement, removal, membership, length, and iteration
  are distinct semantic operations. Unsupported key shapes and unbounded
  operations fail with stable diagnostics.

## Limits and lifecycle

Collections have explicit element and operation limits. Exceeding a limit or
failing to allocate returns a failure result without silently dropping existing
entries. A collection owns its entries, is cleaned up exactly once, and may not
outlive the authority or borrow extent that created it. Iteration observes a
stable snapshot or a declared mutation diagnostic; it must not expose an
implementation cursor or host address.

The contract does not claim runtime implementation, direct-native lowering,
allocator behavior, or readiness completion. Those proofs require runtime-sized
storage (#1425), lifecycle (#1438), and ownership analysis (#1440).
