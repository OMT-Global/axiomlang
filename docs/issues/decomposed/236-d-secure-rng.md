---
parent: 236
title: "Crypto D: secure random number generator"
labels: [stage1, area:stdlib, security, lane:daedalus]
---

Part of #236. Add a cryptographically secure RNG primitive backed by the host OS.

## Scope

- Host intrinsic `crypto_rand_bytes(out: &mut [u8]): bool` fills the buffer from `getrandom()` on Linux, `arc4random_buf` on macOS/BSD, `BCryptGenRandom` on Windows.
- `std/crypto_rand.ax` exposes `random_bytes(n: int): Vec<u8>` and `random_u64(): u64` gated on `crypto`.

## Acceptance

- Pass fixture: 32 bytes generated; two consecutive calls produce different bytes (statistically; smoke test not cryptographic).
- Capability denial test.

## Out of scope

- Deterministic PRNG for testing — separate stdlib module.
