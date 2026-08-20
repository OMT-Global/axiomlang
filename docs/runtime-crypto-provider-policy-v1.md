# Runtime Crypto Provider Policy v1

Runtime Crypto Provider Policy v1 is the review-candidate provider and
algorithm decision tracked by Refs #1481. It fixes target-neutral requirements
that a later runtime implementation and qualification must satisfy. It does not
claim that direct-native runtime crypto, an OpenSSL distribution, either target
ABI, or cross-target equivalence is implemented or qualified. Issue #1481 must
remain open until that executable evidence exists.

The normative machine-readable artifacts are:

- `stage1/compiler-contracts/snapshots/runtime-crypto-provider-policy-v1.json`
- `stage1/compiler-contracts/schemas/axiom.runtime_crypto_provider_policy.v1.schema.json`
- `stage1/compiler-contracts/fixtures/runtime-crypto-provider-policy-v1/`

## Checked-in state and activation

The snapshot is deterministically `review_candidate`. Merging this policy
changes no status: `merge_effect` is `none`. Activation requires a separate
security- and verification-reviewed commit on trusted `main` that adds the
reserved activation artifact and changes the checked-in state. The current
snapshot records that the artifact is absent and the checker rejects an
artifact/state disagreement. Git cannot implicitly rewrite the policy to
`active` during merge.

The policy remains `static_spike` / `partial`, with
`production_qualified: false`. The explicit blockers are executable algorithm
vectors, target-specific artifacts and ABIs, signer/attestation/SBOM evidence,
and executed Linux/macOS equivalence evidence.

## Algorithm and encoding requirements

Algorithm identifiers are versioned AxiOM contract identifiers. Provider type
names and provider-specific errors do not become public AxiOM semantics.

| Identifier | AxiOM surface | Required sizes and encoding |
| --- | --- | --- |
| `sha2-256@1` | `sha256` | Arbitrary supported runtime bytes or UTF-8 text; 32-byte digest encoded as lowercase hex. |
| `hmac-sha2-256@1` | `hmac_sha256`, `verify_sha256` | 14–65,536-byte key outside published test vectors; 32-byte tag encoded as lowercase hex. |
| `hmac-sha2-512@1` | `hmac_sha512`, `verify_sha512` | 14–65,536-byte key outside published test vectors; 64-byte tag encoded as lowercase hex. |
| `aes-128-gcm@1` | `Aes128Gcm` seal/open | 16-byte key, 12-byte nonce, 16-byte tag; output is `ciphertext || tag`. |
| `aes-256-gcm@1` | `Aes256Gcm` seal/open | 32-byte key, 12-byte nonce, 16-byte tag; output is `ciphertext || tag`. |
| `chacha20-poly1305@1` | `ChaCha20Poly1305` seal/open | 32-byte key, 12-byte nonce, 16-byte tag; output is `ciphertext || tag`. |
| `ed25519@1` | keygen, sign, verify | 32-byte public key, canonical 32-byte private seed, and 64-byte signature. |

Ed25519 key generation returns the 32-byte seed as the private value. Signing
rejects every other private-key length, including the former unchecked 64-byte
`seed || public` shape. This avoids silently ignoring an inconsistent public
half.

The approved primitives are SHA-256 and HMAC-SHA-2, AES-GCM,
ChaCha20-Poly1305, and pure Ed25519. This policy does not approve X25519, Ed448,
pre-hashed Ed25519, alternate GCM nonce lengths, truncated tags, or
nonce-reuse-misuse-resistant AEAD modes. AEAD callers remain responsible for
nonce uniqueness.

`algorithm-vectors.json` is an authoritative HTTPS vector-source catalog, not
an executable vector suite. It contains no exact keys, inputs, outputs, or
encodings to execute, and every row is `not_executed`. Likewise,
`failure-matrix.json` is a closed failure catalog, not proof that a provider
produced those failures. Qualification needs exact published material wired to
executed target tests.

## Provider requirements are not qualification

The requirement is an exact OpenSSL 3.5.x EVP artifact per target, using an
isolated configuration and provider search path, with the OpenSSL `default`
provider selected explicitly. Ambient host-provider loading is forbidden.
`fips_claim` is `none`.

Each target has a representable evidence record for:

- provider version;
- artifact and SBOM SHA-256 digests;
- signer identity;
- attestation identity and subject;
- target ABI;
- isolated configuration and provider loading;
- explicit OpenSSL default-provider selection; and
- executable equivalence evidence.

All of those records are currently `missing`, so each target and the aggregate
remain unqualified. A requirement declaration, version range, or provider name
does not satisfy qualification.

