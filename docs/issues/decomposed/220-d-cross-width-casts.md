---
parent: 220
title: "Numeric tower D: explicit cross-width casts"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [220-a-signed-integer-widths, 220-b-unsigned-integer-widths, 220-c-floats]
---

Part of #220. Allow explicit casts between any pair of numeric types via the existing `as` syntax.

## Scope

- `value as i32`, `value as f64`, `value as u8`, etc. — every combination is valid as long as both sides are numeric.
- Narrowing casts (e.g., `i32 as i8`) wrap silently; the diagnostic suggests adding a range check if the source value is statically out of range.
- Float-to-int truncates toward zero; out-of-range NaN traps as defined for the runtime.

## Acceptance

- Pass fixtures cover representative pairs: `i64 → i32`, `u8 → i32`, `i32 → f32`, `f64 → i64`.
- The `Cast` expression in HIR is rewritten only where the source and target widths actually differ; same-type casts pass through unchanged.

## Depends on

- 220-a, 220-b, 220-c.
