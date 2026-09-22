# Stage1 Ed25519 Signing

Stage1 exposes Ed25519 through `std/crypto_sign.ax`, guarded by
`[capabilities].crypto = true`.

```axiom
import "std/crypto_sign.ax"

let message: [u8] = [104u8, 101u8, 108u8, 108u8, 111u8]
let keys: ([u8], [u8]) = ed25519_keygen()
let signature: [u8] = ed25519_sign(keys.1[:], message[:])

print ed25519_verify(keys.0[:], message[:], signature[:])
```

The stage1 key format is the RFC 8032 Ed25519 raw-key shape:

- Public keys are 32 bytes.
- Secret keys are the canonical 32-byte private seed.
- Signatures are 64 bytes.

`ed25519_sign(secret_key, message)` rejects every private-key length other than
32 bytes, including the former unchecked 64-byte `seed || public` shape.
Verification returns `false` for malformed public keys or signatures. The
current compatibility backend loads OpenSSL/libcrypto Ed25519 raw-key APIs at
runtime; X25519 and Ed448 are intentionally out of scope.

That ambient-loading description is the current compatibility behavior, not a
production provider qualification. The review-gated
[Runtime Crypto Provider Policy v1](runtime-crypto-provider-policy-v1.md)
requires a bundled, pinned, attested OpenSSL 3.5 artifact and forbids ambient
host-provider loading before this surface can be promoted.
