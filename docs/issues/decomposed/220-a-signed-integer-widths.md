---
parent: 220
title: "Numeric tower A: signed integer widths (i8 / i16 / i32 / i64)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
---

Part of #220 / full numeric tower. Add `i8`, `i16`, `i32`, and `i64` as first-class signed integer types alongside the existing `int` floor.

## Scope

- New `Type::Numeric(NumericType::I8 | I16 | I32 | I64)` variants and unification.
- Parser accepts the type names in let bindings and function signatures.
- Codegen lowers each to the matching Rust integer.
- Mixed-width arithmetic requires explicit cast (`as i32`) for now — implicit promotion is a separate sub-issue.
- Overflow behavior matches Rust's debug builds (panic on overflow); release builds wrap. Document this in `docs/stage1.md`.

## Acceptance

- Pass fixture covers each width's literal range bounds.
- Fail fixture: `let x: i8 = 200` rejected at type check (literal out of range).

## Out of scope

- Unsigned widths — 220-b.
- Floats — 220-c.
- Cross-width casts — 220-d.
- Literal suffixes — 220-f.
- Overflow policy beyond the documented default — 220-e.
