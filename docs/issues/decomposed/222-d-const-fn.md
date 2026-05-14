---
parent: 222
title: "Const/static D: `const fn` (pure compile-time evaluation)"
labels: [stage1, language, type-system, phase-a, roadmap, area:lang, lane:daedalus]
depends_on: [222-a-const-sized-arrays]
---

Part of #222. Allow `const fn` declarations whose bodies are evaluated at compile time and may be used to compute `const` initializers.

## Scope

- `const fn name(...): T { … }` is parsed and lowered.
- Body restricted to pure arithmetic, calls to other `const fn`, `if`/`else`, and integer literals; calls into the host runtime are rejected.
- Used as `const N: int = compute(3, 4)` to drive array lengths and pattern positions.

## Acceptance

- Pass fixture: a `const fn` computing a const-sized array length.
- Fail fixture: `const fn` body calls `clock_now_ms` — rejected.

## Depends on

- 222-a (const-sized arrays exercise the use case).
