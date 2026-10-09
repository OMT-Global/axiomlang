# Bounded package metadata mutation coverage

This partial #1660 slice exercises the public package-signature, trust-root, registry-index and verification-expectation parsers. It complements the separate archive-parser campaign and changes no production parsing, trust policy, cryptographic verification or storage behavior.

```sh
cargo test --manifest-path stage1/Cargo.toml -p axiomc --test package_metadata_fuzz --locked -- --nocapture
```

The existing checked-in Package Trust contract supplies one positive seed per document type. Each seed must parse. At no more than 65 deterministic byte positions per document, the test tries truncation, invalid-byte insertion, high-bit flipping and deletion. Successful parsing must preserve the JSON value and survive serialization/reparse; rejection must carry a nonempty diagnostic. Panic reports identify document, byte position and mutation. This finite campaign is not coverage-guided fuzzing, and parser acceptance is not authenticated trust acceptance.

Explicit negative controls cover ordinary and Unicode-escaped duplicate object members, the public document-size limit, excessive nesting, invalid UTF-8, truncated strings, trailing JSON, overflowing numeric exponents and wrong root types. Every negative control must reject; counters are diagnostic rather than a fixed implementation-dependent acceptance ratio.

The fast-check script runs this target after Package Trust contract checks. Because PR scripts are trusted-base pinned, the new invocation reaches that lane after merge; direct local execution provides pre-merge evidence. Existing package-trust CLI tests remain the authority for external error-code mapping.

Uncovered #1660 scope remains: transaction/crash/race injection, filesystem extraction and symlink behavior, inventory and justifications for retained invariants, and full valid registry/offline/cache/signature/vendor acceptance. This PR references #1660 without closing it.
