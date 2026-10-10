# Bounded cache admission contention coverage

Issue #1660 includes cache races. `package_store::contention_tests` exercises
the existing immutable-publication path with four writers released by a common
barrier. Each test repeats three times with a fresh private cache. A reader makes
64 bounded probes per round, allowing only an absent or ambiguous committed
identity, or a complete package whose bytes match one of the submitted identities.
After joining every writer, the tests require successful exact loads and no
leftover writer transactions.

One test admits identical artifacts concurrently and requires all writers to
converge on the same blob, tree, evidence directory and commit. The other admits
two distinct index/verification identities for a shared archive, with two writers
per identity. It requires byte-exact selector-specific replay, shared content,
separate evidence, ambiguity for an unqualified load, and rejection of selectors
that combine an index with the other identity's verification digest.

Run:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --lib package_store:: --locked
```

The existing Full Lib Suite runs these tests; no new CI wiring is needed.
Scheduling determines which interleavings occur. The barrier creates contention
opportunities without sleeps, but this bounded campaign does not prove all races,
guarantee a read between particular publication steps, or simulate process death,
power loss, malicious filesystem replacement, or separate processes. Evidence
bytes are synthetic and test integrity/identity behavior, not authentication.
The partial-state tests from #1751 separately pin rejection before the final
admission record. No production semantics change, and #1660 remains open for its
remaining audit, hostile-input and platform acceptance criteria.
