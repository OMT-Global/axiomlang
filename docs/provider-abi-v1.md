# Provider ABI v1

Provider ABI v1 (#1453) is the target-neutral boundary for safe native extensions. Its machine-readable snapshot is `stage1/compiler-contracts/snapshots/provider-abi-v1.json`.

The AxiOM-facing API exposes only versioned provider descriptors, capability-scoped operations, opaque integer handles, and owned or explicitly scoped borrowed byte/text values. It never exposes a provider address, C pointer, allocator callback, or value that can outlive its call/event acknowledgement. C pointers used for bounded descriptors inside a provider are not safe-surface values.

Loading fails closed: a target-policy selected candidate must resolve the required v1 entry point, negotiate a compatible version, pass signature/trust policy, and declare only known features. Host-default search paths, relative paths, ambient lookup, and unsigned candidates are denied; missing symbols, incompatible versions, invalid descriptors, and trust denial happen before provider code.

Handles are non-null, provider-scoped, generation-tagged opaque `u64` tokens. Create transfers ownership; close is idempotent and invalidates children; drop closes owned handles. Invalid, stale, cross-provider, or closed handles never dispatch.

Buffers have explicit byte length and `borrowed_call`, `borrowed_event`, or `owned_provider` ownership. Text is UTF-8 and not conventionally NUL-terminated. Bounds are checked before allocation/copy. Borrowed data cannot be retained; owned provider output is copied to a safe owned value and released on all paths.

Each call declares its capability/effect, which is checked and audited before dispatch. Audit records provider identity, operation class, capability, and decision—never buffer content, credentials, paths, addresses, or raw handles. Cancellation drains/acknowledges before teardown; v1 events are synchronous only.

Faults, ABI violations, invalid output, and unacknowledged cancellation become `provider_fault`; the runtime quarantines the provider and invalidates its handles. The reference C fixture demonstrates descriptor-only calls and deterministic release.
