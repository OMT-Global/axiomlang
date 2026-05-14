---
parent: 236
title: "Crypto B: AEAD (AES-128-GCM, AES-256-GCM, ChaCha20-Poly1305)"
labels: [stage1, area:stdlib, security, lane:daedalus]
---

Part of #236. Add authenticated encryption primitives as the foundational symmetric encryption surface.

## Scope

- Host intrinsics: `crypto_aead_seal(alg: string, key: &[u8], nonce: &[u8], aad: &[u8], plaintext: &[u8]): Vec<u8>` and `crypto_aead_open(alg: string, key, nonce, aad, ciphertext): Option<Vec<u8>>`.
- `std/crypto_aead.ax` wraps the intrinsics with typed enums for the algorithm; rejects calls without the `crypto` capability.
- Use `ring` or `aws-lc-rs` for the underlying primitives in the Rust runtime.

## Acceptance

- Pass fixture: round-trip AES-256-GCM seal + open recovers the plaintext.
- Pass fixture: tampered ciphertext returns `None` from `open`.
- Constant-time verification documented as part of the underlying library guarantees.

## Out of scope

- Nonce-reuse-misuse-resistant modes — separate follow-up.
