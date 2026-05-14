---
parent: 222
title: "Const/static A: const-sized arrays"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
---

Part of #222. Allow array types to use a `const`-evaluated integer for their length: `[int; N]`.

## Scope

- `const N: int = 8; let buf: [int; N] = [0, 0, 0, 0, 0, 0, 0, 0]` typechecks.
- Length must be a `const` expression with type `int` (or another supported integer type once #220 lands).
- Mismatched element count is a deterministic compile error.

## Acceptance

- Pass fixture using a top-level `const` for the array length.
- Fail fixture: literal length doesn't match the const-evaluated length.

## Out of scope

- Named constants in match-pattern positions — 222-b.
- `const fn` — 222-d.
- Generic const parameters — separate follow-up.
