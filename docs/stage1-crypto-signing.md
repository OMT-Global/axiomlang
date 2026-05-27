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
- Secret keys are 64 bytes: 32-byte private seed followed by the 32-byte public
  key.
- Signatures are 64 bytes.

`ed25519_sign(secret_key, message)` accepts either the 64-byte stage1 secret key
or the 32-byte private seed. Verification returns `false` for malformed public
keys or signatures. The current Rust backend loads OpenSSL/libcrypto Ed25519
raw-key APIs at runtime; X25519 and Ed448 are intentionally out of scope.
