# Partial package admission and hostile cleanup state

Issue #1660 includes poisoned or incomplete package transactions. The
`package_store::hostile_state_tests` unit tests exercise existing store behavior
without changing production code or the storage contract.

Run the focused tests and the existing store regressions with:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib package_store:: --locked
```

The tests reconstruct the three intact publication prefixes before the final
admission record: blob only, blob plus tree, and blob plus tree and evidence.
Both ordinary and exact offline loads must reject each incomplete package.
An unrelated committed package remains readable, and re-admitting the same
verified material must recover a readable, byte-exact package and clean its
temporary transaction. These are reconstructed filesystem states, not injected
process termination or power-loss tests. The fixture's evidence bytes are
synthetic; these tests do not prove cryptographic authentication.

Cleanup fixtures cover missing, directory-shaped, empty, truncated, invalid
UTF-8, oversized and mismatched owner markers. Valid stale controls must be
reclaimed while unowned payloads remain untouched. Deterministic timestamps
exercise the exact stale-age boundary, a transaction one nanosecond too young,
and a future timestamp without sleeps. Unix-only tests preserve a live owner
despite an old timestamp, reject transaction/marker symlinks, and check that
reclaiming an owned directory containing a symlink leaves its external target
unchanged. The dead-owner fixture uses an asserted absent PID.

The existing Full Lib Suite executes these tests; no CI wiring is added.
Existing vendor lifecycle tests separately cover concurrent lock exclusion and
process-exit lock release. This slice does not exhaust filesystem races,
platform-specific crash durability, or the complete #1660 invariant audit and
registry/offline/cache/signature/vendor acceptance. It references #1660 without
closing it.
