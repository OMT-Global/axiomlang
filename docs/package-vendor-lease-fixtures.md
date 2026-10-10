# Vendor reader lease marker fixtures

The `package_store::lease_marker_tests` suite exercises public `vendor_snapshot`
reclamation with an old snapshot and a verified current snapshot. Corrupt,
truncated, non-UTF-8 or oversized marker bytes and malformed/overflowing names
must defer reclamation as `unreadable_reader_lease`, preserving old package bytes.
An old live-PID marker and a future timestamp defer as `active_reader`.

Unix fixtures also verify that a symlink marker preserves its external target
and old snapshot, while a valid stale dead-PID marker permits reclamation.
Every retention fixture removes its marker and verifies subsequent reclamation;
the current snapshot must still verify. This detects both unsafe deletion and
cleanup that never makes progress.

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib lease_marker_tests --locked
```

These are static-state regression fixtures for #1660, not a new lease contract.
They do not prove concurrent replacement safety, crash/power-loss recovery,
PID-reuse handling, or Windows behavior. The dead-PID cleanup and symlink cases
are Unix-only; the fixture asserts its chosen PID is not alive. No whole-issue
closure or production behavior change is claimed.
