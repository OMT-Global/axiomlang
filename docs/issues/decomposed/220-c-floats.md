---
parent: 220
title: "Numeric tower C: floats (f32 / f64)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [220-a-signed-integer-widths]
---

Part of #220. Add `f32` and `f64` floating-point types.

## Scope

- Literals: `3.14`, `1e10`, `2.5f32`, `1.0f64` accepted.
- Arithmetic, comparison, and (no) equality semantics match Rust: `==` on floats compiles but the diagnostic notes the precision pitfall.
- NaN, infinity, and signed-zero behavior documented in `docs/stage1.md`.
- Cross-precision (f32↔f64) and cross-kind (int↔f64) operations require explicit cast.

## Acceptance

- Pass fixture: numeric integration / area / mean over `[f64]`.
- Fail fixture: implicit `f32 + f64` rejected.

## Depends on

- 220-a.

## Out of scope

- Decimal / fixed-point — separate roadmap item.
