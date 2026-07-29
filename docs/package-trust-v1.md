# Package Trust v1 Contract

Package Trust v1 freezes an implementation-language-neutral contract for
authenticating registry packages with RFC 8032 Ed25519 signatures and SHA-256
content digests. It is a contract and vector bundle, not a claim that `axiomc`
already implements this verifier. The current Rust-hosted `publish` and
`registry-*` commands still use their pre-existing local HMAC sidecars.

The canonical contract-only fixture is
`stage1/package-trust/contract/package-trust.json`. Its five published schemas
cover the package signature, trust-root metadata, registry index v2,
verification expectation, and verification result. Validate the complete
bundle with:

```bash
make stage1-package-trust-contract
make stage1-package-trust-contract-test
```

## Package signing transcript

An Ed25519 signature is computed over the transcript bytes directly, using the
pure Ed25519 mode from [RFC 8032](https://www.rfc-editor.org/rfc/rfc8032).
SHA-256 authenticates archive, manifest, provenance, transcript, and metadata
content; Package Trust v1 does not substitute a pre-hashed Ed25519 mode.

The `axiom-tlv-v1` package transcript is:

1. the two-byte big-endian byte length of `AXIOM-PACKAGE-TRUST-V1`;
2. those ASCII domain bytes;
3. the two-byte big-endian field count; and
4. for each field in the fixture's exact `field_order`, a two-byte big-endian
   UTF-8 label length, label bytes, eight-byte big-endian value length, and
   value bytes.

Integers are unsigned 64-bit big-endian values. SHA-256 values are their raw
32 bytes. Other values are NFC UTF-8. The signed fields bind the scheme and
version, archive digest and length, manifest digest, namespace, package name,
SemVer version, target path, registry and source identities, publisher and key
identities, in-toto statement digest, SLSA predicate type and subject, and
registry index generation and sequence. The fixture stores the exact transcript
hex and its SHA-256 so independent implementations can reproduce it byte for
byte.

Root and index metadata use `axiom-canonical-json-v1`: NFC UTF-8 JSON with keys
sorted by Unicode code point, no insignificant whitespace, lowercase JSON
literals, and integer-only numbers. The metadata transcript is the two-byte
domain length, domain bytes, eight-byte canonical-payload length, and payload
bytes. Package, root, root-transition, and index envelopes carry `signatures`
arrays. A threshold counts unique, valid public-key fingerprints, never
signature entries or repeated key IDs. Required package key IDs must all
contribute to the satisfied threshold.

A key ID is `sha256:` plus the SHA-256 of the canonical JSON key object
containing exactly the algorithm, public-key encoding, and public-key bytes.
The checker derives this value instead of trusting the supplied ID, rejects
duplicate public-key material under multiple IDs, and checks role membership,
delegation, grants, and key lifecycle before counting a signature. Every
package signer counted toward a threshold must have a key-level publisher
identity exactly equal to the requested publisher, and that publisher,
package, namespace, registry, source, and role must match one namespace grant.
A role containing a valid key for another publisher cannot contribute to the
requested publisher's threshold.

Key rotation links are also authenticated root data. Every
`supersedes_key_ids` entry must resolve to an earlier key for the same
publisher, the predecessor must already be retired or revoked, the successor
must be active with a strictly later `valid_from_sequence`, and the graph must
be acyclic. Revocation effective sequences cannot precede key validity.

The fixture-only RFC 8032 verifier rejects wrong-length keys or signatures,
non-canonical or small-order public keys and `R` encodings, and `S` values
outside the Ed25519 scalar range. It also proves itself against the RFC 8032
empty-message test vector. This test helper is not the production package
verifier.

## Trust and registry metadata

The trust root borrows security properties from
[TUF 1.0.28](https://theupdateframework.github.io/specification/v1.0.28/):
versioned and expiring metadata, consistent snapshots, root and delegated role
thresholds, publisher identities, namespace/package grants, active, retired,
and revoked keys, rotation links, and revocation effective sequence and time.
This is an Axiom metadata format, not a wire-compatible TUF implementation.
Its combined `registry-index` role is Axiom's equivalent of timestamp and
snapshot selection for this contract; verifiers must not label its bytes or
role graph as TUF metadata.

A root transition accepts exactly root `N+1`. The candidate root's canonical
bytes must satisfy the old root role threshold and the candidate root role
threshold independently before the new root is pinned. Repeated signatures
from one fingerprint do not satisfy either side. The vectors reject skipped or
unchanged versions, old-only and new-only authorization, duplicate signers,
expired previous roots, rollback, and signatures made by keys that are
not-yet-valid, retired, or revoked at the effective sequence and verification
time.

The first root is not trusted because it is self-signed. The verification
expectation carries an out-of-band `trusted_root_anchor` containing the exact
old root version, sequence, and canonical transcript SHA-256. The evaluator
checks that anchor before using the old root's keys. The candidate sequence
must be strictly greater than the anchored root sequence and cannot fall below
the retained highest root sequence. A consistently re-signed attacker root or
sequence rollback therefore remains rejected. The attacker-root vector uses a
valid attacker self-signature threshold and valid attacker-key signatures over
the unchanged candidate transcript, so `ROOT_BOOTSTRAP_MISMATCH` is its only
failure.

Verifiers retain highest-seen root version, index generation and sequence,
package version, and authenticated snapshot state. Each `seen_snapshots` row
records generation, sequence, snapshot identity, and canonical index transcript
SHA-256. Re-verifying the exact four-field row is an allowed idempotent repeat.
A lower authenticated generation or sequence is rollback; rebinding a seen
coordinate, snapshot identity, or transcript digest to different authenticated
metadata is replay. Unauthenticated metadata fails signature or transcript
validation and does not establish replay history. Verifiers reject rollback,
replay, SemVer-aware prerelease downgrade, and expired metadata. Target paths must be
safe relative paths: no absolute path, empty component, `.` or `..`, backslash,
or NUL. Registry releases must separately have unique target paths and unique
package coordinates `(registry, source, namespace, name, version)`, in addition
to unique complete release tuples. This prevents two signed releases from
creating an ambiguous cache key or target.

`offline_locked` operation explicitly forbids network fallback and requires
every input to be present. Its lock is exact, not advisory: root version,
sequence, and transcript digest; index generation, sequence, and transcript
digest; and the selected release's registry/source/package/publisher identity,
archive length and digest, manifest digest, provenance statement digest,
predicate type and selected subject, target path, and package-signature digest
must all match. Missing material produces `OFFLINE_INPUT_MISSING`; present but
different material produces `OFFLINE_LOCK_MISMATCH`.

Registry index v2 signs the registry identity itself, not a URL suffix. Its
expiring, monotonic signed payload binds releases, target paths, archive length
and SHA-256, manifest SHA-256, package-signature SHA-256, publisher/key
identity, and provenance. Verification compares the complete request against
the package envelope, selected index release, delegated role, and namespace
grant; a match inferred from only a name or URL is insufficient.

Provenance follows the
[in-toto Statement specification](https://github.com/in-toto/attestation/blob/main/spec/v1/statement.md):
the exact statement value contains `_type`, a subject array, `predicateType`,
and `predicate`. Its canonical bytes and SHA-256 must match, the selected
subject must be an exact member of the subject array, and that subject's name
and SHA-256 must bind the selected target and archive. The statement digest,
SLSA predicate type, selected subject, and canonical statement are compared
across the request, package, index release, and offline lock.

For predicate type `https://slsa.dev/provenance/v1`, the predicate uses the
official SLSA Provenance v1 structure. `buildDefinition` contains `buildType`,
`externalParameters`, `internalParameters`, and `resolvedDependencies`.
`runDetails` contains `builder` identity/dependencies/version, invocation
metadata with ordered start/finish times, and `byproducts`. Resource
descriptors carry a URI and non-empty digest map. The former fixture-only
`build_definition_sha256` object is not accepted as a SLSA v1 predicate.

## Verification results and vectors

Verification results expose observed package/source identity, signer keys and
roles, archive and manifest digests, provenance, threshold state, offline mode,
and a deterministic trust decision. The evaluator collects every applicable
failure, orders the complete set by the published precedence, and reports the
first as `primary_reason_code`; it does not stop at the first check. The stored
result is cross-checked field-for-field against the evaluator's published
`axiom.package_verification.v1` shape.

The vectors cover every stable reason code and include multi-failure ordering,
duplicate JSON members, tampered archive and index data, malformed and
cryptographically invalid signatures, identity/non-canonical/small-order
points, unknown and revoked keys, mixed-publisher thresholds, invalid
supersession graphs, wrong publisher/namespace/name/version/source or target,
unsafe paths, ambiguous cache targets/package coordinates, exact-repeat and
rebound snapshot behavior, bootstrap-anchor substitution, replay, rollback,
SemVer prerelease downgrade, expiry, threshold failure, grant/delegation
failure, and official SLSA predicate mismatch. JSON parsing rejects duplicate
object member names before schema or semantic evaluation.

This slice intentionally remains `contract_only`. It does not close issue
`#1458`, authenticate existing HMAC artifacts as Ed25519, or qualify a hosted
registry implementation.
