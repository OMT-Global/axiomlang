---
parent: 220
title: "Numeric tower E: documented overflow policy"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [220-d-cross-width-casts]
---

Part of #220. Decide and document the overflow policy for the full numeric tower so user code has predictable semantics.

## Scope

- Default in debug builds: panic on signed overflow, wrap on unsigned overflow (matches Rust).
- Default in release builds: wrap on signed and unsigned overflow.
- Provide explicit `value.wrapping_add(other)`, `value.checked_add(other) -> Option<T>`, and `value.saturating_add(other)` helpers in `std/num.ax` for every supported width.
- `docs/stage1.md` and `docs/style.md` capture the policy.

## Acceptance

- Pass fixture demonstrates `wrapping_add` on `i8::MAX`.
- Pass fixture: `checked_add` on `i32::MAX` returns `None`.
- Fail fixture: ambient `i32 + i32` overflow panics in debug build with the documented diagnostic.

## Depends on

- 220-d (casts are the helper signature parameter shape).
