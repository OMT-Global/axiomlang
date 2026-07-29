# Package Trust v1 Contract

Package Trust v1 freezes an implementation-language-neutral contract for
authenticating registry packages with RFC 8032 Ed25519 signatures and SHA-256
content digests. The contract fixture and its vectors remain the normative
portable test bundle. Runtime-authored Package Trust inputs and results use the
same closed schemas and identify themselves as `implemented`. The Rust-hosted
`publish`, `registry-index`, `registry-validate`, `registry-serve`, and
`pkg verify` paths now implement the asymmetric Package Trust flow. Legacy
`axiom-hmac-sha256-v1` sidecars are not Package Trust inputs and are rejected.

The canonical contract-only fixture is
`stage1/package-trust/contract/package-trust.json`. Its five published schemas
cover the package signature, trust-root metadata, registry index v2,
verification expectation, and verification result. Validate the complete
bundle with:

```bash
make stage1-package-trust-contract
make stage1-package-trust-contract-test
```

Each of the five schemas permits exactly two `contract_status` values:
`contract_only` for the canonical fixture and `implemented` for values consumed
or produced by the runtime. Status identifies the producer boundary; it is not
a trust decision. The checked-in fixture deliberately remains `contract_only`.
Unknown status values and unknown fields remain rejected.

The verification expectation's `expected` object is an optional vector oracle.
The contract checker requires and evaluates it for the canonical fixture, but
production verification ignores it completely. Omitting `expected`, or changing
it in an otherwise identical production request, cannot change the runtime
decision.

## Runtime and command boundary

The library signing boundary is the key-storage-agnostic `Ed25519Signer`
provider: callers expose only a 32-byte public key and a `sign(message)`
operation. The provider API never accepts or returns secret key material, so
production callers can delegate to an HSM, keychain, or isolated signing
service. Every provider result is decoded, its key ID is derived from the
public key, and the signature is verified before publishing code accepts it.

The command-line adapter is intentionally narrower and local. `publish` and
`registry-index` accept one or more repeated `--signing-key-file` arguments.
Each file must contain exactly a 32-byte Ed25519 seed or 64 lowercase
hexadecimal characters and is bounded to 64 bytes. This file format is a CLI
adapter, not the library provider contract. Verification commands accept no
signing key or secret-bearing option.

The implemented registry lifecycle is:

1. `publish` builds the exact package archive, binds its packaged manifest and
   canonical provenance, signs the Package Trust transcript with distinct
   authorized package-role providers, and atomically writes the release and
   `package-signature.json`.
2. `registry-index` strictly reads those signature envelopes, requires an index
   threshold of at least two distinct authorized providers, emits a signed
   `axiom.registry_index.v2` envelope, and fully verifies every indexed release
   before returning the index.
3. `registry-validate` strictly loads the signed v2 index and re-verifies every
   release against the supplied roots and expectation using the exact archive,
   packaged `axiom.toml`, provenance, and package-signature bytes.
4. `registry-serve` performs the same complete verification before serving any
   response. It then snapshots the verified index and served release bytes in
   memory. Later filesystem replacement or tampering cannot change that
   running server's responses. `/` and `/index.json` expose the frozen index;
   only the verified archive, manifest, provenance, and signature paths are
   included in the served snapshot.

Registry file reads are bounded, reject symlinks and path escape, and compare
the exact local bytes rather than trusting index metadata alone. The server
supports `GET` and `HEAD`; malformed targets receive HTTP 400, unsupported
methods 405, and paths outside the snapshot 404.

All four production JSON inputs—package signature, trust roots, registry index
v2, and verification expectation—are parsed with duplicate-member rejection
and validated against the embedded published schema before semantic
verification. Runtime results are typed `axiom.package_verification.v1`
documents with `contract_status: implemented`; regression tests validate both
trusted and every pre-semantic failure shape against the result schema. Result
schema validation is a wire-contract check, not a substitute for the
cryptographic decision.

Package Trust metadata is bounded before parsing at 8 MiB per document. The
schemas additionally cap every signature array and satisfiable threshold at
16, root keys at 128, root roles at 64, namespace grants at 2,048, index
releases at 1,024, required package key IDs at 16, and role/supersession key-ID
arrays at 128. SLSA subject, dependency, and byproduct arrays are capped at
1,024; retained snapshot history at 10,000; and generic digest, version, and
parameter maps at 128 properties. Identifiers and package/role/display values
are bounded to 256 Unicode code points, registry/source/publisher/URI/builder
identities to 2,048, and relative paths and free text to 4,096. Fixed-size
digests, keys, and signatures retain their exact patterns. These are rejection
budgets, not truncation rules.

