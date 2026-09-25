# Runtime Associative Collections v1

Runtime associative collections v1 (#1476) defines the review-gated semantic contract for `Map<K, V>` and `Set<T>`. Its machine-readable snapshot is `stage1/compiler-contracts/snapshots/runtime-associative-collections-v1.json`.

`Map` and `Set` are growable, runtime-owned collections. Construction, lookup/contains, insert-or-replace, removal, clear, length, capacity, and iteration return structured resource errors rather than panicking. `Map` stores one value per equal key; `Set` is the corresponding key-only surface.

Keys are valid only when their AxiOM equality is total, deterministic, and compatible with their hash. Supported v1 shapes are primitive scalars, text, tuples and enums recursively made of valid keys, and explicitly accepted immutable user-defined value shapes. Mutable, resource, function, borrowed, and float keys are rejected until a later contract gives them coherent equality and lifetime semantics. Equal keys always have the same stable hash.

The default order contract is insertion order: replacing an existing map value does not move its key; removing then reinserting appends; `clear` resets order. Iteration uses a mutation generation and fails closed with `concurrent_collection_mutation` if the collection changes after iterator creation. No host hash seed may affect order or serialized/compiler output. Any randomized hardening mode must be selected explicitly and still preserve the default observable order.

Implementations use collision chains with bounded probes, checked growth, and explicit entry/byte/load limits. Allocation failure, limit exhaustion, and an adversarial collision chain return structured errors without partial mutation. Borrowed lookup copies or returns a scoped borrowed result only while its collection borrow is live. Insert transfers owned key/value storage; clone and drop recursively clone/drop nested aggregates exactly once; aliases obey the ownership contract and cannot mutate during an active mutable borrow.

The v1 snapshot carries positive, negative, lifecycle, adversarial, and compiler-symbol-table proof rows. Runtime implementation remains gated on #1425, #1437, #1438, and #1440; this slice prevents an implementation from silently inheriting Rust `HashMap`/`HashSet` equality, seeding, or iteration.

## Inspection and model-comparison obligations

The contract pins target-neutral inspection fields for collection kind, semantic key/value types, length/capacity, iteration order/generation, allocation effects, resource authority, ownership/cleanup, source provenance, and collision/growth budgets. For a set, value_type is not_applicable; inspection must not expose host layout or addresses. The inspection fixture requires these boundaries after insert, replace, remove, and growth.

The randomized model-comparison fixture specifies seeded operation streams against a simple ordered association-list map and ordered unique-value set reference model across every accepted key shape. Contents, equality classes, length, and insertion order must agree after each operation; budget errors preserve pre-operation state. These are pinned **future runtime proof obligations**, not claims that runtime inspection or randomized differential execution is implemented or has passed. The current checker validates the contract and rejects missing obligations, including coupled schema/snapshot weakening.
