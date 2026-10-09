# Bounded package archive mutation coverage

Issue #1660 tracks hostile package-input coverage in addition to the earlier panic-path audit. `package_archive_fuzz` supplies a deterministic, bounded archive-parser campaign without a new dependency or production change.

Run:

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test package_archive_fuzz --locked -- --nocapture
```

The ordinary fast-check script includes this target alongside schema metadata tests. The trusted-base PR workflow will consume that script addition after merge; direct local execution supplies the new target's pre-merge evidence.

The corpus includes empty and valid multi-file archives, binary payloads, duplicate paths, traversal, overflowing length text, invalid UTF-8 headers, and truncated content. The valid package seed lives beside the integration test; the binary payload and short malformed seeds are escaped byte strings so their exact framing and encoding are visible in review. Each seed has an explicit expected acceptance result.

Every truncation, single-bit flip, and insertion of NUL/newline/slash/space/0xff is replayed at every applicable byte boundary. The digest is recomputed for every candidate so mutated inputs reach structural parsing. Budgets cap archive bytes, file size, paths, file/directory counts, and tree entries; zero-byte-budget rejection is also exercised.

The oracle requires a structured, nonempty diagnostic or a successful byte-exact canonical round-trip and matching digest. A panic reports the seed, mutation position, and bytes for reproduction. Accepted and rejected counters ensure both paths are exercised. This is a finite mutation campaign, not coverage-guided fuzzing or a claim that arbitrary inputs are proven safe.

Remaining #1660 acceptance includes registry/index/package-metadata parsing coverage, injected transaction/crash/race cases, retained invariant justifications, and full valid offline/cache/signature/vendor evidence. This slice does not close #1660 and changes no extraction authority, runtime trust, or security policy.