`axiomc pkg verify --json` writes exactly one verification result to stdout.
Exit status `0` means `trusted`, `1` means `rejected` (including missing,
unreadable, malformed, or cryptographically invalid input), and `2` is reserved
for failure to serialize or write the result. The registry lifecycle commands
return `0` on success and `1` on validation, signing, filesystem, or serving
diagnostics.

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
registry index publication-floor generation and sequence. The fixture stores
the exact transcript hex and its SHA-256 so independent implementations can
reproduce it byte for byte.

Root and index metadata use `axiom-canonical-json-v1`: NFC UTF-8 JSON with keys
sorted by Unicode code point, no insignificant whitespace, lowercase JSON
literals, and integer-only numbers. The metadata transcript is the two-byte
domain length, domain bytes, eight-byte canonical-payload length, and payload
bytes. Package, root, and index envelopes carry `signatures` arrays. Root
transition authorization carries old-root and new-root signatures over the
canonical candidate-root bytes. A threshold counts unique, valid public-key
fingerprints, never signature entries or repeated key IDs. Required package key
IDs must all contribute to the satisfied threshold.

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
threshold independently before the new root is pinned. The signed candidate
root's `issued_at` is the effective rotation time for expiry checks.
`transition_time` is retained only as unauthenticated compatibility metadata
and cannot affect trust, expiry, or key eligibility. The unsigned
`from_version` and `to_version` fields are consistency checks against the two
signed root versions, not authorization evidence. Repeated signatures from one
fingerprint do not satisfy either threshold. The vectors reject skipped or
unchanged versions, old-only and new-only authorization, duplicate signers,
previous roots expired at the signed effective rotation time, rollback, and
signatures made by keys that are not-yet-valid, retired, or revoked at the
effective sequence and verification time.

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

The generation and sequence signed into a package envelope are independent,
immutable publication floors, not an exact snapshot binding. After both the
package envelope and current registry index authenticate, the current index
generation must be at least the package generation floor and its sequence must
be at least the package sequence floor. Either current component below its
signed floor is `METADATA_REPLAYED`; newer current coordinates are valid. This
does not relax the exact current-index checks: the signed index transcript,
retained snapshot state, and offline-lock generation, sequence, and transcript
digest still identify the current index exactly.

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
The package envelope admits a bounded absolute predicate URI so a structurally
valid hostile value reaches semantic comparison. Rejected results may preserve
that observed URI; expectations and trusted results still require the exact
SLSA Provenance v1 predicate type.

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

The result schema discriminates evidence by `decision`. A `trusted` result has
`primary_reason_code: OK`, exactly `reason_codes: [OK]`, a non-empty signer
array, complete observed/archive/manifest/provenance/trust evidence, and
strictly positive threshold and valid-signer counts. A `rejected` result never
uses `OK`; it may report only evidence obtained before failure. Its observed
identity may be partial, its signer array may be empty, archive, manifest, and
provenance may be `null`, and trust evidence may be `null` or a closed partial
object whose available counts can be zero. These allowances do not apply to a
trusted result, and unknown fields remain invalid in both branches.

Malformed and unavailable production inputs use the existing stable reason
codes:

| Input condition | Required reason mapping |
| --- | --- |
| Offline input absent, unreadable, or missing required material | `OFFLINE_INPUT_MISSING` |
| Trust-root JSON, canonical payload, or transcript malformed | `ROOT_DIGEST_MISMATCH`, plus applicable root signature, threshold, bootstrap, rotation, or key-authorization reasons |
| Registry-index JSON, canonical payload, or transcript malformed | `INDEX_DIGEST_MISMATCH`, plus applicable index signature or threshold reasons |
| Package envelope or signature encoding malformed | `SIGNATURE_MALFORMED` when the signature/key encoding cannot be decoded; otherwise `SIGNATURE_INVALID` for a parseable but unauthentic package transcript or signature |

The runtime must return a schema-valid rejected result for these cases rather
than treating malformed untrusted bytes as a result-serialization error.

The vectors cover every stable reason code and include multi-failure ordering,
duplicate JSON members, tampered archive and index data, malformed and
cryptographically invalid signatures, identity/non-canonical/small-order
points, unknown and revoked keys, mixed-publisher thresholds, invalid
supersession graphs, wrong publisher/namespace/name/version/source or target,
unsafe paths, ambiguous cache targets/package coordinates, exact-repeat and
rebound snapshot behavior, bootstrap-anchor substitution, replay, rollback,
publication floors (including a signed current index below the package floor
and newer authenticated current indexes), SemVer prerelease downgrade, expiry,
threshold failure, grant/delegation failure, and official SLSA predicate
mismatch. JSON parsing rejects duplicate object member names before schema or
semantic evaluation.

The canonical vector bundle intentionally remains `contract_only`; operational
inputs and results are `implemented`. The asymmetric registry path accepts only
strict Package Trust JSON and exact authenticated artifacts. A legacy HMAC
sidecar is neither upgraded nor grandfathered: it fails strict JSON/schema
parsing and cannot be indexed, validated, served, or consumed as an Ed25519
package signature.
