# Stage1 AEAD

Stage1 exposes authenticated encryption through `std/crypto_aead.ax`, guarded
by `[capabilities].crypto = true`.

```axiom
import "std/crypto_aead.ax"

let ciphertext: [u8] = aead_seal(Aes256Gcm, key[:], nonce[:], aad[:], plaintext[:])
match aead_open(Aes256Gcm, key[:], nonce[:], aad[:], ciphertext[:]) {
Some(opened) {
print opened == plaintext
}
None {
print false
}
}
```

The supported algorithms are `Aes128Gcm`, `Aes256Gcm`, and
`ChaCha20Poly1305`. Keys are 16 bytes for `Aes128Gcm` and 32 bytes for
`Aes256Gcm` and `ChaCha20Poly1305`. Nonces are 12 bytes. The sealed byte array
is `ciphertext || tag`, with a 16-byte authentication tag appended to the
encrypted payload.

Opening malformed or unauthentic ciphertext returns `None`; plaintext is
returned only after the backend library verifies the authentication tag. The
current generated-Rust backend loads OpenSSL/libcrypto EVP AEAD routines at
runtime. Nonce-reuse-misuse-resistant modes are intentionally out of scope.
