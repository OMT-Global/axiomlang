---
parent: 220
title: "Numeric tower B: unsigned integer widths (u8 / u16 / u32 / u64)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [220-a-signed-integer-widths]
---

Part of #220. Add unsigned integer widths.

## Scope

- `u8`, `u16`, `u32`, `u64` types with parser, HIR, and codegen support.
- Literals: `0u8`, `255u8`, etc. accepted; negative literal at unsigned type rejected.
- Bitwise operations (`&`, `|`, `^`, `<<`, `>>`) follow the same precedence / parsing model as today's `int` operations.

## Acceptance

- Pass fixture exercises all four widths and bitwise ops.
- Fail fixture: `let x: u8 = -1` rejected.
- Fail fixture: `let x: u8 = 256` rejected (literal out of range).

## Depends on

- 220-a (signed widths establish the numeric scaffolding).
