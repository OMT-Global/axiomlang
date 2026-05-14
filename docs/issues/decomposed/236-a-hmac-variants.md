---
parent: 236
title: "Crypto A: HMAC variants (SHA-256, SHA-512) with constant-time verify"
labels: [stage1, area:stdlib, security, lane:daedalus]
---

Part of #236. The current `std/crypto_mac.ax` exposes `hmac_sha256` and `constant_time_eq`. Add `hmac_sha512` and document the constant-time verification contract.

## Scope

- Host intrinsic `crypto_hmac_sha512(key: string, message: string): string` mirrors the existing SHA-256 intrinsic shape.
- `std/crypto_mac.ax` adds `hmac_sha512(key, message)` and a documented `verify(tag, expected_key, expected_message): bool` helper that wraps `hmac_*` plus `constant_time_eq`.
- Stage1 vector tests check against RFC 4231 test vectors.

## Acceptance

- Pass fixture: SHA-512 vector against an RFC test case.
- Pass fixture: `verify` returns `false` for a tampered tag and the timing is roughly equal to a verified-good case (smoke timing test, not security oracle).

## Out of scope

- AEAD — 236-b.
- Asymmetric signing — 236-c.
