---
parent: 222
title: "Const/static C: module-scope `static` (non-scalar)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
---

Part of #222. Lift the current scalar-only `static` restriction so module-scope statics can hold strings, tuples, and small aggregates.

## Scope

- `static GREETING: string = "hello"` and `static POINT: (int, int) = (0, 0)` typecheck.
- Non-scalar static initialization runs at program startup (no `const fn` requirement).
- Statics remain immutable; address-taking is still out of scope (separate follow-up).

## Acceptance

- Pass: aggregate static read in `main`.
- Fail: `static GREETING = "hello"` (missing type annotation) — current rule that statics need explicit types.

## Out of scope

- Mutable statics — explicitly off-roadmap.
- `&static`-style address-taking — separate issue.
