---
parent: 236
title: "Crypto E: constant-time primitives surfaced in std/crypto.ax"
labels: [stage1, area:stdlib, security, lane:daedalus]
depends_on: [236-a-hmac-variants]
---

Part of #236. Surface the constant-time helpers used by HMAC verify as first-class stdlib exports so they're usable by application code (token verification, capability tags, etc.).

## Scope

- Re-export the existing `constant_time_eq` helper from `std/crypto.ax` (a new top-level module that re-exports from `crypto_hash.ax`, `crypto_mac.ax`, `crypto_aead.ax`, `crypto_sign.ax`, `crypto_rand.ax`).
- Add `constant_time_eq_u8(left: &[u8], right: &[u8]): bool` for raw byte buffers (the existing helper compares `string`).
- Property test (#561 / Phase-I.3 once available) asserts that swapping two equal byte buffers never short-circuits.

## Acceptance

- Pass fixture imports `std/crypto.ax` and uses both string and byte variants.
- Documentation note in `docs/stage1.md` explaining the threat model.

## Depends on

- 236-a (HMAC verify uses the same helper).