The current compatibility backend loads ambient OpenSSL/libcrypto and is
recorded only as `compatibility_backend_ambient_loading_only`. Both
`linux-x86_64` and `macos-arm64` retain explicit implementation, provider,
target-ABI, and executable-equivalence gaps. “Equivalent” is a requirement,
not a present-tense evidence claim.

An unsupported target returns `unsupported_target`; it must not silently load a
different algorithm provider or entropy source.

## Entropy, runtime effects, and failures

Linux fills buffers with `getrandom(flags=0)` in chunks no larger than 256
bytes, retrying interrupted and partial reads until the requested chunk is
complete. macOS uses `SecRandomCopyBytes(kSecRandomDefault)` and accepts only
`errSecSuccess` for the complete request. A single requested runtime buffer is
bounded to 65,536 bytes in v1.

The operating-system API success result is the health signal.
Application-level statistical tests are forbidden because they are not a
substitute for provider health checks. Any terminal failure or partial result
discards the entire destination and returns `entropy_unavailable`. There is no
alternate device, PRNG, timestamp, or provider fallback.

Crypto effects are runtime-only. Hash, MAC, entropy, AEAD, and signature
operations must not execute during compilation. Build evidence may carry only
closed provider and contract metadata, never runtime values.

The stable failure codes are:

- `allocation_failed`
- `authentication_failed`
- `capability_denied`
- `entropy_unavailable`
- `invalid_key_length`
- `invalid_nonce_length`
- `malformed_input`
- `provider_failure`
- `provider_unavailable`
- `unsupported_algorithm`
- `unsupported_target`
- `verification_failed`

Failures are fail-closed. Provider failures return no output, AEAD
authentication failures return no plaintext before authentication succeeds,
partial entropy is discarded, and verification failure returns false with only
a stable code.

## Closed inspection and secret handling

Inspection is a closed value contract, not a field-name list and not a
prefix-based “redaction” claim. The checker enforces the exact report fields,
approved algorithm/operation combinations, provider and target enums,
operation-specific input-length fields, fixed outcome status/code pairs, and
closed nested examples for serialized inspection, logs, traces, errors, and
evidence.

Key identity is limited to the fixed states `opaque_runtime_handle` and
`not_applicable`; it cannot carry arbitrary identifier text or encoded secret
material. AEAD, MAC, and signing operations use the opaque state. Hashing,
entropy fill, key generation, and signature verification use `not_applicable`
because they do not consume a secret runtime key handle. Entropy reports use
the target-neutral `system-entropy@1` algorithm identity and bind the provider
to `linux-getrandom` or `apple-security-secrandom` for the reported target.
Representative hash and entropy reports keep both unkeyed profiles executable
in the checker. Marker-secret regressions cover raw, lowercase, hexadecimal, and
base64 marker forms across every serialized channel. Unexpected nested
structures and values fail closed.

Ciphertext, digests, generated bytes, keys, MACs, messages, nonces, plaintext,
private keys, secret keys, and signatures remain forbidden in evidence and
inspection channels. The policy requires best-effort zeroization of AxiOM-owned
buffers and provider contexts. It does not claim erasure of allocator copies,
immutable or foreign-provider copies, operating-system caches, or crash dumps
without platform hardening.

## Trusted CI and bootstrap limitation

The PR fast-check job executes the checker and self-test from the base-pinned
`.trusted-ci` checkout. The PR-head checkout is data only and is supplied
explicitly as `--root "$repo_root"`. All policy/schema/fixture reads, including
self-test harness copies, walk from an opened root descriptor with
`O_NOFOLLOW | O_NONBLOCK`, reject unsafe path syntax and nonregular files, and
enforce a 1 MiB UTF-8 JSON bound.

The first PR that introduces this trusted checker cannot be enforced by the
base-pinned job because the base revision does not contain the checker or its
run-fast wiring. That bootstrap PR therefore requires the focused local
evidence and separate review recorded here; enforcement begins only after the
trusted files land on `main`. PR-head checker code is never executed under the
trusted label.

Run the focused gate with:

```bash
make stage1-runtime-crypto-provider-policy-v1
python3 scripts/ci/test-check-runtime-crypto-provider-policy-v1.py --root "$PWD"
python3 scripts/ci/check-runtime-crypto-provider-policy-v1.py --root "$PWD" --json
```

TLS, package signing, certificate validation, and package trust remain governed
by their own protocol and trust gates. They may consume a qualified runtime
crypto surface later; this policy does not activate them.
