---
parent: 220
title: "Numeric tower F: literal suffixes"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [220-a-signed-integer-widths, 220-b-unsigned-integer-widths, 220-c-floats]
---

Part of #220. Add literal-suffix parsing so authors can write `1i32`, `2u64`, `3.14f32` without an explicit `as` cast.

## Scope

- Parser recognizes the suffixes `i8`/`i16`/`i32`/`i64`, `u8`/`u16`/`u32`/`u64`, `f32`/`f64`.
- Type inference treats the suffixed literal as that exact width; an annotated `let` of a different width with a same-typed RHS literal is unambiguous, while a mismatch fails check.

## Acceptance

- Pass: `let x: i32 = 1i32`.
- Pass: `let pi: f32 = 3.14f32`.
- Fail: `let x: i64 = 1i32` (literal suffix conflicts with annotation).

## Depends on

- 220-a, 220-b, 220-c.
